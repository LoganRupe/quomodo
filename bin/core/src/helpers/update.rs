use anyhow::Context;
use database::mungos::{
  by_id::{find_one_by_id, update_one_by_id},
  mongodb::bson::to_document,
};
use komodo_client::entities::{
  Operation, ResourceTarget,
  action::Action,
  komodo_timestamp,
  procedure::Procedure,
  repo::Repo,
  server::Server,
  stack::Stack,
  sync::ResourceSync,
  update::{Update, UpdateListItem},
  user::User,
};

use crate::{
  api::execute::ExecuteRequest, resource, state::db_client,
};

use super::channel::update_channel;

pub fn make_update(
  target: impl Into<ResourceTarget>,
  operation: Operation,
  user: &User,
) -> Update {
  Update {
    start_ts: komodo_timestamp(),
    target: target.into(),
    operation,
    operator: user.id.clone(),
    success: true,
    ..Default::default()
  }
}

pub async fn add_update(
  mut update: Update,
) -> anyhow::Result<String> {
  update.id = db_client()
    .updates
    .insert_one(&update)
    .await
    .context("failed to insert update into db")?
    .inserted_id
    .as_object_id()
    .context("inserted_id is not object id")?
    .to_string();
  let id = update.id.clone();
  let update = update_list_item(update).await?;
  let _ = send_update(update).await;
  Ok(id)
}

pub async fn add_update_without_send(
  update: &Update,
) -> anyhow::Result<String> {
  let id = db_client()
    .updates
    .insert_one(update)
    .await
    .context("failed to insert update into db")?
    .inserted_id
    .as_object_id()
    .context("inserted_id is not object id")?
    .to_string();
  Ok(id)
}

pub async fn update_update(update: Update) -> anyhow::Result<()> {
  update_one_by_id(&db_client().updates, &update.id, database::mungos::update::Update::Set(to_document(&update)?), None)
    .await
    .context("failed to update the update on db. the update build process was deleted")?;
  let update = update_list_item(update).await?;
  let _ = send_update(update).await;
  Ok(())
}

async fn update_list_item(
  update: Update,
) -> anyhow::Result<UpdateListItem> {
  let username = if User::is_service_user(&update.operator) {
    update.operator.clone()
  } else {
    find_one_by_id(&db_client().users, &update.operator)
      .await
      .context("failed to query mongo for user")?
      .with_context(|| {
        format!("no user found with id {}", update.operator)
      })?
      .username
  };
  let update = UpdateListItem {
    id: update.id,
    operation: update.operation,
    start_ts: update.start_ts,
    success: update.success,
    operator: update.operator,
    target: update.target,
    status: update.status,
    version: update.version,
    other_data: update.other_data,
    username,
  };
  Ok(update)
}

async fn send_update(update: UpdateListItem) -> anyhow::Result<()> {
  update_channel().sender.lock().await.send(update)?;
  Ok(())
}

pub async fn init_execution_update(
  request: &ExecuteRequest,
  user: &User,
) -> anyhow::Result<Update> {
  macro_rules! init_execution_match {
    (
      resource: [$(($Variant:ident, $ResType:ident, $field:ident)),* $(,)?],
      batch: [$($BatchVariant:ident),* $(,)?],
      stack: [$($StackVariant:ident),* $(,)?],
      system: [$($SysVariant:ident),* $(,)?],
    ) => {
      match &request {
        $(
          ExecuteRequest::$Variant(data) => (
            Operation::$Variant,
            ResourceTarget::$ResType(
              resource::get::<$ResType>(&data.$field).await?.id,
            ),
          ),
        )*
        $(
          ExecuteRequest::$BatchVariant(_data) => {
            return Ok(Default::default());
          }
        )*
        $(
          ExecuteRequest::$StackVariant(data) => (
            Operation::$StackVariant,
            ResourceTarget::Stack(
              resource::get::<Stack>(&data.stack).await?.id,
            ),
          ),
        )*
        // DeployStackIfChanged doesn't have a service variant
        ExecuteRequest::DeployStackIfChanged(data) => (
          Operation::DeployStack,
          ResourceTarget::Stack(
            resource::get::<Stack>(&data.stack).await?.id,
          ),
        ),
        $(
          ExecuteRequest::$SysVariant(_data) => {
            (Operation::$SysVariant, ResourceTarget::system())
          }
        )*
      }
    };
  }

  let (operation, target) = init_execution_match!(
    resource: [
      // Server
      (PruneImages, Server, server),
      (PruneSystem, Server, server),
      // Repo
      (CloneRepo, Repo, repo),
      (PullRepo, Repo, repo),
      // Procedure
      (RunProcedure, Procedure, procedure),
      (CancelProcedure, Procedure, procedure),
      // Action
      (RunAction, Action, action),
      (CancelAction, Action, action),
      // Resource Sync
      (RunSync, ResourceSync, sync),
      // Stack (simple)
    ],
    batch: [
      BatchCloneRepo,
      BatchPullRepo,
      BatchRunProcedure,
      BatchRunAction,
      BatchDeployStack,
      BatchDeployStackIfChanged,
      BatchPullStack,
      BatchDestroyStack,
    ],
    stack: [
      DeployStack,
      PullStack,
      StartStack,
      RestartStack,
      StopStack,
      DestroyStack,
    ],
    system: [
      ClearRepoCache,
      BackupCoreDatabase,
      RotateAllServerKeys,
      RotateCoreKeys,
    ],
  );

  let mut update = make_update(target, operation, user);
  update.in_progress();

  // Hold off on even adding update for DeployStackIfChanged
  if !matches!(&request, ExecuteRequest::DeployStackIfChanged(_)) {
    // Don't actually send it here, let the handlers send it after they can set action state.
    update.id = add_update_without_send(&update).await?;
  }

  Ok(update)
}

