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
  description = "Prunes the docker images on the target server.",
  request_body(content = PruneImages),
  responses(
    (status = 200, description = "The update", body = Update),
  ),
)]
pub fn prune_images() {}

/// Prunes the docker images on the target server. Response: [Update].
///
/// 1. Runs `docker image prune -a -f`.
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
  description = "Prunes the docker system on the target server, including volumes.",
  request_body(content = PruneSystem),
  responses(
    (status = 200, description = "The update", body = Update),
  ),
)]
pub fn prune_system() {}

/// Prunes the docker system on the target server, including volumes. Response: [Update].
///
/// 1. Runs `docker system prune -a -f --volumes`.
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
