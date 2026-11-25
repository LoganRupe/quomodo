use komodo_client::entities::{
  docker::container::ContainerListItem,
  stack::{
    QuadletKind, Stack, StackService, StackServiceState, StackState,
  },
};

use super::helpers::{
  insert_stacks_status_unknown, record_stack_status,
};
use crate::{
  helpers::query::get_stack_state_from_containers,
  stack::units_of_stack,
  state::{CachedStackStatus, stack_status_cache},
};

/// Match the stack's units against the containers on the server,
/// and cache the resulting services and state.
pub async fn update_server_stack_cache(
  stacks: Vec<Stack>,
  containers: &[ContainerListItem],
) {
  for stack in stacks {
    let units = units_of_stack(&stack);
    let mut services = units
      .iter()
      .filter(|unit| unit.kind.is_runnable())
      .map(|unit| {
        let container = if unit.kind == QuadletKind::Container {
          containers
            .iter()
            .find(|container| {
              container.unit.as_deref() == Some(unit.service.as_str())
                || container.name == unit.name
            })
            .cloned()
        } else {
          None
        };
        let state = match unit.kind {
          QuadletKind::Container => container
            .as_ref()
            .map(|c| c.state.into())
            .unwrap_or(StackServiceState::Down),
          // Pods and kube units are judged by their member containers.
          _ => {
            let members = containers
              .iter()
              .filter(|c| c.pod.as_deref() == Some(unit.name.as_str()))
              .collect::<Vec<_>>();
            if members.is_empty() {
              StackServiceState::Down
            } else if members.iter().all(|c| {
              c.state
                == komodo_client::entities::docker::container::ContainerStateStatusEnum::Running
            }) {
              StackServiceState::Running
            } else {
              StackServiceState::Unhealthy
            }
          }
        };
        StackService {
          stack_id: stack.id.clone(),
          stack_name: stack.name.clone(),
          service: unit.file.clone(),
          unit: unit.clone(),
          container,
          state,
        }
      })
      .collect::<Vec<_>>();

    let current_state = get_stack_state_from_containers(
      &stack.config.ignore_units,
      &units,
      containers,
    );

    services.sort_by(|a, b| a.service.cmp(&b.service));

    record_stack_status(CachedStackStatus {
      id: stack.id,
      server_id: stack.config.server_id,
      state: current_state,
      services,
      baseline: 0,
    })
    .await;
  }
}

/// The agent could not list containers, so nothing new is known.
/// Leave each stack's last state seen on this server as it is, with
/// any transition still pending for the alert loop, so the next
/// listing compares against it. A stack with no state from this
/// server (moved here, or new) is Unknown.
pub async fn keep_server_stack_cache(stacks: Vec<Stack>) {
  let stack_status_cache = stack_status_cache();
  let mut unknown = Vec::new();
  for stack in stacks {
    match stack_status_cache.get(&stack.id).await {
      Some(status)
        if status.curr.server_id == stack.config.server_id => {}
      _ => unknown.push(stack),
    }
  }
  insert_stacks_status_unknown(unknown).await;
}

/// Only for tests / callers that need the plain state without caching.
#[allow(dead_code)]
pub fn stack_state(
  stack: &Stack,
  containers: &[ContainerListItem],
) -> StackState {
  get_stack_state_from_containers(
    &stack.config.ignore_units,
    &units_of_stack(stack),
    containers,
  )
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::{
    monitor::helpers::consume_stack_transition, state::History,
  };

  fn stack(id: &str, server_id: &str) -> Stack {
    let mut stack = Stack {
      id: id.to_string(),
      ..Default::default()
    };
    stack.config.server_id = server_id.to_string();
    stack
  }

  async fn insert(
    id: &str,
    server_id: &str,
    state: StackState,
    prev: Option<StackState>,
  ) {
    stack_status_cache()
      .insert(
        id.to_string(),
        History {
          curr: CachedStackStatus {
            id: id.to_string(),
            server_id: server_id.to_string(),
            state,
            services: Vec::new(),
            baseline: 0,
          },
          prev,
        }
        .into(),
      )
      .await;
  }

  async fn get(id: &str) -> (StackState, Option<StackState>) {
    let status =
      stack_status_cache().get(&id.to_string()).await.unwrap();
    (status.curr.state, status.prev)
  }

  /// What the alert loop does with an entry, minus the alert.
  async fn alert_loop(id: &str) {
    let status =
      stack_status_cache().get(&id.to_string()).await.unwrap();
    consume_stack_transition(&status).await;
  }

  // These stacks have no units, so any listing reads Down.

  #[tokio::test]
  async fn failed_listing_keeps_a_pending_transition() {
    // A good listing saw Running -> Down, not yet alerted.
    insert("blip", "a", StackState::Down, Some(StackState::Running))
      .await;
    keep_server_stack_cache(vec![stack("blip", "a")]).await;
    keep_server_stack_cache(vec![stack("blip", "a")]).await;
    assert_eq!(
      get("blip").await,
      (StackState::Down, Some(StackState::Running))
    );
    // The alert loop takes it once.
    alert_loop("blip").await;
    keep_server_stack_cache(vec![stack("blip", "a")]).await;
    assert_eq!(
      get("blip").await,
      (StackState::Down, Some(StackState::Down))
    );
  }

  #[tokio::test]
  async fn forced_refresh_keeps_a_pending_transition() {
    // The loop's refresh saw Running -> Down.
    insert(
      "forced",
      "a",
      StackState::Down,
      Some(StackState::Running),
    )
    .await;
    // A forced refresh lands before the alert loop reads it.
    update_server_stack_cache(vec![stack("forced", "a")], &[]).await;
    assert_eq!(
      get("forced").await,
      (StackState::Down, Some(StackState::Running))
    );
  }

  #[tokio::test]
  async fn alert_loop_does_not_take_a_newer_transition() {
    insert(
      "newer",
      "a",
      StackState::Stopped,
      Some(StackState::Running),
    )
    .await;
    let seen = stack_status_cache()
      .get(&"newer".to_string())
      .await
      .unwrap();
    // A refresh sees Down after the alert loop read Stopped.
    update_server_stack_cache(vec![stack("newer", "a")], &[]).await;
    consume_stack_transition(&seen).await;
    // Running -> Stopped was alerted, Stopped -> Down is pending.
    assert_eq!(
      get("newer").await,
      (StackState::Down, Some(StackState::Stopped))
    );
  }

  #[tokio::test]
  async fn alert_loop_does_not_move_a_reset_baseline() {
    insert(
      "reset",
      "a",
      StackState::Stopped,
      Some(StackState::Running),
    )
    .await;
    let seen = stack_status_cache()
      .get(&"reset".to_string())
      .await
      .unwrap();
    // The stack moves, and its new server sets a fresh baseline.
    update_server_stack_cache(vec![stack("reset", "b")], &[]).await;
    consume_stack_transition(&seen).await;
    assert_eq!(
      get("reset").await,
      (StackState::Down, Some(StackState::Down))
    );
  }

  #[tokio::test]
  async fn first_listing_on_a_new_server_is_the_baseline() {
    insert(
      "moved",
      "a",
      StackState::Running,
      Some(StackState::Running),
    )
    .await;
    update_server_stack_cache(vec![stack("moved", "b")], &[]).await;
    assert_eq!(
      get("moved").await,
      (StackState::Down, Some(StackState::Down))
    );
  }

  #[tokio::test]
  async fn failed_listing_after_a_server_move_is_unknown() {
    insert(
      "moved2",
      "a",
      StackState::Running,
      Some(StackState::Running),
    )
    .await;
    keep_server_stack_cache(vec![stack("moved2", "b")]).await;
    let status = stack_status_cache()
      .get(&"moved2".to_string())
      .await
      .unwrap();
    assert_eq!(status.curr.state, StackState::Unknown);
    assert_eq!(status.curr.server_id, "b");
    assert!(status.curr.services.is_empty());
  }
}
