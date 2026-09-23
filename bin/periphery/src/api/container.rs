use komodo_client::entities::{
  docker::container::ContainerListItem, update::Log,
};
use mogh_resolver::Resolve;
use periphery_client::api::container::{
  GetContainerLog, GetContainerLogSearch, ListContainers,
};

use crate::podman;

impl Resolve<crate::api::Args> for ListContainers {
  async fn resolve(
    self,
    _: &crate::api::Args,
  ) -> anyhow::Result<Vec<ContainerListItem>> {
    podman::list_containers().await
  }
}

impl Resolve<crate::api::Args> for GetContainerLog {
  async fn resolve(
    self,
    _: &crate::api::Args,
  ) -> anyhow::Result<Log> {
    let GetContainerLog {
      name,
      tail,
      timestamps,
    } = self;
    Ok(podman::container_log(&name, tail, timestamps).await)
  }
}

impl Resolve<crate::api::Args> for GetContainerLogSearch {
  async fn resolve(
    self,
    _: &crate::api::Args,
  ) -> anyhow::Result<Log> {
    let GetContainerLogSearch {
      name,
      terms,
      combinator,
      invert,
      timestamps,
    } = self;
    Ok(
      podman::container_log_search(
        &name, &terms, combinator, invert, timestamps,
      )
      .await,
    )
  }
}
