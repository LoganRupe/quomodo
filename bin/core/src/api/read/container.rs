use std::cmp;

use anyhow::Context as _;
use database::bson::doc;
use komodo_client::{
  api::read::*,
  entities::{
    ResourceTarget,
    docker::container::{
      ContainerListItem, ContainerSortBy, ContainerStateStatusEnum,
    },
    permission::PermissionLevel,
    server::{Server, ServerQuery, ServerState},
    stack::Stack,
    update::Log,
  },
};
use mogh_resolver::Resolve;
use periphery_client::api as periphery;

use crate::{
  api::read::{ReadArgs, list_limit},
  helpers::{periphery_client, query::get_all_tags},
  permission::{get_check_permissions, list_resources_for_user},
  resource,
  stack::units_of_stack,
  state::server_status_cache,
};

impl Resolve<ReadArgs> for GetContainersSummary {
  async fn resolve(
    self,
    ReadArgs { user }: &ReadArgs,
  ) -> mogh_error::Result<GetContainersSummaryResponse> {
    let servers = resource::list_full_for_user::<Server>(
      Default::default(),
      None,
      None,
      user,
      PermissionLevel::Read.into(),
      &[],
    )
    .await
    .context("failed to get servers from db")?;

    let mut res = GetContainersSummaryResponse::default();

    for server in servers {
      let cache = server_status_cache()
        .get_or_insert_default(&server.id)
        .await;

      if let Some(containers) = &cache.containers {
        for container in containers {
          res.total += 1;
          match container.state {
            ContainerStateStatusEnum::Running => res.running += 1,
            ContainerStateStatusEnum::Exited
            | ContainerStateStatusEnum::Created
            | ContainerStateStatusEnum::Paused => res.stopped += 1,
            ContainerStateStatusEnum::Empty => res.unknown += 1,
            _ => res.unhealthy += 1,
          }
        }
      }
    }

    Ok(res)
  }
}

impl Resolve<ReadArgs> for ListAllContainers {
  async fn resolve(
    self,
    ReadArgs { user }: &ReadArgs,
  ) -> mogh_error::Result<ListAllContainersResponse> {
    let all_tags = if self.tags.is_empty() {
      vec![]
    } else {
      get_all_tags(None).await?
    };
    let servers = resource::list_for_user::<Server>(
      ServerQuery::builder()
        .names(self.servers)
        .tags(self.tags)
        .build(),
      None,
      None,
      user,
      PermissionLevel::Read.into(),
      &all_tags,
    )
    .await?;

    let terms = self
      .terms
      .iter()
      .map(|term| term.to_lowercase())
      .collect::<Vec<_>>();

    let mut containers = Vec::<ContainerListItem>::new();

    for server in servers {
      if server.info.state != ServerState::Ok {
        continue;
      }
      let cache = server_status_cache()
        .get_or_insert_default(&server.id)
        .await;
      let Some(server_containers) = &cache.containers else {
        continue;
      };
      containers.extend(
        server_containers
          .iter()
          .filter(|container| {
            (self.state.is_empty()
              || self.state.contains(&container.state))
              && (terms.is_empty() || {
                let name = container.name.to_lowercase();
                terms.iter().all(|term| name.contains(term))
              })
          })
          .cloned(),
      );
    }

    let cmp_desc = |a: cmp::Ordering| {
      if self.sort_desc { a.reverse() } else { a }
    };
    containers.sort_by(|a, b| {
      let ord = match self.sort_by {
        ContainerSortBy::Name => a.name.cmp(&b.name),
        ContainerSortBy::Server => a.server_name.cmp(&b.server_name),
        ContainerSortBy::State => a.state.cmp(&b.state),
        ContainerSortBy::Image => a.image.cmp(&b.image),
        ContainerSortBy::Networks => {
          a.networks.first().cmp(&b.networks.first())
        }
        ContainerSortBy::Ports => a
          .ports
          .first()
          .map(|p| p.public_port)
          .cmp(&b.ports.first().map(|p| p.public_port)),
        ContainerSortBy::Volumes => {
          a.volumes.first().cmp(&b.volumes.first())
        }
      };
      cmp_desc(ord).then_with(|| a.name.cmp(&b.name))
    });

    let limit = list_limit(self.limit) as usize;
    let skip = (limit as u64).saturating_mul(self.page) as usize;
    let containers = containers
      .into_iter()
      .skip(skip)
      .take(if limit == 0 { usize::MAX } else { limit })
      .collect();

    Ok(containers)
  }
}

impl Resolve<ReadArgs> for ListContainers {
  async fn resolve(
    self,
    ReadArgs { user }: &ReadArgs,
  ) -> mogh_error::Result<ListContainersResponse> {
    let server = get_check_permissions::<Server>(
      &self.server,
      user,
      PermissionLevel::Read.into(),
    )
    .await?;
    let cache =
      server_status_cache().get_or_insert_default(&server.id).await;
    Ok(cache.containers.clone().unwrap_or_default())
  }
}

impl Resolve<ReadArgs> for GetResourceMatchingContainer {
  async fn resolve(
    self,
    ReadArgs { user }: &ReadArgs,
  ) -> mogh_error::Result<GetResourceMatchingContainerResponse> {
    let server = get_check_permissions::<Server>(
      &self.server,
      user,
      PermissionLevel::Read.into(),
    )
    .await?;

    let stacks = list_resources_for_user::<Stack>(
      doc! { "config.server_id": &server.id },
      None,
      None,
      user,
      PermissionLevel::Read.into(),
    )
    .await?;

    // The container name or unit label identifies the stack.
    let unit = server_status_cache()
      .get(&server.id)
      .await
      .and_then(|cache| {
        cache.containers.as_ref().and_then(|containers| {
          containers
            .iter()
            .find(|c| c.name == self.container)
            .and_then(|c| c.unit.clone())
        })
      });

    for stack in stacks {
      let is_match = units_of_stack(&stack).iter().any(|u| {
        u.name == self.container
          || unit.as_deref() == Some(u.service.as_str())
      });
      if is_match {
        return Ok(GetResourceMatchingContainerResponse {
          resource: ResourceTarget::Stack(stack.id).into(),
        });
      }
    }

    Ok(GetResourceMatchingContainerResponse { resource: None })
  }
}

impl Resolve<ReadArgs> for GetContainerLog {
  async fn resolve(
    self,
    ReadArgs { user }: &ReadArgs,
  ) -> mogh_error::Result<Log> {
    let GetContainerLog {
      server,
      container,
      tail,
      timestamps,
    } = self;
    let server = get_check_permissions::<Server>(
      &server,
      user,
      PermissionLevel::Read.logs(),
    )
    .await?;
    let res = periphery_client(&server)
      .await?
      .request(periphery::container::GetContainerLog {
        name: container,
        tail,
        timestamps,
      })
      .await
      .context("failed at periphery")?;
    Ok(res)
  }
}

impl Resolve<ReadArgs> for SearchContainerLog {
  async fn resolve(
    self,
    ReadArgs { user }: &ReadArgs,
  ) -> mogh_error::Result<Log> {
    let SearchContainerLog {
      server,
      container,
      terms,
      combinator,
      invert,
      timestamps,
    } = self;
    let server = get_check_permissions::<Server>(
      &server,
      user,
      PermissionLevel::Read.logs(),
    )
    .await?;
    let res = periphery_client(&server)
      .await?
      .request(periphery::container::GetContainerLogSearch {
        name: container,
        terms,
        combinator,
        invert,
        timestamps,
      })
      .await
      .context("failed at periphery")?;
    Ok(res)
  }
}
