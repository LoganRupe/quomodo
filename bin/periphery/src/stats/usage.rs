//! CPU and memory per container, read from its cgroup v2 files.
//!
//! The container's main process (from `podman ps`) names its cgroup in
//! `/proc/<pid>/cgroup`. The process can sit in a leaf below the
//! container's own cgroup (`libpod-<id>.scope/container`), so the
//! path is walked up to the directory named for the container id:
//! `libpod-payload-<id>` under a quadlet service, `libpod-<id>.scope`
//! in a pod. No podman process is started.
//!
//! `cpu.stat` exists in every cgroup v2 group whether or not the cpu
//! controller is delegated, so rootless agents get CPU too. Memory
//! needs the memory controller, which systemd delegates to users by
//! default. Anything unreadable is None, never 0.

use std::{
  collections::{HashMap, HashSet},
  path::{Path, PathBuf},
  sync::{Mutex, OnceLock},
  time::{Duration, Instant},
};

use komodo_client::entities::{
  docker::container::{ContainerListItem, ContainerStateStatusEnum},
  stats::ContainerUsage,
};

const CGROUP_ROOT: &str = "/sys/fs/cgroup";

/// A poll this soon after the last measured one (a forced refresh
/// right after the regular poll) reuses the last CPU figure rather
/// than measuring over a tiny, noisy window.
const MIN_CPU_WINDOW: Duration = Duration::from_secs(5);

struct CpuSample {
  cgroup: PathBuf,
  usage_usec: u64,
  at: Instant,
  cores: Option<f64>,
}

fn cpu_samples() -> &'static Mutex<HashMap<String, CpuSample>> {
  static SAMPLES: OnceLock<Mutex<HashMap<String, CpuSample>>> =
    OnceLock::new();
  SAMPLES.get_or_init(Default::default)
}

/// Set `usage` on each running container. `pid` is its main process
/// on the host, from `podman ps`.
pub fn fill(containers: &mut [(ContainerListItem, u32)]) {
  let now = Instant::now();
  let mut samples = cpu_samples()
    .lock()
    .unwrap_or_else(|poisoned| poisoned.into_inner());
  let mut seen = HashSet::new();
  for (container, pid) in containers.iter_mut() {
    if container.state != ContainerStateStatusEnum::Running
      || *pid == 0
    {
      continue;
    }
    let Some(id) = container.id.clone() else {
      continue;
    };
    let Some(cgroup) =
      std::fs::read_to_string(format!("/proc/{pid}/cgroup"))
        .ok()
        .and_then(|contents| {
          container_cgroup(&contents, Path::new(CGROUP_ROOT), &id)
        })
    else {
      continue;
    };
    seen.insert(id.clone());
    let cpu_cores = read(&cgroup, "cpu.stat")
      .and_then(|stat| stat_value(&stat, "usage_usec"))
      .and_then(|usage_usec| {
        next_cpu(&mut samples, &id, cgroup.clone(), usage_usec, now)
      });
    let mem_bytes = read(&cgroup, "memory.current")
      .and_then(|current| current.trim().parse::<u64>().ok())
      .map(|current| {
        let inactive = read(&cgroup, "memory.stat")
          .and_then(|stat| stat_value(&stat, "inactive_file"))
          .unwrap_or_default();
        current.saturating_sub(inactive) as f64
      });
    let mem_limit_bytes = read(&cgroup, "memory.max")
      .and_then(|max| max.trim().parse::<u64>().ok())
      .map(|max| max as f64);
    container.usage = (cpu_cores.is_some() || mem_bytes.is_some())
      .then_some(ContainerUsage {
        cpu_cores,
        mem_bytes,
        mem_limit_bytes,
      });
  }
  samples.retain(|id, _| seen.contains(id));
}

fn read(cgroup: &Path, file: &str) -> Option<String> {
  std::fs::read_to_string(cgroup.join(file)).ok()
}

/// The container's own cgroup directory: the cgroup v2 path in
/// `/proc/<pid>/cgroup`, walked up to the directory whose name holds
/// the container id. None on cgroup v1, or when no directory names it.
fn container_cgroup(
  proc_cgroup: &str,
  root: &Path,
  container_id: &str,
) -> Option<PathBuf> {
  let path = proc_cgroup
    .lines()
    .find_map(|line| line.strip_prefix("0::"))?
    .trim()
    .trim_start_matches('/');
  let mut dir = root.join(path);
  while dir != root {
    if dir
      .file_name()
      .and_then(|name| name.to_str())
      .is_some_and(|name| name.contains(container_id))
    {
      return Some(dir);
    }
    dir = dir.parent()?.to_path_buf();
  }
  None
}

/// The value of `key` in a flat keyed file like `cpu.stat`.
fn stat_value(contents: &str, key: &str) -> Option<u64> {
  contents.lines().find_map(|line| {
    let (k, v) = line.split_once(' ')?;
    (k == key).then(|| v.trim().parse().ok()).flatten()
  })
}

/// Cores used since the last sample. The first sample, a new cgroup
/// (the container restarted) or a counter that went backwards starts
/// over and gives None.
fn next_cpu(
  samples: &mut HashMap<String, CpuSample>,
  id: &str,
  cgroup: PathBuf,
  usage_usec: u64,
  now: Instant,
) -> Option<f64> {
  let fresh = |cores| CpuSample {
    cgroup: cgroup.clone(),
    usage_usec,
    at: now,
    cores,
  };
  let Some(prev) = samples.get(id) else {
    samples.insert(id.to_string(), fresh(None));
    return None;
  };
  if prev.cgroup != cgroup || usage_usec < prev.usage_usec {
    samples.insert(id.to_string(), fresh(None));
    return None;
  }
  let elapsed = now.saturating_duration_since(prev.at);
  if elapsed < MIN_CPU_WINDOW {
    return prev.cores;
  }
  let cores = (usage_usec - prev.usage_usec) as f64
    / 1_000_000.0
    / elapsed.as_secs_f64();
  samples.insert(id.to_string(), fresh(Some(cores)));
  Some(cores)
}

#[cfg(test)]
mod tests {
  use super::*;

  const ID: &str = "57dc0d06a1c359cafb77043ecdb6c8889fb0ef94dedad0d38030f8a82c3548c5";

  #[test]
  fn finds_the_quadlet_payload_cgroup() {
    let proc =
      format!("0::/system.slice/m66.service/libpod-payload-{ID}\n");
    assert_eq!(
      container_cgroup(&proc, Path::new("/sys/fs/cgroup"), ID),
      Some(PathBuf::from(format!(
        "/sys/fs/cgroup/system.slice/m66.service/libpod-payload-{ID}"
      )))
    );
  }

  #[test]
  fn walks_up_from_a_leaf_below_the_container() {
    // A pod's container, as rootless podman lays it out.
    let pod = "user-libpod_pod_6184aca6b9fe04c9dd9d9d7e25ece97e17b7d7ac4dc46c5fb58cd8d04dbea3f9.slice";
    let proc = format!(
      "0::/user.slice/user-1000.slice/user@1000.service/user.slice/{pod}/libpod-{ID}.scope/container\n"
    );
    assert_eq!(
      container_cgroup(&proc, Path::new("/sys/fs/cgroup"), ID),
      Some(PathBuf::from(format!(
        "/sys/fs/cgroup/user.slice/user-1000.slice/user@1000.service/user.slice/{pod}/libpod-{ID}.scope"
      )))
    );
  }

  #[test]
  fn no_cgroup_without_the_container_id_or_on_v1() {
    let root = Path::new("/sys/fs/cgroup");
    assert_eq!(
      container_cgroup("0::/system.slice/other.service\n", root, ID),
      None
    );
    assert_eq!(
      container_cgroup("4:memory:/docker/abc\n", root, ID),
      None
    );
  }

  #[test]
  fn reads_flat_keyed_values() {
    let stat = "usage_usec 15272498\nuser_usec 100\nsystem_usec 5\n";
    assert_eq!(stat_value(stat, "usage_usec"), Some(15272498));
    assert_eq!(stat_value(stat, "user_usec"), Some(100));
    assert_eq!(stat_value(stat, "nr_periods"), None);
  }

  #[test]
  fn cpu_is_measured_over_the_window() {
    let mut samples = HashMap::new();
    let cg = PathBuf::from("/cg");
    let t0 = Instant::now();
    assert_eq!(next_cpu(&mut samples, "c", cg.clone(), 0, t0), None);
    // 1.5 cores for 10 seconds.
    let t1 = t0 + Duration::from_secs(10);
    assert_eq!(
      next_cpu(&mut samples, "c", cg.clone(), 15_000_000, t1),
      Some(1.5)
    );
    // A forced poll one second later keeps the figure and baseline.
    let t2 = t1 + Duration::from_secs(1);
    assert_eq!(
      next_cpu(&mut samples, "c", cg.clone(), 99_000_000, t2),
      Some(1.5)
    );
    // The next window is measured from t1, not t2.
    let t3 = t1 + Duration::from_secs(10);
    assert_eq!(
      next_cpu(&mut samples, "c", cg.clone(), 20_000_000, t3),
      Some(0.5)
    );
  }

  #[test]
  fn cpu_starts_over_after_a_restart() {
    let mut samples = HashMap::new();
    let t0 = Instant::now();
    let t1 = t0 + Duration::from_secs(10);
    next_cpu(&mut samples, "c", PathBuf::from("/a"), 500, t0);
    // A new cgroup: the container was recreated.
    assert_eq!(
      next_cpu(&mut samples, "c", PathBuf::from("/b"), 900, t1),
      None
    );
    // The counter went backwards.
    let t2 = t1 + Duration::from_secs(10);
    assert_eq!(
      next_cpu(&mut samples, "c", PathBuf::from("/b"), 100, t2),
      None
    );
  }
}
