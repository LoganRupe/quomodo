use komodo_client::entities::{
  SearchCombinator, docker::container::ContainerListItem, update::Log,
};
use mogh_resolver::Resolve;
use serde::{Deserialize, Serialize};

//

/// `podman ps -a --format json`
#[derive(Serialize, Deserialize, Debug, Clone, Resolve)]
#[response(Vec<ContainerListItem>)]
#[error(anyhow::Error)]
pub struct ListContainers {}

//

#[derive(Serialize, Deserialize, Debug, Clone, Resolve)]
#[response(Log)]
#[error(anyhow::Error)]
pub struct GetContainerLog {
  pub name: String,
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
pub struct GetContainerLogSearch {
  pub name: String,
  pub terms: Vec<String>,
  #[serde(default)]
  pub combinator: SearchCombinator,
  #[serde(default)]
  pub invert: bool,
  #[serde(default)]
  pub timestamps: bool,
}
