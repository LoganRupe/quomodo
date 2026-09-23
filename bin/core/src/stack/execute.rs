use komodo_client::entities::{
  permission::PermissionLevel,
  stack::StackActionState,
  update::{Log, Update},
  user::User,
};
use periphery_client::api::quadlet::{StackUnitAction, StackUnits};

use crate::{
  helpers::{periphery_client, update::update_update},
  monitor::refresh_server_cache,
  state::action_states,
};

use super::setup_stack_execution;

/// Start / stop / restart the stack's units with systemctl on the agent.
pub async fn execute_stack_units(
  stack: &str,
  action: StackUnitAction,
  services: Vec<String>,
  user: &User,
  set_in_progress: impl Fn(&mut StackActionState),
  mut update: Update,
) -> anyhow::Result<Update> {
  let (stack, server) = setup_stack_execution(
    stack,
    user,
    PermissionLevel::Execute.into(),
  )
  .await?;

  // get the action state for the stack (or insert default).
  let action_state =
    action_states().stack.get_or_insert_default(&stack.id).await;

  // Will check to ensure stack not already busy before updating, and return Err if so.
  // The returned guard will set the action state back to default when dropped.
  let action_guard = action_state.update(set_in_progress)?;

  // Send update here for UI to recheck action state
  update_update(update.clone()).await?;

  let periphery = periphery_client(&server).await?;

  if !services.is_empty() {
    update.logs.push(Log::simple(
      "Unit/s",
      format!(
        "Execution requested for Stack unit/s {}",
        services.join(", ")
      ),
    ))
  }

  update.logs.push(
    periphery
      .request(StackUnits {
        stack: stack.name.clone(),
        action,
        services,
      })
      .await?,
  );

  // Ensure cached stack state up to date by updating server cache
  refresh_server_cache(&server, true).await;

  update.finalize();

  // Drop action guard before updating
  // clients to requery action state
  drop(action_guard);
  update_update(update.clone()).await?;

  Ok(update)
}
