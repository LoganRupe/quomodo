use crate::entities::update::Update;
use clap::Parser;
use mogh_resolver::Resolve;
use serde::{Deserialize, Serialize};
use typeshare::typeshare;

use super::{BatchExecutionResponse, KomodoExecuteRequest};

#[cfg(feature = "utoipa")]
#[utoipa::path(
  post,
  path = "/DeployStack",
  description = "Deploys the target stack.",
  request_body(content = DeployStack),
  responses(
    (status = 200, description = "The update", body = Update),
  ),
)]
pub fn deploy_stack() {}

/// Deploys the target stack. `docker compose up`. Response: [Update]
#[typeshare]
#[derive(
  Debug, Clone, PartialEq, Serialize, Deserialize, Resolve, Parser,
)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[empty_traits(KomodoExecuteRequest)]
#[response(Update)]
#[error(mogh_error::Error)]
pub struct DeployStack {
  /// Id or name
  pub stack: String,
  /// Filter to only deploy specific services.
  /// If empty, will deploy all services.
  ///
  /// Note. For Swarm mode Stacks, this field is not supported and will be ignored.
  #[serde(default)]
  pub services: Vec<String>,
}

//

#[cfg(feature = "utoipa")]
#[utoipa::path(
  post,
  path = "/BatchDeployStack",
  description = "Deploys multiple Stacks in parallel that match pattern.",
  request_body(content = BatchDeployStack),
  responses(
    (status = 200, description = "The batch execution response", body = BatchExecutionResponse),
  ),
)]
pub fn batch_deploy_stack() {}

/// Deploys multiple Stacks in parallel that match pattern. Response: [BatchExecutionResponse].
#[typeshare]
#[derive(
  Serialize, Deserialize, Debug, Clone, PartialEq, Resolve, Parser,
)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[empty_traits(KomodoExecuteRequest)]
#[response(BatchExecutionResponse)]
#[error(mogh_error::Error)]
pub struct BatchDeployStack {
  /// Id or name or wildcard pattern or regex.
  /// Supports multiline and comma delineated combinations of the above.
  ///
  /// Example:
  /// ```text
  /// # match all foo-* stacks
  /// foo-*
  /// # add some more
  /// extra-stack-1, extra-stack-2
  /// ```
  pub pattern: String,

  /// Filter matches by tag.
  /// If empty, skips tag filtering.
  #[serde(default)]
  pub tags: Vec<String>,
}

//

#[cfg(feature = "utoipa")]
#[utoipa::path(
  post,
  path = "/DeployStackIfChanged",
  description = "Checks deployed contents vs latest contents and deploys if changed.",
  request_body(content = DeployStackIfChanged),
  responses(
    (status = 200, description = "The update", body = Update),
  ),
)]
pub fn deploy_stack_if_changed() {}

/// Checks deployed contents vs latest contents,
/// and only if any changes found
/// will `docker compose up`. Response: [Update]
#[typeshare]
#[derive(
  Debug, Clone, PartialEq, Serialize, Deserialize, Resolve, Parser,
)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[empty_traits(KomodoExecuteRequest)]
#[response(Update)]
#[error(mogh_error::Error)]
pub struct DeployStackIfChanged {
  /// Id or name
  pub stack: String,
}

//

#[cfg(feature = "utoipa")]
#[utoipa::path(
  post,
  path = "/BatchDeployStackIfChanged",
  description = "Deploys multiple Stacks if changed in parallel that match pattern.",
  request_body(content = BatchDeployStackIfChanged),
  responses(
    (status = 200, description = "The batch execution response", body = BatchExecutionResponse),
  ),
)]
pub fn batch_deploy_stack_if_changed() {}

/// Deploys multiple Stacks if changed in parallel that match pattern. Response: [BatchExecutionResponse].
#[typeshare]
#[derive(
  Serialize, Deserialize, Debug, Clone, PartialEq, Resolve, Parser,
)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[empty_traits(KomodoExecuteRequest)]
#[response(BatchExecutionResponse)]
#[error(mogh_error::Error)]
pub struct BatchDeployStackIfChanged {
  /// Id or name or wildcard pattern or regex.
  /// Supports multiline and comma delineated combinations of the above.
  ///
  /// Example:
  /// ```text
  /// # match all foo-* stacks
  /// foo-*
  /// # add some more
  /// extra-stack-1, extra-stack-2
  /// ```
  pub pattern: String,

  /// Filter matches by tag.
  /// If empty, skips tag filtering.
  #[serde(default)]
  pub tags: Vec<String>,
}

//

#[cfg(feature = "utoipa")]
#[utoipa::path(
  post,
  path = "/PullStack",
  description = "Pulls images for the target stack.",
  request_body(content = PullStack),
  responses(
    (status = 200, description = "The update", body = Update),
  ),
)]
pub fn pull_stack() {}

/// Pulls images for the target stack. `docker compose pull`. Response: [Update]
#[typeshare]
#[derive(
  Debug, Clone, PartialEq, Serialize, Deserialize, Resolve, Parser,
)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[empty_traits(KomodoExecuteRequest)]
#[response(Update)]
#[error(mogh_error::Error)]
pub struct PullStack {
  /// Id or name
  pub stack: String,
  /// Filter to only pull specific services.
  /// If empty, will pull all services.
  #[serde(default)]
  pub services: Vec<String>,
}

//

#[cfg(feature = "utoipa")]
#[utoipa::path(
  post,
  path = "/BatchPullStack",
  description = "Pulls multiple Stacks in parallel that match pattern.",
  request_body(content = BatchPullStack),
  responses(
    (status = 200, description = "The batch execution response", body = BatchExecutionResponse),
  ),
)]
pub fn batch_pull_stack() {}

/// Pulls multiple Stacks in parallel that match pattern. Response: [BatchExecutionResponse].
#[typeshare]
#[derive(
  Serialize, Deserialize, Debug, Clone, PartialEq, Resolve, Parser,
)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[empty_traits(KomodoExecuteRequest)]
#[response(BatchExecutionResponse)]
#[error(mogh_error::Error)]
pub struct BatchPullStack {
  /// Id or name or wildcard pattern or regex.
  /// Supports multiline and comma delineated combinations of the above.
  ///
  /// Example:
  /// ```text
  /// # match all foo-* stacks
  /// foo-*
  /// # add some more
  /// extra-stack-1, extra-stack-2
  /// ```
  pub pattern: String,

  /// Filter matches by tag.
  /// If empty, skips tag filtering.
  #[serde(default)]
  pub tags: Vec<String>,
}

//

#[cfg(feature = "utoipa")]
#[utoipa::path(
  post,
  path = "/StartStack",
  description = "Starts the target stack.",
  request_body(content = StartStack),
  responses(
    (status = 200, description = "The update", body = Update),
  ),
)]
pub fn start_stack() {}

/// Starts the target stack. `docker compose start`. Response: [Update]
#[typeshare]
#[derive(
  Debug, Clone, PartialEq, Serialize, Deserialize, Resolve, Parser,
)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[empty_traits(KomodoExecuteRequest)]
#[response(Update)]
#[error(mogh_error::Error)]
pub struct StartStack {
  /// Id or name
  pub stack: String,
  /// Filter to only start specific services.
  /// If empty, will start all services.
  #[serde(default)]
  pub services: Vec<String>,
}

//

#[cfg(feature = "utoipa")]
#[utoipa::path(
  post,
  path = "/RestartStack",
  description = "Restarts the target stack.",
  request_body(content = RestartStack),
  responses(
    (status = 200, description = "The update", body = Update),
  ),
)]
pub fn restart_stack() {}

/// Restarts the target stack. `docker compose restart`. Response: [Update]
#[typeshare]
#[derive(
  Debug, Clone, PartialEq, Serialize, Deserialize, Resolve, Parser,
)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[empty_traits(KomodoExecuteRequest)]
#[response(Update)]
#[error(mogh_error::Error)]
pub struct RestartStack {
  /// Id or name
  pub stack: String,
  /// Filter to only restart specific services.
  /// If empty, will restart all services.
  #[serde(default)]
  pub services: Vec<String>,
}

//

#[cfg(feature = "utoipa")]
#[utoipa::path(
  post,
  path = "/StopStack",
  description = "Stops the target stack.",
  request_body(content = StopStack),
  responses(
    (status = 200, description = "The update", body = Update),
  ),
)]
pub fn stop_stack() {}

/// Stops the target stack. `docker compose stop`. Response: [Update]
#[typeshare]
#[derive(
  Debug, Clone, PartialEq, Serialize, Deserialize, Resolve, Parser,
)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[empty_traits(KomodoExecuteRequest)]
#[response(Update)]
#[error(mogh_error::Error)]
pub struct StopStack {
  /// Id or name
  pub stack: String,
  /// Filter to only stop specific services.
  /// If empty, will stop all services.
  #[serde(default)]
  pub services: Vec<String>,
}

//

#[cfg(feature = "utoipa")]
#[utoipa::path(
  post,
  path = "/DestroyStack",
  description = "Destroys the target stack.",
  request_body(content = DestroyStack),
  responses(
    (status = 200, description = "The update", body = Update),
  ),
)]
pub fn destroy_stack() {}

/// Destoys the target stack. `docker compose down`. Response: [Update]
#[typeshare]
#[derive(
  Debug, Clone, PartialEq, Serialize, Deserialize, Resolve, Parser,
)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[empty_traits(KomodoExecuteRequest)]
#[response(Update)]
#[error(mogh_error::Error)]
pub struct DestroyStack {
  /// Id or name
  pub stack: String,
}

//

#[cfg(feature = "utoipa")]
#[utoipa::path(
  post,
  path = "/BatchDestroyStack",
  description = "Destroys multiple Stacks in parallel that match pattern.",
  request_body(content = BatchDestroyStack),
  responses(
    (status = 200, description = "The batch execution response", body = BatchExecutionResponse),
  ),
)]
pub fn batch_destroy_stack() {}

/// Destroys multiple Stacks in parallel that match pattern. Response: [BatchExecutionResponse].
#[typeshare]
#[derive(
  Serialize, Deserialize, Debug, Clone, PartialEq, Resolve, Parser,
)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[empty_traits(KomodoExecuteRequest)]
#[response(BatchExecutionResponse)]
#[error(mogh_error::Error)]
pub struct BatchDestroyStack {
  /// Id or name or wildcard pattern or regex.
  /// Supports multiline and comma delineated combinations of the above.
  ///
  /// Example:
  /// ```text
  /// # match all foo-* stacks
  /// foo-*
  /// # add some more
  /// extra-stack-1, extra-stack-2
  /// ```
  pub pattern: String,

  /// Filter matches by tag.
  /// If empty, skips tag filtering.
  #[serde(default)]
  pub tags: Vec<String>,
}
