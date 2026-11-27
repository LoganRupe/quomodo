use clap::Parser;
use mogh_resolver::Resolve;
use serde::{Deserialize, Serialize};
use typeshare::typeshare;

use crate::entities::update::Update;

use super::KomodoExecuteRequest;

//
#[cfg(feature = "utoipa")]
#[utoipa::path(
  post,
  path = "/PruneImages",
  description = "Removes the images on the target server that no container uses and no installed quadlet unit names.",
  request_body(content = PruneImages),
  responses(
    (status = 200, description = "The update", body = Update),
  ),
)]
pub fn prune_images() {}

/// Removes the images on the target server that no container uses and
/// no installed quadlet unit names. Response: [Update].
///
/// Quadlet removes a container when it stops, so the image of a stopped
/// unit, or of a one-shot a timer starts, is kept for its next start.
#[typeshare]
#[derive(
  Serialize, Deserialize, Debug, Clone, PartialEq, Resolve, Parser,
)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[empty_traits(KomodoExecuteRequest)]
#[response(Update)]
#[error(mogh_error::Error)]
pub struct PruneImages {
  /// Id or name
  pub server: String,
}

//

#[cfg(feature = "utoipa")]
#[utoipa::path(
  post,
  path = "/PruneSystem",
  description = "Prunes the podman system on the target server, including volumes. Images installed quadlet units name are kept.",
  request_body(content = PruneSystem),
  responses(
    (status = 200, description = "The update", body = Update),
  ),
)]
pub fn prune_system() {}

/// Prunes the podman system on the target server, including volumes.
/// Response: [Update].
///
/// 1. Runs `podman system prune -f --volumes`.
/// 2. Removes images as [PruneImages] does.
#[typeshare]
#[derive(
  Serialize, Deserialize, Debug, Clone, PartialEq, Resolve, Parser,
)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[empty_traits(KomodoExecuteRequest)]
#[response(Update)]
#[error(mogh_error::Error)]
pub struct PruneSystem {
  /// Id or name
  pub server: String,
}
