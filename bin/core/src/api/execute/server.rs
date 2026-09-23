use komodo_client::{
  api::execute::*,
  entities::{
    permission::PermissionLevel, server::Server, update::Update,
    user::User,
  },
};
use mogh_resolver::Resolve;
use periphery_client::api;

use crate::{
  helpers::{periphery_client, update::update_update},
  monitor::refresh_server_cache,
  permission::get_check_permissions,
  state::action_states,
};

use super::ExecuteArgs;

async fn prune<T>(
  server: &str,
  user: &User,
  mut update: Update,
  set_in_progress: impl Fn(&mut komodo_client::entities::server::ServerActionState),
  request: T,
) -> mogh_error::Result<Update>
where
  T: std::fmt::Debug + serde::Serialize + mogh_resolver::HasResponse<Response = komodo_client::entities::update::Log>,
{
  let server = get_check_permissions::<Server>(
    server,
    user,
    PermissionLevel::Execute.into(),
  )
  .await?;

  // get the action state for the server (or insert default).
  let action_state =
    action_states().server.get_or_insert_default(&server.id).await;

  // Will check to ensure server not already busy before updating, and return Err if so.
  // The returned guard will set the action state back to default when dropped.
  let action_guard = action_state.update(set_in_progress)?;

  update_update(update.clone()).await?;

  let periphery = periphery_client(&server).await?;

  let log = periphery.request(request).await?;
  update.logs.push(log);

  refresh_server_cache(&server, true).await;

  update.finalize();

  // Drop action guard before updating
  // clients to requery action state
  drop(action_guard);
  update_update(update.clone()).await?;

  Ok(update)
}

impl Resolve<ExecuteArgs> for PruneImages {
  #[instrument(
    "PruneImages",
    skip_all,
    fields(
      task_id = task_id.to_string(),
      operator = user.id,
      update_id = update.id,
      server = self.server,
    )
  )]
  async fn resolve(
    self,
    ExecuteArgs {
      user,
      update,
      task_id,
    }: &ExecuteArgs,
  ) -> mogh_error::Result<Update> {
    prune(
      &self.server,
      user,
      update.clone(),
      |state| state.pruning_images = true,
      api::PruneImages {},
    )
    .await
  }
}

impl Resolve<ExecuteArgs> for PruneSystem {
  #[instrument(
    "PruneSystem",
    skip_all,
    fields(
      task_id = task_id.to_string(),
      operator = user.id,
      update_id = update.id,
      server = self.server,
    )
  )]
  async fn resolve(
    self,
    ExecuteArgs {
      user,
      update,
      task_id,
    }: &ExecuteArgs,
  ) -> mogh_error::Result<Update> {
    prune(
      &self.server,
      user,
      update.clone(),
      |state| state.pruning_system = true,
      api::PruneSystem {},
    )
    .await
  }
}
