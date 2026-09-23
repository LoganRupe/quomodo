use std::collections::HashMap;

use komodo_client::entities::{
  action::Action, procedure::Procedure, repo::Repo, server::Server,
  stack::Stack, sync::ResourceSync,
};

#[derive(Debug, Default)]
pub struct AllResourcesById {
  pub servers: HashMap<String, Server>,
  pub stacks: HashMap<String, Stack>,
  pub repos: HashMap<String, Repo>,
  pub procedures: HashMap<String, Procedure>,
  pub actions: HashMap<String, Action>,
  pub syncs: HashMap<String, ResourceSync>,
}

impl AllResourcesById {
  /// Use `match_tags` to filter resources by tag.
  pub async fn load() -> anyhow::Result<Self> {
    let map = HashMap::new();
    let id_to_tags = &map;
    let match_tags = &[];
    Ok(Self {
      servers: crate::resource::get_id_to_resource_map::<Server>(
        id_to_tags, match_tags,
      )
      .await?,
      repos: crate::resource::get_id_to_resource_map::<Repo>(
        id_to_tags, match_tags,
      )
      .await?,
      procedures:
        crate::resource::get_id_to_resource_map::<Procedure>(
          id_to_tags, match_tags,
        )
        .await?,
      actions: crate::resource::get_id_to_resource_map::<Action>(
        id_to_tags, match_tags,
      )
      .await?,
      syncs: crate::resource::get_id_to_resource_map::<ResourceSync>(
        id_to_tags, match_tags,
      )
      .await?,
      stacks: crate::resource::get_id_to_resource_map::<Stack>(
        id_to_tags, match_tags,
      )
      .await?,
    })
  }
}
