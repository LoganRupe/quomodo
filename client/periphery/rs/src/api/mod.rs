use komodo_client::entities::{
  config::{GitProvider, ImageRegistry},
  update::Log,
};
use mogh_resolver::Resolve;
use serde::{Deserialize, Serialize};

pub mod container;
pub mod git;
pub mod keys;
pub mod poll;
pub mod quadlet;
pub mod stats;

//

#[derive(Deserialize, Debug, Clone)]
pub struct CoreConnectionQuery {
  /// Core host (eg demo.komo.do)
  pub core: String,
}

#[derive(Deserialize, Debug, Clone)]
pub struct PeripheryConnectionQuery {
  /// Server Id or name
  pub server: String,
}

//

#[derive(Serialize, Deserialize, Debug, Clone, Resolve)]
#[response(GetHealthResponse)]
#[error(anyhow::Error)]
pub struct GetHealth {}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct GetHealthResponse {}

//

#[derive(Serialize, Deserialize, Debug, Clone, Resolve)]
#[response(GetVersionResponse)]
#[error(anyhow::Error)]
pub struct GetVersion {}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct GetVersionResponse {
  pub version: String,
}

//

#[derive(Serialize, Deserialize, Debug, Clone, Resolve)]
#[response(ListGitProvidersResponse)]
#[error(anyhow::Error)]
pub struct ListGitProviders {}

pub type ListGitProvidersResponse = Vec<GitProvider>;

//

#[derive(Serialize, Deserialize, Debug, Clone, Resolve)]
#[response(ListImageRegistriesResponse)]
#[error(anyhow::Error)]
pub struct ListImageRegistries {}

pub type ListImageRegistriesResponse = Vec<ImageRegistry>;

//

/// `podman image prune -a -f`
#[derive(Serialize, Deserialize, Debug, Clone, Resolve)]
#[response(Log)]
#[error(anyhow::Error)]
pub struct PruneImages {}

//

/// `podman system prune -a -f --volumes`
#[derive(Serialize, Deserialize, Debug, Clone, Resolve)]
#[response(Log)]
#[error(anyhow::Error)]
pub struct PruneSystem {}
