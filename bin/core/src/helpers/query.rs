use std::{collections::HashMap, str::FromStr};

use anyhow::{Context, anyhow};
use database::mungos::{
  find::find_collect,
  mongodb::bson::{Document, doc, oid::ObjectId},
};
use komodo_client::{
  busy::Busy,
  entities::{
    ResourceTarget,
    action::{Action, ActionState},
    alerter::Alerter,
    docker::container::{
      ContainerListItem, ContainerStateStatusEnum,
    },
    permission::{PermissionLevel, PermissionLevelAndSpecifics},
    procedure::{Procedure, ProcedureState},
    repo::Repo,
    server::{Server, ServerState},
    stack::{QuadletKind, QuadletUnit, Stack, StackState},
    sync::ResourceSync,
    tag::Tag,
    user::{User, admin_service_user},
    user_group::UserGroup,
  },
};
use mogh_auth_server::provider::oidc::SubjectIdentifier;

use crate::{
  config::core_config,
  permission::get_user_permission_on_resource,
  resource::{self, KomodoResource},
  state::{
    action_state_cache, action_states, db_client,
    procedure_state_cache, server_status_cache, stack_status_cache,
  },
};

// user: Id or username
pub async fn get_user(user: &str) -> anyhow::Result<User> {
  if let Some(user) = admin_service_user(user) {
    return Ok(user);
  }
  db_client()
    .users
    .find_one(id_or_username_filter(user))
    .await
    .context("Failed to query mongo for user")?
    .with_context(|| format!("No user found matching '{user}'"))
}

pub async fn get_server_with_state(
  server_id_or_name: &str,
) -> anyhow::Result<(Server, ServerState)> {
  let server = resource::get::<Server>(server_id_or_name).await?;
  let state = get_server_state(&server).await;
  Ok((server, state))
}

pub async fn get_server_state(server: &Server) -> ServerState {
  if !server.config.enabled {
    return ServerState::Disabled;
  }
  let Ok(periphery) = super::periphery_client(server).await else {
    return ServerState::NotOk;
  };
  match periphery
    .request(periphery_client::api::GetVersion {})
    .await
  {
    Ok(_) => ServerState::Ok,
    Err(_) => ServerState::NotOk,
  }
}

/// The Server state as computed for the server list items,
/// from the in memory status cache.
pub async fn get_cached_server_state(id: &String) -> ServerState {
  server_status_cache()
    .get(id)
    .await
    .map(|status| status.state)
    .unwrap_or_default()
}

/// The Stack state as computed for the stack list items,
/// from the in memory action states / status cache.
pub async fn get_cached_stack_state(id: &String) -> StackState {
  if action_states()
    .stack
    .get(id)
    .await
    .map(|s| s.get().map(|s| s.deploying))
    .transpose()
    .ok()
    .flatten()
    .unwrap_or_default()
  {
    return StackState::Deploying;
  }
  stack_status_cache()
    .get(id)
    .await
    .map(|status| status.curr.state)
    .unwrap_or_default()
}

/// Pass all the containers from the same server.
/// Runnable units (container / kube / pod) count; a pod counts through its member containers.
pub fn get_stack_state_from_containers(
  ignore_units: &[String],
  units: &[QuadletUnit],
  containers: &[ContainerListItem],
) -> StackState {
  let units = units
    .iter()
    .filter(|unit| {
      unit.kind == QuadletKind::Container
        && !ignore_units.contains(&unit.file)
    })
    .collect::<Vec<_>>();
  if units.is_empty() {
    return StackState::Down;
  }
  let containers = containers
    .iter()
    .filter(|container| {
      units.iter().any(|unit| {
        container.unit.as_deref() == Some(unit.service.as_str())
          || container.name == unit.name
      })
    })
    .collect::<Vec<_>>();
  if containers.is_empty() {
    return StackState::Down;
  }
  if units.len() > containers.len() {
    return StackState::Unhealthy;
  }
  let all = |state: ContainerStateStatusEnum| {
    containers.iter().all(|container| container.state == state)
  };
  if all(ContainerStateStatusEnum::Running) {
    return StackState::Running;
  }
  if all(ContainerStateStatusEnum::Paused) {
    return StackState::Paused;
  }
  if all(ContainerStateStatusEnum::Exited) {
    return StackState::Stopped;
  }
  if all(ContainerStateStatusEnum::Created) {
    return StackState::Created;
  }
  if all(ContainerStateStatusEnum::Restarting) {
    return StackState::Restarting;
  }
  if all(ContainerStateStatusEnum::Dead) {
    return StackState::Dead;
  }
  if all(ContainerStateStatusEnum::Removing) {
    return StackState::Removing;
  }
  StackState::Unhealthy
}

pub async fn get_stack_state(
  stack: &Stack,
) -> anyhow::Result<StackState> {
  if stack.config.server_id.is_empty() {
    return Ok(StackState::Down);
  }
  let state = stack_status_cache()
    .get(&stack.id)
    .await
    .unwrap_or_default()
    .curr
    .state;
  Ok(state)
}

pub async fn get_tag(id_or_name: &str) -> anyhow::Result<Tag> {
  let query = match ObjectId::from_str(id_or_name) {
    Ok(id) => doc! { "_id": id },
    Err(_) => doc! { "name": id_or_name },
  };
  db_client()
    .tags
    .find_one(query)
    .await
    .context("failed to query mongo for tag")?
    .with_context(|| format!("no tag found matching {id_or_name}"))
}

pub async fn get_tag_check_owner(
  id_or_name: &str,
  user: &User,
) -> anyhow::Result<Tag> {
  let tag = get_tag(id_or_name).await?;
  if user.admin || tag.owner == user.id {
    return Ok(tag);
  }
  Err(anyhow!("user must be tag owner or admin"))
}

pub async fn get_all_tags(
  filter: impl Into<Option<Document>>,
) -> anyhow::Result<Vec<Tag>> {
  find_collect(&db_client().tags, filter, None)
    .await
    .context("failed to query db for tags")
}

pub async fn get_id_to_tags(
  filter: impl Into<Option<Document>>,
) -> anyhow::Result<HashMap<String, Tag>> {
  let res = find_collect(&db_client().tags, filter, None)
    .await
    .context("failed to query db for tags")?
    .into_iter()
    .map(|tag| (tag.id.clone(), tag))
    .collect();
  Ok(res)
}

pub async fn get_user_user_groups(
  user_id: &str,
) -> anyhow::Result<Vec<UserGroup>> {
  find_collect(
    &db_client().user_groups,
    doc! {
      "$or": [
        { "everyone": true },
        { "users": user_id },
      ]
    },
    None,
  )
  .await
  .context("failed to query db for user groups")
}

pub async fn get_user_user_group_ids(
  user_id: &str,
) -> anyhow::Result<Vec<String>> {
  let res = get_user_user_groups(user_id)
    .await?
    .into_iter()
    .map(|ug| ug.id)
    .collect();
  Ok(res)
}

pub fn user_target_query(
  user_id: &str,
  user_groups: &[UserGroup],
) -> anyhow::Result<Vec<Document>> {
  let mut user_target_query = vec![
    doc! { "user_target.type": "User", "user_target.id": user_id },
  ];
  let user_groups = user_groups.iter().map(|ug| {
    doc! {
      "user_target.type": "UserGroup", "user_target.id": &ug.id,
    }
  });
  user_target_query.extend(user_groups);
  Ok(user_target_query)
}

pub async fn get_user_permission_on_target(
  user: &User,
  target: &ResourceTarget,
) -> anyhow::Result<PermissionLevelAndSpecifics> {
  match target {
    ResourceTarget::System(_) => Ok(PermissionLevel::None.into()),
    ResourceTarget::Server(id) => {
      get_user_permission_on_resource::<Server>(user, id).await
    }
    ResourceTarget::Stack(id) => {
      get_user_permission_on_resource::<Stack>(user, id).await
    }
    ResourceTarget::Repo(id) => {
      get_user_permission_on_resource::<Repo>(user, id).await
    }
    ResourceTarget::Procedure(id) => {
      get_user_permission_on_resource::<Procedure>(user, id).await
    }
    ResourceTarget::Action(id) => {
      get_user_permission_on_resource::<Action>(user, id).await
    }
    ResourceTarget::ResourceSync(id) => {
      get_user_permission_on_resource::<ResourceSync>(user, id).await
    }
    ResourceTarget::Alerter(id) => {
      get_user_permission_on_resource::<Alerter>(user, id).await
    }
  }
}

pub fn id_or_name_filter(id_or_name: &str) -> Document {
  match ObjectId::from_str(id_or_name) {
    Ok(id) => doc! { "_id": id },
    Err(_) => doc! { "name": id_or_name },
  }
}

pub fn id_or_username_filter(id_or_username: &str) -> Document {
  match ObjectId::from_str(id_or_username) {
    Ok(id) => doc! { "_id": id },
    Err(_) => doc! { "username": id_or_username },
  }
}

/// Get last time procedure / action was run using Update query.
/// Ignored whether run was successful.
pub async fn get_last_run_at<R: KomodoResource>(
  id: &String,
) -> anyhow::Result<Option<i64>> {
  let resource_type = R::resource_type();
  let res = db_client()
    .updates
    .find_one(doc! {
      "target.type": resource_type.as_ref(),
      "target.id": id,
      "operation": format!("Run{resource_type}"),
      "status": "Complete"
    })
    .sort(doc! { "start_ts": -1 })
    .await
    .context("Failed to query updates collection for last run time")?
    .map(|u| u.start_ts);
  Ok(res)
}

pub async fn get_action_state(id: &String) -> ActionState {
  if action_states()
    .action
    .get(id)
    .await
    .map(|s| s.get().map(|s| s.busy()))
    .transpose()
    .ok()
    .flatten()
    .unwrap_or_default()
  {
    return ActionState::Running;
  }
  action_state_cache().get(id).await.unwrap_or_default()
}

pub async fn get_procedure_state(id: &String) -> ProcedureState {
  if action_states()
    .procedure
    .get(id)
    .await
    .map(|s| s.get().map(|s| s.busy()))
    .transpose()
    .ok()
    .flatten()
    .unwrap_or_default()
  {
    return ProcedureState::Running;
  }
  procedure_state_cache().get(id).await.unwrap_or_default()
}

pub async fn find_oidc_user(
  subject: &SubjectIdentifier,
) -> anyhow::Result<Option<User>> {
  let oidc_provider = &core_config().oidc_provider;
  let oidc_user_id = subject.as_str();
  db_client()
    .users
    .find_one(doc! {
      // Find either primary or linked Oidc user
      "$or": [
        // User is primary Oidc user
        {
          "config.data.provider": oidc_provider,
          "config.data.user_id": oidc_user_id
        },
        // User has linked this Oidc login
        {
          "linked_logins.Oidc.data.provider": oidc_provider,
          "linked_logins.Oidc.data.user_id": oidc_user_id
        }
      ]
    })
    .await
    .context("Failed at find user query from database")
}
