use std::collections::HashSet;

use komodo_client::entities::{
  docker::container::{ContainerListItem, ContainerStateStatusEnum},
  stack::{
    QuadletKind, Stack, StackService, StackServiceState, StackState,
  },
};

use super::helpers::{
  clear_stack_usage, insert_stacks_status_unknown,
  record_stack_status,
};
use crate::{
  helpers::query::{get_stack_state_from_containers, running_usage},
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
            let members =
              pod_members(containers, &unit.name).collect::<Vec<_>>();
            if members.is_empty() {
              StackServiceState::Down
            } else if members
              .iter()
              .all(|c| c.state == ContainerStateStatusEnum::Running)
            {
              StackServiceState::Running
            } else {
              StackServiceState::Unhealthy
            }
          }
        };
        let usage = match unit.kind {
          QuadletKind::Container => container
            .as_ref()
            .filter(|c| c.state == ContainerStateStatusEnum::Running)
            .and_then(|c| c.usage),
          _ => running_usage(pod_members(containers, &unit.name)),
        };
        StackService {
          stack_id: stack.id.clone(),
          stack_name: stack.name.clone(),
          service: unit.file.clone(),
          unit: unit.clone(),
          container,
          state,
          usage,
        }
      })
      .collect::<Vec<_>>();

    // Each container once: a unit's own, and every pod member
    // (the pod's infra container included).
    let mut stack_containers = HashSet::new();
    for service in &services {
      if let Some(container) = &service.container {
        stack_containers.insert(container_key(container));
      }
      if service.unit.kind != QuadletKind::Container {
        stack_containers.extend(
          pod_members(containers, &service.unit.name)
            .map(container_key),
        );
      }
    }
    let usage = running_usage(
      containers
        .iter()
        .filter(|c| stack_containers.contains(&container_key(c))),
    );

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
      usage,
      baseline: 0,
    })
    .await;
  }
}

fn pod_members<'a>(
  containers: &'a [ContainerListItem],
  pod: &'a str,
) -> impl Iterator<Item = &'a ContainerListItem> {
  containers
    .iter()
    .filter(move |c| c.pod.as_deref() == Some(pod))
}

fn container_key(container: &ContainerListItem) -> &str {
  container.id.as_deref().unwrap_or(container.name.as_str())
}

/// The agent could not list containers, so nothing new is known.
/// Leave each stack's last state seen on this server as it is, with
/// any transition still pending for the alert loop, so the next
/// listing compares against it. Its usage is dropped: kept, it would
/// show old numbers as live. A stack with no state from this
/// server (moved here, or new) is Unknown.
pub async fn keep_server_stack_cache(stacks: Vec<Stack>) {
  let stack_status_cache = stack_status_cache();
  let mut unknown = Vec::new();
  for stack in stacks {
    match stack_status_cache.get(&stack.id).await {
      Some(status)
        if status.curr.server_id == stack.config.server_id =>
      {
        clear_stack_usage(&stack.id).await
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
  use crate::{
    monitor::helpers::consume_stack_transition, state::History,
  };
  use komodo_client::entities::{
    stack::QuadletUnit, stats::ContainerUsage,
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
            usage: None,
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

  fn unit(file: &str, kind: QuadletKind, name: &str) -> QuadletUnit {
    let stem = file.split('.').next().unwrap();
    QuadletUnit {
      file: file.to_string(),
      kind,
      service: if kind == QuadletKind::Pod {
        format!("{stem}-pod.service")
      } else {
        format!("{stem}.service")
      },
      name: name.to_string(),
      ..Default::default()
    }
  }

  fn container(
    id: &str,
    service: &str,
    pod: Option<&str>,
    state: ContainerStateStatusEnum,
    usage: Option<(Option<f64>, Option<f64>)>,
  ) -> ContainerListItem {
    ContainerListItem {
      id: Some(id.to_string()),
      name: format!("systemd-{id}"),
      unit: (!service.is_empty()).then(|| service.to_string()),
      pod: pod.map(str::to_string),
      state,
      usage: usage.map(|(cpu_cores, mem_bytes)| ContainerUsage {
        cpu_cores,
        mem_bytes,
        mem_limit_bytes: None,
      }),
      ..Default::default()
    }
  }

  fn app_stack(id: &str) -> Stack {
    let mut stack = stack(id, "a");
    stack.info.latest_units = vec![
      unit("app.pod", QuadletKind::Pod, "app"),
      unit("web.container", QuadletKind::Container, "systemd-web"),
      unit("db.container", QuadletKind::Container, "systemd-db"),
      unit(
        "cache.container",
        QuadletKind::Container,
        "systemd-cache",
      ),
    ];
    stack
  }

  async fn usage(id: &str) -> CachedStackStatus {
    stack_status_cache()
      .get(&id.to_string())
      .await
      .unwrap()
      .curr
      .clone()
  }

  #[tokio::test]
  async fn pod_and_stack_sum_each_container_once() {
    use ContainerStateStatusEnum::*;
    let containers = [
      container(
        "infra",
        "app-pod.service",
        Some("app"),
        Running,
        Some((Some(0.01), Some(10.0))),
      ),
      container(
        "web",
        "web.service",
        Some("app"),
        Running,
        Some((Some(0.5), Some(100.0))),
      ),
      container(
        "db",
        "db.service",
        Some("app"),
        Running,
        Some((Some(0.25), Some(200.0))),
      ),
      // Stopped: uses nothing, left out rather than blanking.
      container("cache", "cache.service", None, Exited, None),
      // Another stack's container.
      container(
        "other",
        "other.service",
        None,
        Running,
        Some((Some(9.0), Some(9.0))),
      ),
    ];
    update_server_stack_cache(vec![app_stack("sum")], &containers)
      .await;
    let status = usage("sum").await;
    let pod = status
      .services
      .iter()
      .find(|s| s.service == "app.pod")
      .unwrap();
    assert_eq!(pod.state, StackServiceState::Running);
    assert_eq!(pod.usage.unwrap().mem_bytes, Some(310.0));
    let web = status
      .services
      .iter()
      .find(|s| s.service == "web.container")
      .unwrap();
    assert_eq!(web.usage.unwrap().cpu_cores, Some(0.5));
    // web and db are both pod members and units: counted once.
    let total = status.usage.unwrap();
    assert_eq!(total.mem_bytes, Some(310.0));
    assert!((total.cpu_cores.unwrap() - 0.76).abs() < 1e-9);
  }

  #[tokio::test]
  async fn a_total_is_blank_when_a_running_member_has_no_reading() {
    use ContainerStateStatusEnum::*;
    let containers = [
      container(
        "infra",
        "app-pod.service",
        Some("app"),
        Running,
        Some((Some(0.01), Some(10.0))),
      ),
      // First sample after a restart: memory, no CPU yet.
      container(
        "web",
        "web.service",
        Some("app"),
        Running,
        Some((None, Some(100.0))),
      ),
    ];
    update_server_stack_cache(
      vec![app_stack("partial")],
      &containers,
    )
    .await;
    let total = usage("partial").await.usage.unwrap();
    assert_eq!(total.cpu_cores, None);
    assert_eq!(total.mem_bytes, Some(110.0));
  }

  #[tokio::test]
  async fn failed_listing_drops_usage_and_keeps_state() {
    use ContainerStateStatusEnum::*;
    let containers = [container(
      "web",
      "web.service",
      None,
      Running,
      Some((Some(0.5), Some(100.0))),
    )];
    let mut stack = stack("stale", "a");
    stack.info.latest_units = vec![unit(
      "web.container",
      QuadletKind::Container,
      "systemd-web",
    )];
    update_server_stack_cache(vec![stack.clone()], &containers).await;
    assert!(usage("stale").await.usage.is_some());
    keep_server_stack_cache(vec![stack]).await;
    let status = usage("stale").await;
    assert_eq!(status.state, StackState::Running);
    assert!(status.usage.is_none());
    assert!(status.services.iter().all(|s| s.usage.is_none()
      && s.container.as_ref().is_none_or(|c| c.usage.is_none())));
  }
}
