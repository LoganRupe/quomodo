use command::{CommandOptions, run_komodo_standard_command};
use encoding::{EncodedJsonMessage, EncodedResponse};
use komodo_client::entities::{
  config::{GitProvider, ImageRegistry},
  stats::SystemProcess,
  update::Log,
};
use mogh_resolver::Resolve;
use periphery_client::api::{
  container::*, git::*, keys::*, poll::*, quadlet::*, stats::*, *,
};
use serde::{Deserialize, Serialize};
use strum::EnumDiscriminants;
use uuid::Uuid;

use crate::{config::periphery_config, state::stats_client};

mod container;
mod git;
mod keys;
mod poll;
mod quadlet;

#[derive(Debug)]
pub struct Args {
  pub core: String,
  /// The execution id.
  /// Unique for every /execute call.
  pub id: Uuid,
}

#[derive(
  Serialize, Deserialize, Debug, Clone, Resolve, EnumDiscriminants,
)]
#[strum_discriminants(name(PeripheryRequestVariant))]
#[args(Args)]
#[response(EncodedResponse<EncodedJsonMessage>)]
#[error(anyhow::Error)]
#[serde(tag = "type", content = "params")]
#[allow(clippy::enum_variant_names, clippy::large_enum_variant)]
pub enum PeripheryRequest {
  // Stats / Info (Read)
  PollStatus(PollStatus),
  GetHealth(GetHealth),
  GetVersion(GetVersion),
  GetSystemProcesses(GetSystemProcesses),
  GetLatestCommit(GetLatestCommit),

  // Config (Read)
  ListGitProviders(ListGitProviders),
  ListImageRegistries(ListImageRegistries),

  // Repo (Write)
  CloneRepo(CloneRepo),
  PullRepo(PullRepo),
  PullOrCloneRepo(PullOrCloneRepo),
  RenameRepo(RenameRepo),
  DeleteRepo(DeleteRepo),

  // Container (Read)
  ListContainers(ListContainers),
  GetContainerLog(GetContainerLog),
  GetContainerLogSearch(GetContainerLogSearch),

  // Stack (Read)
  ReadStack(ReadStack),
  GetStackLog(GetStackLog),
  GetStackLogSearch(GetStackLogSearch),

  // Stack (Write)
  DeployStack(DeployStack),
  PullStack(PullStack),
  StackUnits(StackUnits),
  DestroyStack(DestroyStack),

  // Podman (Write)
  PruneImages(PruneImages),
  PruneSystem(PruneSystem),

  // Keys
  RotatePrivateKey(RotatePrivateKey),
  RotateCorePublicKey(RotateCorePublicKey),
}

//

impl Resolve<Args> for GetHealth {
  async fn resolve(
    self,
    _: &Args,
  ) -> anyhow::Result<GetHealthResponse> {
    Ok(GetHealthResponse {})
  }
}

//

impl Resolve<Args> for GetVersion {
  async fn resolve(
    self,
    _: &Args,
  ) -> anyhow::Result<GetVersionResponse> {
    Ok(GetVersionResponse {
      version: env!("CARGO_PKG_VERSION").to_string(),
    })
  }
}

//

impl Resolve<Args> for GetSystemProcesses {
  async fn resolve(
    self,
    _: &Args,
  ) -> anyhow::Result<Vec<SystemProcess>> {
    Ok(stats_client().read().await.get_processes())
  }
}

//

impl Resolve<Args> for ListGitProviders {
  async fn resolve(
    self,
    _: &Args,
  ) -> anyhow::Result<Vec<GitProvider>> {
    Ok(periphery_config().git_providers.0.clone())
  }
}

impl Resolve<Args> for ListImageRegistries {
  async fn resolve(
    self,
    _: &Args,
  ) -> anyhow::Result<Vec<ImageRegistry>> {
    Ok(periphery_config().image_registries.0.clone())
  }
}

//

impl Resolve<Args> for PruneImages {
  #[instrument(
    "PruneImages",
    skip_all,
    fields(id = args.id.to_string(), core = args.core)
  )]
  async fn resolve(self, args: &Args) -> anyhow::Result<Log> {
    Ok(
      run_komodo_standard_command(
        "Prune Images",
        "podman image prune -a -f",
        CommandOptions::default(),
      )
      .await,
    )
  }
}

impl Resolve<Args> for PruneSystem {
  #[instrument(
    "PruneSystem",
    skip_all,
    fields(id = args.id.to_string(), core = args.core)
  )]
  async fn resolve(self, args: &Args) -> anyhow::Result<Log> {
    Ok(
      run_komodo_standard_command(
        "Prune System",
        "podman system prune -a -f --volumes",
        CommandOptions::default(),
      )
      .await,
    )
  }
}
