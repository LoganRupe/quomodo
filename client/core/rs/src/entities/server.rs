use std::{collections::HashMap, path::PathBuf};

use derive_builder::Builder;
use partial_derive2::Partial;
use serde::{Deserialize, Serialize};
use strum::Display;
use typeshare::typeshare;

use crate::{
  deserializers::{
    option_string_list_deserializer, string_list_deserializer,
  },
  entities::{
    _Serror, MaintenanceWindow, Timelength, stats::MinimalSystemStats,
  },
};

use super::{
  alert::SeverityLevel,
  resource::{AddFilters, Resource, ResourceListItem, ResourceQuery},
};

#[cfg(feature = "utoipa")]
#[derive(utoipa::ToSchema)]
#[schema(as = Server)]
pub struct ServerSchema(
  #[schema(inline)] pub Resource<ServerConfig, ServerInfo>,
);

#[typeshare]
pub type Server = Resource<ServerConfig, ServerInfo>;

#[typeshare]
pub type ServerListItem = ResourceListItem<ServerListItemInfo>;

#[typeshare]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct ServerListItemInfo {
  /// The server's state.
  pub state: ServerState,
  /// If there is an error reaching
  /// the server, message will be given here.
  pub err: Option<_Serror>,
  /// System stats, if available
  pub stats: Option<MinimalSystemStats>,
  /// The server alerting thresholds
  pub alerting_thresholds: ServerAlertingThresholds,
  /// The server's number of physical cores.
  pub core_count: Option<u32>,
  /// The server's number of logical cores.
  pub logical_core_count: Option<u32>,
  /// Region of the server.
  pub region: String,
  /// External address of the server (reachable by users).
  /// Used with links.
  pub external_address: Option<String>,
  /// Host public ip, if it could be resolved.
  pub public_ip: Option<String>,
  /// Whether server is configured to send disconnected alerts.
  pub send_unreachable_alerts: bool,
  /// Whether server is configured to send cpu alerts.
  pub send_cpu_alerts: bool,
  /// Whether server is configured to send mem alerts.
  pub send_mem_alerts: bool,
  /// Whether server is configured to send disk alerts.
  pub send_disk_alerts: bool,
  /// Whether server is configured to send version mismatch alerts.
  pub send_version_mismatch_alerts: bool,
  /// The Komodo Periphery version.
  pub version: Option<String>,
  /// The public key of Periphery
  pub public_key: Option<String>,
  /// The user a rootless agent runs as. None for an agent running as
  /// root, or one not connected.
  pub rootless_user: Option<String>,
  /// If a Periphery fails to authenticate to Core with invalid Periphery public key,
  /// it will be stored here to accept the connection later on.
  pub attempted_public_key: Option<String>,
}

#[typeshare]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct ServerInfo {
  /// If a Periphery fails to authenticate to Core
  /// for a disconnected server with invalid Periphery public key,
  /// it will be stored here to accept the connection later on.
  #[serde(default)]
  pub attempted_public_key: String,
  /// The expected public key associated with
  /// private key of the periphery agent.
  #[serde(default)]
  pub public_key: String,
}

#[typeshare(serialized_as = "Partial<ServerConfig>")]
pub type _PartialServerConfig = PartialServerConfig;

/// Server configuration.
#[typeshare]
#[derive(Serialize, Deserialize, Debug, Clone, Builder, Partial)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[partial_derive(Serialize, Deserialize, Debug, Clone, Default)]
#[cfg_attr(
  feature = "schemars",
  partial_derive(schemars::JsonSchema)
)]
#[diff_derive(Serialize, Deserialize, Debug, Clone, Default)]
#[partial(skip_serializing_none, from, diff)]
pub struct ServerConfig {
  /// The address to use with links for containers on the server.
  #[serde(default)]
  #[builder(default)]
  pub external_address: String,

  /// An optional region label
  #[serde(default)]
  #[builder(default)]
  pub region: String,

  /// Whether a server is enabled.
  /// If a server is disabled,
  /// you won't be able to perform any actions on it or see deployment's status.
  /// Default: false
  #[serde(default = "default_enabled")]
  #[builder(default = "default_enabled()")]
  #[partial_default(default_enabled())]
  pub enabled: bool,

  /// Whether to automatically rotate Server keys when
  /// RotateAllServerKeys is called.
  /// Default: true
  #[serde(default = "default_auto_rotate_keys")]
  #[builder(default = "default_auto_rotate_keys()")]
  #[partial_default(default_auto_rotate_keys())]
  pub auto_rotate_keys: bool,

  /// Sometimes the system stats reports a mount path that is not desired.
  /// Use this field to filter it out from the report.
  #[serde(default, deserialize_with = "string_list_deserializer")]
  #[partial_attr(serde(
    default,
    deserialize_with = "option_string_list_deserializer"
  ))]
  #[builder(default)]
  pub ignore_mounts: Vec<String>,

  /// Whether to trigger 'docker image prune -a -f' every 24 hours.
  /// default: true
  #[serde(default = "default_auto_prune")]
  #[builder(default = "default_auto_prune()")]
  #[partial_default(default_auto_prune())]
  pub auto_prune: bool,

  /// Configure quick links that are displayed in the resource header
  #[serde(default, deserialize_with = "string_list_deserializer")]
  #[partial_attr(serde(
    default,
    deserialize_with = "option_string_list_deserializer"
  ))]
  #[builder(default)]
  pub links: Vec<String>,

  /// Whether to monitor any server stats beyond passing health check.
  /// default: true
  #[serde(default = "default_stats_monitoring")]
  #[builder(default = "default_stats_monitoring()")]
  #[partial_default(default_stats_monitoring())]
  pub stats_monitoring: bool,

  /// Whether to send alerts about the servers reachability
  #[serde(default = "default_send_alerts")]
  #[builder(default = "default_send_alerts()")]
  #[partial_default(default_send_alerts())]
  pub send_unreachable_alerts: bool,

  /// Whether to send alerts about the servers CPU status
  #[serde(default = "default_send_alerts")]
  #[builder(default = "default_send_alerts()")]
  #[partial_default(default_send_alerts())]
  pub send_cpu_alerts: bool,

  /// Whether to send alerts about the servers MEM status
  #[serde(default = "default_send_alerts")]
  #[builder(default = "default_send_alerts()")]
  #[partial_default(default_send_alerts())]
  pub send_mem_alerts: bool,

  /// Whether to send alerts about the servers DISK status
  #[serde(default = "default_send_alerts")]
  #[builder(default = "default_send_alerts()")]
  #[partial_default(default_send_alerts())]
  pub send_disk_alerts: bool,

  /// Whether to send alerts about the servers version mismatch with core
  #[serde(default = "default_send_alerts")]
  #[builder(default = "default_send_alerts()")]
  #[partial_default(default_send_alerts())]
  pub send_version_mismatch_alerts: bool,

  /// The percentage threshhold which triggers WARNING state for CPU.
  #[serde(default = "default_cpu_warning")]
  #[builder(default = "default_cpu_warning()")]
  #[partial_default(default_cpu_warning())]
  pub cpu_warning: f32,

  /// The percentage threshhold which triggers CRITICAL state for CPU.
  #[serde(default = "default_cpu_critical")]
  #[builder(default = "default_cpu_critical()")]
  #[partial_default(default_cpu_critical())]
  pub cpu_critical: f32,

  /// The percentage threshhold which triggers WARNING state for MEM.
  #[serde(default = "default_mem_warning")]
  #[builder(default = "default_mem_warning()")]
  #[partial_default(default_mem_warning())]
  pub mem_warning: f64,

  /// The percentage threshhold which triggers CRITICAL state for MEM.
  #[serde(default = "default_mem_critical")]
  #[builder(default = "default_mem_critical()")]
  #[partial_default(default_mem_critical())]
  pub mem_critical: f64,

  /// The percentage threshhold which triggers WARNING state for DISK.
  #[serde(default = "default_disk_warning")]
  #[builder(default = "default_disk_warning()")]
  #[partial_default(default_disk_warning())]
  pub disk_warning: f64,

  /// The percentage threshhold which triggers CRITICAL state for DISK.
  #[serde(default = "default_disk_critical")]
  #[builder(default = "default_disk_critical()")]
  #[partial_default(default_disk_critical())]
  pub disk_critical: f64,

  /// Scheduled maintenance windows during which alerts will be suppressed.
  #[serde(default)]
  #[builder(default)]
  pub maintenance_windows: Vec<MaintenanceWindow>,
}

impl ServerConfig {
  pub fn builder() -> ServerConfigBuilder {
    ServerConfigBuilder::default()
  }
}

fn default_enabled() -> bool {
  false
}

fn default_auto_rotate_keys() -> bool {
  true
}

fn default_stats_monitoring() -> bool {
  true
}

fn default_auto_prune() -> bool {
  true
}

fn default_send_alerts() -> bool {
  true
}

fn default_cpu_warning() -> f32 {
  90.0
}

fn default_cpu_critical() -> f32 {
  99.0
}

fn default_mem_warning() -> f64 {
  75.0
}

fn default_mem_critical() -> f64 {
  95.0
}

fn default_disk_warning() -> f64 {
  75.0
}

fn default_disk_critical() -> f64 {
  95.0
}

impl Default for ServerConfig {
  fn default() -> Self {
    Self {
      external_address: Default::default(),
      enabled: default_enabled(),
      auto_rotate_keys: default_auto_rotate_keys(),
      ignore_mounts: Default::default(),
      stats_monitoring: default_stats_monitoring(),
      auto_prune: default_auto_prune(),
      links: Default::default(),
      send_unreachable_alerts: default_send_alerts(),
      send_cpu_alerts: default_send_alerts(),
      send_mem_alerts: default_send_alerts(),
      send_disk_alerts: default_send_alerts(),
      send_version_mismatch_alerts: default_send_alerts(),
      region: Default::default(),
      cpu_warning: default_cpu_warning(),
      cpu_critical: default_cpu_critical(),
      mem_warning: default_mem_warning(),
      mem_critical: default_mem_critical(),
      disk_warning: default_disk_warning(),
      disk_critical: default_disk_critical(),
      maintenance_windows: Default::default(),
    }
  }
}

#[cfg(feature = "utoipa")]
impl utoipa::PartialSchema for PartialServerConfig {
  fn schema()
  -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    utoipa::schema!(#[inline] std::collections::HashMap<String, serde_json::Value>).into()
  }
}

#[cfg(feature = "utoipa")]
impl utoipa::ToSchema for PartialServerConfig {}

/// Just the server alerting thresholds
#[typeshare]
#[derive(Serialize, Deserialize, Debug, Clone)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct ServerAlertingThresholds {
  /// The percentage threshhold which triggers WARNING state for CPU.
  #[serde(default = "default_cpu_warning")]
  pub cpu_warning: f32,

  /// The percentage threshhold which triggers CRITICAL state for CPU.
  #[serde(default = "default_cpu_critical")]
  pub cpu_critical: f32,

  /// The percentage threshhold which triggers WARNING state for MEM.
  #[serde(default = "default_mem_warning")]
  pub mem_warning: f64,

  /// The percentage threshhold which triggers CRITICAL state for MEM.
  #[serde(default = "default_mem_critical")]
  pub mem_critical: f64,

  /// The percentage threshhold which triggers WARNING state for DISK.
  #[serde(default = "default_disk_warning")]
  pub disk_warning: f64,

  /// The percentage threshhold which triggers CRITICAL state for DISK.
  #[serde(default = "default_disk_critical")]
  pub disk_critical: f64,
}

impl From<&ServerConfig> for ServerAlertingThresholds {
  fn from(config: &ServerConfig) -> Self {
    ServerAlertingThresholds {
      cpu_warning: config.cpu_warning,
      cpu_critical: config.cpu_critical,
      mem_warning: config.mem_warning,
      mem_critical: config.mem_critical,
      disk_warning: config.disk_warning,
      disk_critical: config.disk_critical,
    }
  }
}

/// The health of a part of the server.
#[typeshare]
#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct ServerHealthState {
  pub level: SeverityLevel,
  /// Whether the health is good enough to close an open alert.
  pub should_close_alert: bool,
}

/// Summary of the health of the server.
#[typeshare]
#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct ServerHealth {
  pub cpu: ServerHealthState,
  pub mem: ServerHealthState,
  #[cfg_attr(feature = "utoipa", schema(value_type = HashMap<String, ServerHealthState>))]
  pub disks: HashMap<PathBuf, ServerHealthState>,
}

/// Info about Periphery configuration
#[typeshare]
#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct PeripheryInformation {
  /// The Periphery version.
  pub version: String,
  /// The public key of Periphery
  pub public_key: String,
  /// The rate the system stats are being polled from the system
  pub stats_polling_rate: Timelength,
  /// Whether Periphery is successfully talking to podman.
  pub podman_connected: bool,
  /// The host public ip, if it can be resolved.
  pub public_ip: Option<String>,
  /// Whether the agent runs as a user other than root, deploying that
  /// user's rootless quadlets.
  #[serde(default)]
  pub rootless: bool,
  /// The user the agent runs as.
  #[serde(default)]
  pub user: String,
}

/// Current pending actions on the server.
#[typeshare]
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct ServerActionState {
  /// Server currently pruning images
  pub pruning_images: bool,
  /// Server currently pruning system
  pub pruning_system: bool,
}

#[typeshare]
#[derive(
  Debug,
  Clone,
  Copy,
  PartialEq,
  Eq,
  Hash,
  PartialOrd,
  Ord,
  Default,
  Display,
  Serialize,
  Deserialize,
)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[strum(serialize_all = "kebab-case")]
pub enum ServerState {
  /// Server health check passing.
  Ok,
  /// Server is unreachable.
  #[default]
  NotOk,
  /// Server is disabled.
  Disabled,
}

/// Server-specific query
#[typeshare]
pub type ServerQuery = ResourceQuery<ServerQuerySpecifics>;

#[typeshare]
#[derive(
  Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize,
)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub enum ServerSortBy {
  /// Sort by name. Default.
  #[default]
  Name,
  /// Sort by region.
  Region,
  /// Sort by periphery version.
  Version,
  /// Sort by state.
  State,
  /// Sort by current cpu usage percentage.
  Cpu,
  /// Sort by current memory usage percentage.
  Memory,
  /// Sort by current disk usage percentage.
  Disk,
  /// Sort by current 1m load average.
  LoadAverage,
  /// Sort by current network usage (ingress + egress).
  Network,
}

#[typeshare]
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct ServerQuerySpecifics {
  /// Query only for Servers matching these states.
  /// If empty, does not filter by state.
  #[serde(default)]
  pub states: Vec<ServerState>,
}

impl AddFilters for ServerQuerySpecifics {}
