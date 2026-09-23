use komodo_client::entities::{
  action::Action, procedure::Procedure, repo::Repo, server::Server,
  stack::Stack, sync::ResourceSync,
};

use crate::{
  helpers::procedure::replace_procedure_stage_ids_with_names,
  resource::KomodoResource, state::all_resources_cache,
};

/// Trait implementing replacing of linked resource ids with names for Resource Sync TOML.
pub trait ReplaceIds: KomodoResource {
  /// Replace linked ids (server_id, build_id, etc) with the resource name.
  fn replace_ids(_config: &mut Self::Config) {}
}

// These have no linked resource ids to replace
impl ReplaceIds for Server {}
impl ReplaceIds for Action {}

impl ReplaceIds for ResourceSync {
  fn replace_ids(config: &mut Self::Config) {
    let all = all_resources_cache().load();
    config.linked_repo.clone_from(
      all
        .repos
        .get(&config.linked_repo)
        .map(|r| &r.name)
        .unwrap_or(&String::new()),
    );
  }
}

impl ReplaceIds for Stack {
  fn replace_ids(config: &mut Self::Config) {
    let all = all_resources_cache().load();

    config.server_id.clone_from(
      all
        .servers
        .get(&config.server_id)
        .map(|s| &s.name)
        .unwrap_or(&String::new()),
    );

    config.linked_repo.clone_from(
      all
        .repos
        .get(&config.linked_repo)
        .map(|r| &r.name)
        .unwrap_or(&String::new()),
    );
  }
}

impl ReplaceIds for Repo {
  fn replace_ids(config: &mut Self::Config) {
    let all = all_resources_cache().load();
    config.server_id.clone_from(
      all
        .servers
        .get(&config.server_id)
        .map(|s| &s.name)
        .unwrap_or(&String::new()),
    );
  }
}

impl ReplaceIds for Procedure {
  fn replace_ids(config: &mut Self::Config) {
    replace_procedure_stage_ids_with_names(&mut config.stages);
  }
}
