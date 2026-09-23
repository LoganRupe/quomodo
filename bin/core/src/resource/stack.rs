use anyhow::Context;
use database::mungos::mongodb::Collection;
use formatting::format_serror;
use indexmap::IndexSet;
use komodo_client::{
  api::write::RefreshStackCache,
  entities::{
    Operation, ResourceTarget, ResourceTargetVariant,
    permission::{PermissionLevel, SpecificPermission},
    repo::Repo,
    resource::Resource,
    server::Server,
    stack::{
      PartialStackConfig, Stack, StackConfig, StackConfigDiff,
      StackInfo, StackListItem, StackListItemInfo,
      StackQuerySpecifics, StackState,
    },
    to_docker_compatible_name,
    update::Update,
    user::{User, stack_user},
  },
};
use mogh_resolver::Resolve;
use periphery_client::api::quadlet::DestroyStack;

use crate::{
  api::write::WriteArgs,
  config::core_config,
  helpers::{
    periphery_client,
    query::{get_cached_stack_state, get_stack_state},
    repo_link,
  },
  monitor::refresh_server_cache,
  stack::{stack_server, units_of_stack},
  state::{
    action_states, all_resources_cache, db_client, stack_status_cache,
  },
};

use super::get_check_permissions;

impl super::KomodoResource for Stack {
  type Config = StackConfig;
  type PartialConfig = PartialStackConfig;
  type ConfigDiff = StackConfigDiff;
  type Info = StackInfo;
  type ListItem = StackListItem;
  type QuerySpecifics = StackQuerySpecifics;

  fn resource_type() -> ResourceTargetVariant {
    ResourceTargetVariant::Stack
  }

  fn resource_target(id: impl Into<String>) -> ResourceTarget {
    ResourceTarget::Stack(id.into())
  }

  fn validated_name(name: &str) -> String {
    to_docker_compatible_name(name)
  }

  fn creator_specific_permissions() -> IndexSet<SpecificPermission> {
    [SpecificPermission::Inspect, SpecificPermission::Logs]
      .into_iter()
      .collect()
  }

  fn inherit_specific_permissions_from(
    _self: &Resource<Self::Config, Self::Info>,
  ) -> Option<ResourceTarget> {
    if !_self.config.server_id.is_empty() {
      Some(ResourceTarget::Server(_self.config.server_id.clone()))
    } else {
      None
    }
  }

  fn coll() -> &'static Collection<Resource<Self::Config, Self::Info>>
  {
    &db_client().stacks
  }

  async fn to_list_item(
    stack: Resource<Self::Config, Self::Info>,
  ) -> Self::ListItem {
    let state = get_cached_stack_state(&stack.id).await;
    let units = units_of_stack(&stack);

    let default_git = || {
      (
        stack.config.git_provider,
        stack.config.repo,
        stack.config.branch,
        stack.config.git_https,
        String::new(),
      )
    };
    let (git_provider, repo, branch, git_https, linked_repo_name) =
      if stack.config.linked_repo.is_empty() {
        default_git()
      } else {
        all_resources_cache()
          .load()
          .repos
          .get(&stack.config.linked_repo)
          .map(|r| {
            (
              r.config.git_provider.clone(),
              r.config.repo.clone(),
              r.config.branch.clone(),
              r.config.git_https,
              r.name.clone(),
            )
          })
          .unwrap_or_else(default_git)
      };

    let status = match state {
      StackState::Down | StackState::Unknown => None,
      _ => stack_status_cache().get(&stack.id).await.map(|s| {
        let running = s
          .curr
          .services
          .iter()
          .filter(|s| {
            s.state
              == komodo_client::entities::stack::StackServiceState::Running
          })
          .count();
        format!("{running}/{} units running", s.curr.services.len())
      }),
    };

    let server_name = all_resources_cache()
      .load()
      .servers
      .get(&stack.config.server_id)
      .map(|server| server.name.clone())
      .unwrap_or_default();

    let missing_units = stack.info.latest_units.is_empty()
      && stack
        .info
        .remote_errors
        .as_ref()
        .map(|e| !e.is_empty())
        .unwrap_or(false);

    StackListItem {
      name: stack.name,
      id: stack.id,
      template: stack.template,
      tags: stack.tags,
      resource_type: ResourceTargetVariant::Stack,
      info: StackListItemInfo {
        state,
        status,
        units,
        missing_units,
        server_id: stack.config.server_id,
        server_name,
        linked_repo: stack.config.linked_repo,
        linked_repo_name,
        repo_link: repo_link(
          &git_provider,
          &repo,
          &branch,
          git_https,
        ),
        git_provider,
        repo,
        branch,
        latest_hash: stack.info.latest_hash,
        deployed_hash: stack.info.deployed_hash,
      },
    }
  }

  async fn busy(id: &String) -> anyhow::Result<bool> {
    action_states()
      .stack
      .get(id)
      .await
      .unwrap_or_default()
      .busy()
  }

  // CREATE

  fn create_operation() -> Operation {
    Operation::CreateStack
  }

  fn user_can_create(user: &User) -> bool {
    user.admin || !core_config().disable_non_admin_create
  }

  async fn validate_create_config(
    config: &mut Self::PartialConfig,
    user: &User,
  ) -> anyhow::Result<()> {
    validate_config(config, user).await
  }

  async fn post_create(
    created: &Resource<Self::Config, Self::Info>,
    update: &mut Update,
  ) -> anyhow::Result<()> {
    if let Err(e) = (RefreshStackCache {
      stack: created.name.clone(),
    })
    .resolve(&WriteArgs {
      user: stack_user().to_owned(),
    })
    .await
    {
      update.push_error_log(
        "Refresh stack cache",
        format_serror(&e.error.context("The stack cache has failed to refresh. This may be due to a misconfiguration of the Stack").into())
      );
    };
    if created.config.server_id.is_empty() {
      return Ok(());
    }
    if let Ok(server) = stack_server(created).await.inspect_err(|e| {
      warn!(
        "Failed to get Server for Stack {} | {e:#}",
        created.name
      )
    }) {
      refresh_server_cache(&server, true).await;
    }
    Ok(())
  }

  // UPDATE

  fn update_operation() -> Operation {
    Operation::UpdateStack
  }

  async fn validate_update_config(
    _id: &str,
    config: &mut Self::PartialConfig,
    user: &User,
  ) -> anyhow::Result<()> {
    validate_config(config, user).await
  }

  async fn post_update(
    updated: &Resource<Self::Config, Self::Info>,
    update: &mut Update,
  ) -> anyhow::Result<()> {
    Self::post_create(updated, update).await
  }

  // RENAME

  fn rename_operation() -> Operation {
    Operation::RenameStack
  }

  // DELETE

  fn delete_operation() -> Operation {
    Operation::DeleteStack
  }

  async fn pre_delete(
    stack: &Resource<Self::Config, Self::Info>,
    update: &mut Update,
  ) -> anyhow::Result<()> {
    // If it is Up, it should be taken down
    let state = get_stack_state(stack)
      .await
      .context("failed to get stack state")?;
    if matches!(state, StackState::Down | StackState::Unknown) {
      return Ok(());
    }
    let server = match stack_server(stack).await {
      Ok(server) => server,
      Err(e) => {
        update.push_error_log(
          "Destroy Stack",
          format_serror(
            &e.context("Failed to retrieve Server from database.")
              .into(),
          ),
        );
        return Ok(());
      }
    };
    if !server.config.enabled {
      update.push_simple_log(
        "Destroy Stack",
        "Skipping stack destroy, Server is disabled.",
      );
      return Ok(());
    }
    let periphery = match periphery_client(&server).await {
      Ok(periphery) => periphery,
      Err(e) => {
        update.push_error_log(
          "Destroy Stack",
          format_serror(
            &e.context("Failed to get periphery client").into(),
          ),
        );
        return Ok(());
      }
    };
    match periphery
      .request(DestroyStack {
        stack: stack.name.clone(),
      })
      .await
    {
      Ok(log) => update.logs.push(log),
      Err(e) => update.push_simple_log(
        "Failed to destroy stack",
        format_serror(
          &e.context(
            "failed to destroy stack on periphery server before delete",
          )
          .into(),
        ),
      ),
    };
    Ok(())
  }

  async fn post_delete(
    resource: &Resource<Self::Config, Self::Info>,
    _update: &mut Update,
  ) -> anyhow::Result<()> {
    stack_status_cache().remove(&resource.id).await;
    Ok(())
  }
}

#[instrument("ValidateStackConfig", skip_all)]
async fn validate_config(
  config: &mut PartialStackConfig,
  user: &User,
) -> anyhow::Result<()> {
  if let Some(server_id) = &config.server_id
    && !server_id.is_empty()
  {
    let server = get_check_permissions::<Server>(
      server_id,
      user,
      PermissionLevel::Read.attach(),
    )
    .await
    .context("Cannot attach Stack to this Server")?;
    // in case it comes in as name
    config.server_id = Some(server.id);
  }
  if let Some(linked_repo) = &config.linked_repo
    && !linked_repo.is_empty()
  {
    let repo = get_check_permissions::<Repo>(
      linked_repo,
      user,
      PermissionLevel::Read.attach(),
    )
    .await
    .context("Cannot attach Repo to this Stack")?;
    // in case it comes in as name
    config.linked_repo = Some(repo.id);
  }
  Ok(())
}
