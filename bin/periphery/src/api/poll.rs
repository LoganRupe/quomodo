use komodo_client::entities::server::PeripheryInformation;
use mogh_resolver::Resolve;
use periphery_client::api::poll::{PollStatus, PollStatusResponse};

use crate::{
  config::periphery_config,
  podman,
  state::{host_public_ip, periphery_keys, stats_client},
};

impl Resolve<crate::api::Args> for PollStatus {
  async fn resolve(
    self,
    _: &crate::api::Args,
  ) -> anyhow::Result<PollStatusResponse> {
    let stats_client = stats_client().read().await;

    let system_stats = if self.include_stats {
      Some(stats_client.stats.clone())
    } else {
      None
    };

    let (podman_connected, containers) = if self.include_containers {
      match podman::list_containers().await {
        Ok(containers) => (true, Some(containers)),
        Err(e) => {
          warn!("Failed to list containers | {e:#}");
          (false, None)
        }
      }
    } else {
      (podman::version().await.is_ok(), None)
    };

    Ok(PollStatusResponse {
      periphery_info: periphery_information(podman_connected).await,
      system_info: stats_client.info.clone(),
      system_stats,
      containers,
    })
  }
}

async fn periphery_information(
  podman_connected: bool,
) -> PeripheryInformation {
  let config = periphery_config();
  PeripheryInformation {
    version: env!("CARGO_PKG_VERSION").to_string(),
    public_key: periphery_keys().load().public.to_string(),
    stats_polling_rate: config.stats_polling_rate,
    podman_connected,
    public_ip: host_public_ip().await.cloned(),
  }
}
