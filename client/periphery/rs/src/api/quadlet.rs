//! Quadlet stack requests.
//!
//! A Stack is a directory of quadlet unit files in a git repo.
//! Deploying copies the directory to `${quadlet_dir}/<stack>/`,
//! reloads systemd, and restarts the units whose files changed.

use std::collections::HashMap;

use komodo_client::entities::{
  FileContents, SearchCombinator,
  repo::Repo,
  stack::{QuadletUnit, Stack},
  update::Log,
};
use mogh_resolver::Resolve;
use serde::{Deserialize, Serialize};

//

/// Deploy the stack.
///
/// Core resolves every `[[NAME]]` referenced in the stack files from the secret
/// store before calling this, into `secrets`.
/// The agent interpolates in memory. Secret values only ever reach the host as
/// podman secrets, never as files under the quadlet directory.
#[derive(Serialize, Deserialize, Debug, Clone, Resolve)]
#[response(DeployStackResponse)]
#[error(anyhow::Error)]
pub struct DeployStack {
  pub stack: Stack,
  pub repo: Option<Repo>,
  pub git_token: Option<String>,
  pub registry_token: Option<String>,
  /// Resolved secrets by name. Only allowed inside `Secret=` lines of unit files,
  /// or inside config files referenced as `Secret=file:<path>`.
  #[serde(default)]
  pub secrets: HashMap<String, String>,
  /// (value, name) pairs used to sanitize logs.
  #[serde(default)]
  pub replacers: Vec<(String, String)>,
  /// Restrict the restart to these unit files (eg `web.container`).
  /// Empty means every changed unit.
  #[serde(default)]
  pub services: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DeployStackResponse {
  pub logs: Vec<Log>,
  /// Whether the stack was successfully deployed
  pub deployed: bool,
  /// The units in the deployed directory
  pub units: Vec<QuadletUnit>,
  /// The files in the stack directory, secret references unresolved.
  pub file_contents: Vec<FileContents>,
  /// Files which could not be read, with the error as contents.
  pub remote_errors: Vec<FileContents>,
  /// Hash over file contents plus the hashes of the resolved secret values.
  pub contents_hash: Option<String>,
  /// The deployed commit hash
  pub commit_hash: Option<String>,
  /// The deployed commit message
  pub commit_message: Option<String>,
}

//

/// `podman pull` the images of the stack's container units.
#[derive(Serialize, Deserialize, Debug, Clone, Resolve)]
#[response(PullStackResponse)]
#[error(anyhow::Error)]
pub struct PullStack {
  pub stack: Stack,
  pub repo: Option<Repo>,
  pub git_token: Option<String>,
  pub registry_token: Option<String>,
  /// Restrict to these unit files. Empty means all container units.
  #[serde(default)]
  pub services: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PullStackResponse {
  pub logs: Vec<Log>,
}

//

#[derive(
  Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq,
)]
pub enum StackUnitAction {
  Start,
  Stop,
  Restart,
}

/// `systemctl <action>` on the stack's runnable units.
#[derive(Serialize, Deserialize, Debug, Clone, Resolve)]
#[response(Log)]
#[error(anyhow::Error)]
pub struct StackUnits {
  /// The stack name
  pub stack: String,
  pub action: StackUnitAction,
  /// Restrict to these unit files. Empty means all runnable units.
  #[serde(default)]
  pub services: Vec<String>,
}

//

/// Stop every unit, remove the stack directory from the quadlet dir,
/// remove the stack's podman secrets and its saved state.
#[derive(Serialize, Deserialize, Debug, Clone, Resolve)]
#[response(Log)]
#[error(anyhow::Error)]
pub struct DestroyStack {
  /// The stack name
  pub stack: String,
}

//

/// Tail the container logs of the given units (or all container units).
#[derive(Serialize, Deserialize, Debug, Clone, Resolve)]
#[response(Log)]
#[error(anyhow::Error)]
pub struct GetStackLog {
  /// The stack name
  pub stack: String,
  /// Unit files, eg `web.container`. Empty means all.
  #[serde(default)]
  pub services: Vec<String>,
  #[serde(default = "default_tail")]
  pub tail: u64,
  #[serde(default)]
  pub timestamps: bool,
}

fn default_tail() -> u64 {
  50
}

//

#[derive(Serialize, Deserialize, Debug, Clone, Resolve)]
#[response(Log)]
#[error(anyhow::Error)]
pub struct GetStackLogSearch {
  /// The stack name
  pub stack: String,
  /// Unit files, eg `web.container`. Empty means all.
  #[serde(default)]
  pub services: Vec<String>,
  pub terms: Vec<String>,
  #[serde(default)]
  pub combinator: SearchCombinator,
  #[serde(default)]
  pub invert: bool,
  #[serde(default)]
  pub timestamps: bool,
}
