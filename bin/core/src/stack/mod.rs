use anyhow::{Context, anyhow};
use komodo_client::entities::{
  permission::PermissionLevelAndSpecifics,
  server::{Server, ServerState},
  stack::{QuadletUnit, Stack},
  user::User,
};

use crate::{
  helpers::query::get_server_with_state,
  permission::get_check_permissions,
};

pub mod execute;
pub mod remote;

/// Get the Stack and its Server, checking permissions
/// and that the Server is reachable.
pub async fn setup_stack_execution(
  stack: &str,
  user: &User,
  permissions: PermissionLevelAndSpecifics,
) -> anyhow::Result<(Stack, Server)> {
  let stack =
    get_check_permissions::<Stack>(stack, user, permissions).await?;
  let server = stack_server(&stack).await?;
  Ok((stack, server))
}

/// The Stack's Server, checked reachable.
pub async fn stack_server(stack: &Stack) -> anyhow::Result<Server> {
  if stack.config.server_id.is_empty() {
    return Err(anyhow!("Stack has no Server attached"));
  }
  let (server, state) =
    get_server_with_state(&stack.config.server_id)
      .await
      .context("Failed to get the Stack's Server")?;
  if state != ServerState::Ok {
    return Err(anyhow!(
      "Cannot send command when Server is unreachable or disabled"
    ));
  }
  Ok(server)
}

/// The deployed units if the stack has been deployed, otherwise the latest.
pub fn units_of_stack(stack: &Stack) -> Vec<QuadletUnit> {
  stack
    .info
    .deployed_units
    .clone()
    .unwrap_or_else(|| stack.info.latest_units.clone())
}
