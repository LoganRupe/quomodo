use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use typeshare::typeshare;

use crate::entities::{I64, Timelength};

/// System information of a server
#[typeshare]
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct SystemInformation {
  /// The system name
  pub name: Option<String>,
  /// The system long os version
  pub os: Option<String>,
  /// System's kernel version
  pub kernel: Option<String>,
  /// Physical core count
  pub core_count: Option<u32>,
  /// Logical core count.
  pub logical_core_count: Option<u32>,
  /// System hostname based off DNS
  pub host_name: Option<String>,
  /// The CPU's brand
  #[serde(default)]
  pub cpu_brand: String,
  /// CPU architecture (eg. x86_64, aarch64, arm64)
  #[serde(default)]
  pub cpu_arch: String,
}

/// System stats stored on the database.
#[typeshare]
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[cfg_attr(
  feature = "mongo",
  derive(mongo_indexed::derive::MongoIndexed)
)]
#[cfg_attr(feature = "mongo", collection_name(Stats))]
// For GetHistoricalStackStats: a stack's samples across every agent.
#[cfg_attr(feature = "mongo", doc_index({ "stacks.id": 1, "ts": -1 }))]
pub struct SystemStatsRecord {
  /// Unix timestamp in milliseconds
  #[cfg_attr(feature = "mongo", index)]
  pub ts: I64,
  /// Server id
  #[cfg_attr(feature = "mongo", index)]
  pub sid: String,
  // basic stats
  /// Cpu usage percentage
  pub cpu_perc: f32,
  /// Load average (1m, 5m, 15m)
  #[serde(default)]
  pub load_average: SystemLoadAverage,
  /// Memory used in GB
  pub mem_used_gb: f64,
  /// Total memory in GB
  pub mem_total_gb: f64,
  /// [2.3.0+]
  /// Reclaimable page cache + buffers in GB.
  #[serde(default)]
  pub mem_buff_cache_gb: f64,
  /// [2.3.0+]
  /// ZFS ARC cache in GB. 0 when ZFS is not present.
  #[serde(default)]
  pub mem_zfs_arc_gb: f64,
  /// [2.3.0+]
  /// Total swap in GB.
  #[serde(default)]
  pub swap_total_gb: f64,
  /// [2.3.0+]
  /// Used swap in GB.
  #[serde(default)]
  pub swap_used_gb: f64,
  /// Disk used in GB
  pub disk_used_gb: f64,
  /// Total disk size in GB
  pub disk_total_gb: f64,
  /// Breakdown of individual disks, including their usage, total size, and mount point
  pub disks: Vec<SingleDiskUsage>,
  /// Total network ingress in bytes
  #[serde(default)]
  pub network_ingress_bytes: f64,
  /// Total network egress in bytes
  #[serde(default)]
  pub network_egress_bytes: f64,
  // /// Network usage by interface name (ingress, egress in bytes)
  // #[serde(default)]
  // pub network_usage_interface: Vec<SingleNetworkInterfaceUsage>, // interface -> (ingress, egress)
  /// Usage of every running container on this agent, in a stack or
  /// not. None when it could not be measured.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub containers: Option<ContainerUsage>,
  /// Usage of each of this agent's stacks that could be measured.
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  pub stacks: Vec<StackUsage>,
}

/// CPU and memory used by a container, or summed over a pod, stack
/// or agent. Read from the container's cgroup v2 files.
/// A field is None when it could not be measured, never 0.
#[typeshare]
#[derive(
  Serialize, Deserialize, Debug, Default, Clone, Copy, PartialEq,
)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct ContainerUsage {
  /// CPU in cores (1.5 = one and a half cores), averaged over
  /// the agent's last sampling window.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub cpu_cores: Option<f64>,
  /// Working set memory in bytes: `memory.current` less the
  /// reclaimable `inactive_file` page cache.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub mem_bytes: Option<f64>,
  /// The `memory.max` limit in bytes, if one is set.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub mem_limit_bytes: Option<f64>,
}

impl ContainerUsage {
  /// Order two optional readings, a missing one lowest.
  pub fn cmp_reading(
    a: Option<f64>,
    b: Option<f64>,
  ) -> std::cmp::Ordering {
    a.partial_cmp(&b).unwrap_or(std::cmp::Ordering::Equal)
  }

  /// Sum the usage of running containers. A total is None when any
  /// of them lacks that number, so a partial sum never poses as the
  /// whole. Limits are not summed. None for no containers, or when
  /// neither number is known.
  pub fn sum<'a>(
    usages: impl IntoIterator<Item = Option<&'a ContainerUsage>>,
  ) -> Option<ContainerUsage> {
    let mut cpu = Some(0.0);
    let mut mem = Some(0.0);
    let mut any = false;
    for usage in usages {
      any = true;
      let usage = usage.copied().unwrap_or_default();
      cpu = cpu.zip(usage.cpu_cores).map(|(a, b)| a + b);
      mem = mem.zip(usage.mem_bytes).map(|(a, b)| a + b);
    }
    if !any || (cpu.is_none() && mem.is_none()) {
      return None;
    }
    Some(ContainerUsage {
      cpu_cores: cpu,
      mem_bytes: mem,
      mem_limit_bytes: None,
    })
  }
}

/// A stack's usage at one point in time, kept in [SystemStatsRecord].
#[typeshare]
#[derive(
  Serialize, Deserialize, Debug, Default, Clone, PartialEq,
)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct StackUsage {
  /// The stack id
  pub id: String,
  /// CPU in cores
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub cpu_cores: Option<f64>,
  /// Working set memory in bytes
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub mem_bytes: Option<f64>,
}

/// The usage of some stacks at one point in time, collected from the
/// stats records of every agent they ran on.
#[typeshare]
#[derive(Serialize, Deserialize, Debug, Default, Clone)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct StackStatsRecord {
  /// Unix timestamp in milliseconds
  pub ts: I64,
  /// The usage of each requested stack recorded at `ts`.
  pub stacks: Vec<StackUsage>,
}

/// Realtime system stats data.
#[typeshare]
#[derive(Serialize, Deserialize, Debug, Default, Clone)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct SystemStats {
  /// Cpu usage percentage
  pub cpu_perc: f32,
  ///  Load average (1m, 5m, 15m)
  #[serde(default)]
  pub load_average: SystemLoadAverage,
  /// [1.15.9+]
  /// Free memory in GB.
  /// This is really the 'Free' memory, not the 'Available' memory.
  /// It may be different than mem_total_gb - mem_used_gb.
  #[serde(default)]
  pub mem_free_gb: f64,
  /// Used memory in GB. 'Total' - 'Available' (not free) memory,
  /// with the (reclaimable) ZFS ARC cache subtracted out.
  pub mem_used_gb: f64,
  /// Total memory in GB
  pub mem_total_gb: f64,
  /// [2.3.0+]
  /// Reclaimable page cache + buffers in GB.
  #[serde(default)]
  pub mem_buff_cache_gb: f64,
  /// [2.3.0+]
  /// ZFS ARC cache in GB. 0 when ZFS is not present.
  #[serde(default)]
  pub mem_zfs_arc_gb: f64,
  /// [2.3.0+]
  /// Total swap in GB.
  #[serde(default)]
  pub swap_total_gb: f64,
  /// [2.3.0+]
  /// Used swap in GB.
  #[serde(default)]
  pub swap_used_gb: f64,
  /// Breakdown of individual disks, ie their usages, sizes, and mount points
  pub disks: Vec<SingleDiskUsage>,
  /// Network ingress usage in MB
  #[serde(default)]
  pub network_ingress_bytes: f64,
  /// Network egress usage in MB
  #[serde(default)]
  pub network_egress_bytes: f64,
  // /// Network usage by interface name (ingress, egress in bytes)
  // #[serde(default)]
  // pub network_usage_interface: Vec<SingleNetworkInterfaceUsage>, // interface -> (ingress, egress)
  // metadata
  /// The rate the system stats are being polled from the system
  pub polling_rate: Timelength,
  /// Unix timestamp in milliseconds when stats were last polled
  pub refresh_ts: I64,
  /// Unix timestamp in milliseconds when disk list was last refreshed
  pub refresh_list_ts: I64,
}

/// Realtime minimal system stats data (zero allocation)
#[typeshare]
#[derive(Serialize, Deserialize, Debug, Default, Clone, Copy)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct MinimalSystemStats {
  /// Cpu usage percentage
  pub cpu_perc: f32,
  ///  Load average (1m, 5m, 15m)
  pub load_average: SystemLoadAverage,
  /// This is really the 'Free' memory, not the 'Available' memory.
  /// It may be different than mem_total_gb - mem_used_gb.
  pub mem_free_gb: f64,
  /// Used memory in GB. 'Total' - 'Available' (not free) memory,
  /// with the (reclaimable) ZFS ARC cache subtracted out.
  pub mem_used_gb: f64,
  /// Total memory in GB
  pub mem_total_gb: f64,
  /// Reclaimable page cache + buffers in GB.
  pub mem_buff_cache_gb: f64,
  /// ZFS ARC cache in GB. 0 when ZFS is not present.
  pub mem_zfs_arc_gb: f64,
  /// Total swap in GB.
  pub swap_total_gb: f64,
  /// Used swap in GB.
  pub swap_used_gb: f64,
  /// Total size of all disks combined in GB
  pub disk_total_gb: f64,
  /// Used portion of all disks combined in GB
  pub disk_used_gb: f64,
  /// Network ingress usage in MB
  pub network_ingress_bytes: f64,
  /// Network egress usage in MB
  pub network_egress_bytes: f64,
  /// The rate the system stats are being polled from the system
  pub polling_rate: Timelength,
  /// Unix timestamp in milliseconds when stats were last polled
  pub refresh_ts: I64,
  /// Unix timestamp in milliseconds when disk list was last refreshed
  pub refresh_list_ts: I64,
}

impl From<&SystemStats> for MinimalSystemStats {
  fn from(value: &SystemStats) -> Self {
    Self {
      cpu_perc: value.cpu_perc,
      load_average: value.load_average,
      mem_free_gb: value.mem_free_gb,
      mem_used_gb: value.mem_used_gb,
      mem_total_gb: value.mem_total_gb,
      mem_buff_cache_gb: value.mem_buff_cache_gb,
      mem_zfs_arc_gb: value.mem_zfs_arc_gb,
      swap_total_gb: value.swap_total_gb,
      swap_used_gb: value.swap_used_gb,
      disk_total_gb: value
        .disks
        .iter()
        .fold(0.0, |total, disk| total + disk.total_gb),
      disk_used_gb: value
        .disks
        .iter()
        .fold(0.0, |used, disk| used + disk.used_gb),
      network_ingress_bytes: value.network_ingress_bytes,
      network_egress_bytes: value.network_egress_bytes,
      polling_rate: value.polling_rate,
      refresh_ts: value.refresh_ts,
      refresh_list_ts: value.refresh_list_ts,
    }
  }
}

/// Info for a single disk mounted on the system.
#[typeshare]
#[derive(Serialize, Deserialize, Debug, Clone)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct SingleDiskUsage {
  /// The mount point of the disk
  #[cfg_attr(feature = "utoipa", schema(value_type = String))]
  pub mount: PathBuf,
  /// Detected file system
  pub file_system: String,
  /// Used portion of the disk in GB
  pub used_gb: f64,
  /// Total size of the disk in GB
  pub total_gb: f64,
}

/// Info for network interface usage.
#[typeshare]
#[derive(Serialize, Deserialize, Debug, Clone)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct SingleNetworkInterfaceUsage {
  /// The network interface name
  pub name: String,
  /// The ingress in bytes
  pub ingress_bytes: f64,
  /// The egress in bytes
  pub egress_bytes: f64,
}

pub fn sum_disk_usage(disks: &[SingleDiskUsage]) -> TotalDiskUsage {
  disks
    .iter()
    .fold(TotalDiskUsage::default(), |mut total, disk| {
      total.used_gb += disk.used_gb;
      total.total_gb += disk.total_gb;
      total
    })
}

/// Info for the all system disks combined.
#[typeshare]
#[derive(Serialize, Deserialize, Debug, Default, Clone)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct TotalDiskUsage {
  /// Used portion in GB
  pub used_gb: f64,
  /// Total size in GB
  pub total_gb: f64,
}

/// Information about a process on the system.
#[typeshare]
#[derive(Serialize, Deserialize, Debug, Clone)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct SystemProcess {
  /// The process PID
  pub pid: u32,
  /// The process name
  pub name: String,
  /// The path to the process executable
  #[serde(default)]
  pub exe: String,
  /// The command used to start the process
  pub cmd: Vec<String>,
  /// The time the process was started
  #[serde(default)]
  pub start_time: f64,
  /// The cpu usage percentage of the process.
  /// This is in core-percentage, eg 100% is 1 full core, and
  /// an 8 core machine would max at 800%.
  pub cpu_perc: f32,
  /// The memory usage of the process in MB
  pub mem_mb: f64,
  /// Process disk read in KB/s
  pub disk_read_kb: f64,
  /// Process disk write in KB/s
  pub disk_write_kb: f64,
}

#[typeshare]
#[derive(Serialize, Deserialize, Debug, Default, Clone, Copy)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct SystemLoadAverage {
  /// 1m load average
  pub one: f64,
  /// 5m load average
  pub five: f64,
  /// 15m load average
  pub fifteen: f64,
}
