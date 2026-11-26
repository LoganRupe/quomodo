use komodo_client::entities::{
  alert::SeverityLevel,
  docker::container::ContainerListItem,
  komodo_timestamp,
  repo::Repo,
  server::{
    PeripheryInformation, Server, ServerConfig, ServerHealth,
    ServerHealthState, ServerState,
  },
  stack::{Stack, StackState},
  stats::{
    SingleDiskUsage, StackUsage, SystemInformation, SystemStats,
  },
};
use std::sync::{
  OnceLock,
  atomic::{AtomicU64, Ordering},
};

use mogh_error::Serror;
use tokio::sync::Mutex;

use crate::state::{
  CachedRepoStatus, CachedServerStatus, CachedStackStatus, History,
  action_states, repo_status_cache, server_status_cache,
  stack_status_cache,
};

#[allow(clippy::too_many_arguments)]
pub async fn insert_server_status(
  server: &Server,
  state: ServerState,
  periphery_info: Option<PeripheryInformation>,
  system_info: Option<SystemInformation>,
  system_stats: Option<SystemStats>,
  containers: Option<Vec<ContainerListItem>>,
  stack_usage: Vec<StackUsage>,
  err: impl Into<Option<Serror>>,
) {
  let health =
    system_stats.as_ref().map(|s| get_server_health(server, s));
  let cache = server_status_cache();
  // Refreshes of one server are serialized by
  // `refresh_server_cache_controller`, so this read-then-insert
  // can't race another refresh of the same server.
  let prev_failed_since = cache
    .get(&server.id)
    .await
    .and_then(|prev| prev.containers_failed_since);
  let containers_failed_since = containers_failed_since(
    state,
    containers.is_some(),
    prev_failed_since,
    komodo_timestamp(),
  );
  cache
    .insert(
      server.id.clone(),
      CachedServerStatus {
        id: server.id.clone(),
        state,
        periphery_info,
        system_info,
        system_stats,
        health,
        containers,
        stack_usage,
        containers_failed_since,
        err: err.into(),
      }
      .into(),
    )
    .await;
}

/// Track how long the agent has failed to list containers. Every
/// poll asks for containers, so an Ok poll without them is a failed
/// `podman ps`. An unreachable agent says nothing about podman, so the
/// run carries over it; a disabled server ends it.
pub fn containers_failed_since(
  state: ServerState,
  listed: bool,
  prev: Option<i64>,
  now: i64,
) -> Option<i64> {
  match (state, listed) {
    (ServerState::Ok, true) | (ServerState::Disabled, _) => None,
    (ServerState::Ok, false) => prev.or(Some(now)),
    (ServerState::NotOk, _) => prev,
  }
}

pub async fn insert_stacks_status_unknown(stacks: Vec<Stack>) {
  for stack in stacks {
    record_stack_status(CachedStackStatus {
      id: stack.id,
      server_id: stack.config.server_id,
      state: StackState::Unknown,
      services: Vec::new(),
      usage: None,
      baseline: 0,
    })
    .await;
  }
}

/// The agent could not list containers, so the stack's cached usage
/// is no longer current. Drop it, keeping the state and the alert
/// baseline as they are.
pub async fn clear_stack_usage(id: &String) {
  let _lock = stack_status_write_lock().lock().await;
  let cache = stack_status_cache();
  let Some(entry) = cache.get(id).await else {
    return;
  };
  let has_usage = entry.curr.usage.is_some()
    || entry.curr.services.iter().any(|service| {
      service.usage.is_some()
        || service
          .container
          .as_ref()
          .is_some_and(|c| c.usage.is_some())
    });
  if !has_usage {
    return;
  }
  let mut curr = entry.curr.clone();
  curr.usage = None;
  for service in &mut curr.services {
    service.usage = None;
    if let Some(container) = &mut service.container {
      container.usage = None;
    }
  }
  cache
    .insert(
      id.clone(),
      History {
        curr,
        prev: entry.prev,
      }
      .into(),
    )
    .await;
}

/// Serializes every read-modify-write of the stack status cache. The
/// cache has no compare-and-set of its own, and the monitor loop,
/// forced refreshes and the alert loop all update the same entries.
fn stack_status_write_lock() -> &'static Mutex<()> {
  static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
  LOCK.get_or_init(Default::default)
}

/// Cache a stack state observed on `status.server_id`.
///
/// `prev` is the alert baseline: the state the alert loop last
/// accounted for. A new observation keeps it, so a transition stays
/// pending until [consume_stack_transition] takes it, however many
/// refreshes (forced or failed) land in between. The baseline resets
/// to the observed state when there is nothing to compare against or
/// the change is expected:
/// - the last state came from another server (the stack moved),
/// - the state is Unknown (the server is unreachable, so its own
///   alert covers it, and the next real state is not compared
///   against a stale one),
/// - the stack has an action in progress (deploy, start, stop...),
///   which is the user changing it on purpose.
pub async fn record_stack_status(mut status: CachedStackStatus) {
  let busy = action_states()
    .stack
    .get(&status.id)
    .await
    .and_then(|s| s.busy().ok())
    .unwrap_or_default();
  let _lock = stack_status_write_lock().lock().await;
  let cache = stack_status_cache();
  let prev = match cache.get(&status.id).await {
    Some(last)
      if !busy
        && status.state != StackState::Unknown
        && last.curr.server_id == status.server_id =>
    {
      status.baseline = last.curr.baseline;
      last.prev
    }
    _ => {
      status.baseline = next_baseline();
      Some(status.state)
    }
  };
  cache
    .insert(status.id.clone(), History { curr: status, prev }.into())
    .await;
}

fn next_baseline() -> u64 {
  static BASELINE: AtomicU64 = AtomicU64::new(1);
  BASELINE.fetch_add(1, Ordering::Relaxed)
}

/// The alert loop has handled `seen`: move its baseline up to the
/// state it saw. A refresh in between may have cached a newer state,
/// which stays pending against the new baseline. Skipped if a
/// refresh reset the baseline since `seen` was read.
pub async fn consume_stack_transition(
  seen: &History<CachedStackStatus, StackState>,
) {
  if seen.prev == Some(seen.curr.state) {
    return;
  }
  let _lock = stack_status_write_lock().lock().await;
  let cache = stack_status_cache();
  let Some(now) = cache.get(&seen.curr.id).await else {
    return;
  };
  if now.curr.baseline != seen.curr.baseline {
    return;
  }
  cache
    .insert(
      seen.curr.id.clone(),
      History {
        curr: CachedStackStatus {
          baseline: next_baseline(),
          ..now.curr.clone()
        },
        prev: Some(seen.curr.state),
      }
      .into(),
    )
    .await;
}

pub async fn insert_repos_status_unknown(repos: Vec<Repo>) {
  let status_cache = repo_status_cache();
  for repo in repos {
    status_cache
      .insert(
        repo.id.clone(),
        CachedRepoStatus {
          latest_hash: None,
          latest_message: None,
        }
        .into(),
      )
      .await;
  }
}

const ALERT_PERCENTAGE_THRESHOLD: f32 = 5.0;

fn get_server_health(
  server: &Server,
  SystemStats {
    cpu_perc,
    mem_used_gb,
    mem_total_gb,
    disks,
    ..
  }: &SystemStats,
) -> ServerHealth {
  let ServerConfig {
    cpu_warning,
    cpu_critical,
    mem_warning,
    mem_critical,
    disk_warning,
    disk_critical,
    ..
  } = &server.config;
  let mut health = ServerHealth::default();

  if cpu_perc >= cpu_critical {
    health.cpu.level = SeverityLevel::Critical;
  } else if cpu_perc >= cpu_warning {
    health.cpu.level = SeverityLevel::Warning
  } else if *cpu_perc < cpu_warning - ALERT_PERCENTAGE_THRESHOLD {
    health.cpu.should_close_alert = true
  }

  let mem_perc = 100.0 * mem_used_gb / mem_total_gb;
  if mem_perc >= *mem_critical {
    health.mem.level = SeverityLevel::Critical
  } else if mem_perc >= *mem_warning {
    health.mem.level = SeverityLevel::Warning
  } else if mem_perc
    < mem_warning - (ALERT_PERCENTAGE_THRESHOLD as f64)
  {
    health.mem.should_close_alert = true
  }

  for SingleDiskUsage {
    mount,
    used_gb,
    total_gb,
    ..
  } in disks
  {
    let perc = 100.0 * used_gb / total_gb;
    let mut state = ServerHealthState::default();
    if perc >= *disk_critical {
      state.level = SeverityLevel::Critical;
    } else if perc >= *disk_warning {
      state.level = SeverityLevel::Warning;
    } else if perc
      < disk_warning - (ALERT_PERCENTAGE_THRESHOLD as f64)
    {
      state.should_close_alert = true;
    };
    health.disks.insert(mount.clone(), state);
  }

  health
}
