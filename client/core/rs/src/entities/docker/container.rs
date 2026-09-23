use serde::{Deserialize, Serialize};
use strum::Display;
use typeshare::typeshare;

use crate::entities::I64;

/// Container summary returned by container list apis.
/// Populated from `podman ps -a --format json`.
#[typeshare]
#[derive(
  Debug, Clone, Default, PartialEq, Serialize, Deserialize,
)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct ContainerListItem {
  /// The Server which hosts the container.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub server_id: Option<String>,
  /// The name of the Server which hosts the container.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub server_name: Option<String>,
  /// The first name in Names
  pub name: String,
  /// The ID of this container
  #[serde(skip_serializing_if = "Option::is_none")]
  pub id: Option<String>,
  /// The name of the image used when creating this container
  #[serde(skip_serializing_if = "Option::is_none")]
  pub image: Option<String>,
  /// The ID of the image that this container was created from
  #[serde(skip_serializing_if = "Option::is_none")]
  pub image_id: Option<String>,
  /// When the container was created (unix seconds)
  #[serde(skip_serializing_if = "Option::is_none")]
  pub created: Option<I64>,
  /// The state of this container (e.g. `exited`)
  pub state: ContainerStateStatusEnum,
  /// Additional human-readable status of this container (e.g. `Exited (0) 2 hours ago`)
  #[serde(skip_serializing_if = "Option::is_none")]
  pub status: Option<String>,
  /// The systemd unit which manages this container,
  /// from the `PODMAN_SYSTEMD_UNIT` label quadlet sets. Eg `web.service`.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub unit: Option<String>,
  /// The pod the container belongs to, if any.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub pod: Option<String>,
  /// The network names attached to container
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  pub networks: Vec<String>,
  /// Port mappings for the container
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  pub ports: Vec<Port>,
  /// The volume / mount sources attached to container
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  pub volumes: Vec<String>,
}

/// An open port on a container
#[typeshare]
#[derive(
  Debug, Clone, Default, PartialEq, Serialize, Deserialize,
)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct Port {
  /// Host IP address that the container's port is mapped to
  pub ip: Option<String>,
  /// Port on the container
  #[serde(default)]
  pub private_port: u16,
  /// Port exposed on the host
  pub public_port: Option<u16>,
  #[serde(default)]
  pub typ: PortTypeEnum,
}

#[typeshare]
#[derive(
  Debug,
  Clone,
  Copy,
  PartialEq,
  Eq,
  PartialOrd,
  Ord,
  Default,
  Serialize,
  Deserialize,
)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub enum PortTypeEnum {
  #[default]
  #[serde(rename = "")]
  EMPTY,
  #[serde(rename = "tcp")]
  TCP,
  #[serde(rename = "udp")]
  UDP,
  #[serde(rename = "sctp")]
  SCTP,
}

#[typeshare]
#[derive(
  Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize,
)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub enum ContainerSortBy {
  /// Sort by container name. Default.
  #[default]
  Name,
  /// Sort by host Server name.
  Server,
  /// Sort by container state.
  State,
  /// Sort by image.
  Image,
  /// Sort by first network.
  Networks,
  /// Sort by first port.
  Ports,
  /// Sort by first volume.
  Volumes,
}

#[typeshare]
#[derive(
  Debug,
  Clone,
  Copy,
  PartialEq,
  Eq,
  PartialOrd,
  Ord,
  Default,
  Serialize,
  Deserialize,
  Display,
)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum ContainerStateStatusEnum {
  Running,
  Created,
  Restarting,
  Stopping,
  Removing,
  Paused,
  Exited,
  Dead,
  #[default]
  #[serde(rename = "")]
  #[strum(serialize = "")]
  Empty,
}

impl ::std::str::FromStr for ContainerStateStatusEnum {
  type Err = anyhow::Error;
  fn from_str(s: &str) -> Result<Self, Self::Err> {
    match s {
      "" | "unknown" => Ok(ContainerStateStatusEnum::Empty),
      "created" | "configured" | "initialized" => {
        Ok(ContainerStateStatusEnum::Created)
      }
      "running" => Ok(ContainerStateStatusEnum::Running),
      "paused" => Ok(ContainerStateStatusEnum::Paused),
      "restarting" => Ok(ContainerStateStatusEnum::Restarting),
      "stopping" => Ok(ContainerStateStatusEnum::Stopping),
      "removing" => Ok(ContainerStateStatusEnum::Removing),
      "exited" | "stopped" => Ok(ContainerStateStatusEnum::Exited),
      "dead" => Ok(ContainerStateStatusEnum::Dead),
      x => Err(anyhow::anyhow!("Invalid container state: {}", x)),
    }
  }
}
