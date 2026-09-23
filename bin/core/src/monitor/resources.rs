use komodo_client::entities::{
  docker::container::ContainerListItem,
  stack::{
    QuadletKind, Stack, StackService, StackServiceState, StackState,
  },
};

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
