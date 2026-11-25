use komodo_client::entities::{
  docker::container::ContainerListItem,
  stack::{
    QuadletKind, Stack, StackService, StackServiceState, StackState,
  },
};

use super::helpers::insert_stacks_status_unknown;
use crate::{
  helpers::query::get_stack_state_from_containers,
  stack::units_of_stack,
  state::{CachedStackStatus, History, stack_status_cache},
};

/// Match the stack's units against the containers on the server,
/// and cache the resulting services and state.
pub async fn update_server_stack_cache(
  stacks: Vec<Stack>,
  containers: &[ContainerListItem],
) {
  let stack_status_cache = stack_status_cache();
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

    let prev_state = stack_status_cache
      .get(&stack.id)
      .await
      .map(|s| s.curr.state);

    stack_status_cache
      .insert(
        stack.id.clone(),
        History {
          curr: CachedStackStatus {
            id: stack.id,
            server_id: stack.config.server_id,
            state: current_state,
            services,
          },
          prev: prev_state,
        }
        .into(),
      )
      .await;
  }
}

/// The agent could not list containers, so nothing new is known.
/// Keep each stack's last state seen on this server with `prev` set
/// to it, so the alert loop sees no transition (and does not resend
/// the last one) and the next listing compares against it. A stack
/// with no state from this server (moved here, or new) is Unknown.
pub async fn keep_server_stack_cache(stacks: Vec<Stack>) {
  let stack_status_cache = stack_status_cache();
  let mut unknown = Vec::new();
  for stack in stacks {
    match stack_status_cache.get(&stack.id).await {
      Some(status)
        if status.curr.server_id == stack.config.server_id =>
      {
        stack_status_cache
          .insert(
            stack.id.clone(),
            History {
              curr: status.curr.clone(),
              prev: Some(status.curr.state),
            }
            .into(),
          )
          .await;
      }
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

  #[tokio::test]
  async fn failed_listing_neither_resends_nor_hides_a_transition() {
    // The last good listing saw Running -> Stopped, which alerted.
    insert(
      "blip",
      "a",
      StackState::Stopped,
      Some(StackState::Running),
    )
    .await;
    // Two failed listings: no transition left for the alert loop.
    keep_server_stack_cache(vec![stack("blip", "a")]).await;
    keep_server_stack_cache(vec![stack("blip", "a")]).await;
    assert_eq!(
      get("blip").await,
      (StackState::Stopped, Some(StackState::Stopped))
    );
    // The next good listing compares against the kept state. This
    // stack has no units, so any listing reads Down.
    update_server_stack_cache(vec![stack("blip", "a")], &[]).await;
    assert_eq!(
      get("blip").await,
      (StackState::Down, Some(StackState::Stopped))
    );
  }

  #[tokio::test]
  async fn failed_listing_after_a_server_move_is_unknown() {
    insert(
      "moved",
      "a",
      StackState::Running,
      Some(StackState::Running),
    )
    .await;
    keep_server_stack_cache(vec![stack("moved", "b")]).await;
    let status = stack_status_cache()
      .get(&"moved".to_string())
      .await
      .unwrap();
    assert_eq!(status.curr.state, StackState::Unknown);
    assert_eq!(status.curr.server_id, "b");
    assert!(status.curr.services.is_empty());
  }
}
