//! Per-stack state the agent keeps between deploys, so it can tell which
//! units need a restart. Lives under `${root_directory}/state/<stack>.json`.

use std::{collections::BTreeMap, path::PathBuf};

use anyhow::Context;
use serde::{Deserialize, Serialize};

use crate::config::periphery_config;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StackState {
  /// Relative path -> sha256 of the contents written to the quadlet dir.
  #[serde(default)]
  pub files: BTreeMap<String, String>,
  /// Podman secret name -> sha256 of the secret value.
  #[serde(default)]
  pub secrets: BTreeMap<String, String>,
  /// The unit files installed by the last deploy.
  #[serde(default)]
  pub units: Vec<String>,
}

pub fn state_path(stack: &str) -> PathBuf {
  periphery_config()
    .root_directory
    .join("state")
    .join(format!("{}.json", super::path_name(stack)))
}

pub async fn load(stack: &str) -> anyhow::Result<Option<StackState>> {
  let path = state_path(stack);
  if !path.is_file() {
    return Ok(None);
  }
  let contents = tokio::fs::read_to_string(&path)
    .await
    .with_context(|| format!("Failed to read {path:?}"))?;
  let state = serde_json::from_str(&contents)
    .with_context(|| format!("Failed to parse {path:?}"))?;
  Ok(Some(state))
}

pub async fn save(
  stack: &str,
  state: &StackState,
) -> anyhow::Result<()> {
  let path = state_path(stack);
  if let Some(parent) = path.parent() {
    tokio::fs::create_dir_all(parent)
      .await
      .with_context(|| format!("Failed to create {parent:?}"))?;
  }
  let contents = serde_json::to_string_pretty(state)
    .context("Failed to serialize stack state")?;
  mogh_secret_file::write_async(&path, contents)
    .await
    .with_context(|| format!("Failed to write {path:?}"))
}

pub async fn remove(stack: &str) {
  let _ = tokio::fs::remove_file(state_path(stack)).await;
}
