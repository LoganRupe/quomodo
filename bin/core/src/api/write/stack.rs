use anyhow::Context;
use database::mungos::mongodb::bson::{doc, to_document};
use komodo_client::{
  api::write::*,
  entities::{
    NoData,
    permission::PermissionLevel,
    repo::Repo,
    stack::{Stack, StackInfo},
    update::Update,
  },
};
use mogh_resolver::Resolve;

use crate::{
  permission::get_check_permissions,
  resource,
  stack::remote::{RemoteStackContents, get_repo_stack_contents},
  state::db_client,
};

use super::WriteArgs;

impl Resolve<WriteArgs> for CreateStack {
  #[instrument(
    "CreateStack",
    skip_all,
    fields(
      operator = user.id,
      stack = self.name,
      config = serde_json::to_string(&self.config).unwrap(),
    )
  )]
  async fn resolve(
    self,
    WriteArgs { user }: &WriteArgs,
  ) -> mogh_error::Result<Stack> {
    resource::create::<Stack>(&self.name, self.config, None, user)
      .await
  }
}

impl Resolve<WriteArgs> for CopyStack {
  #[instrument(
    "CopyStack",
    skip_all,
    fields(
      operator = user.id,
      stack = self.name,
      copy_stack = self.id,
    )
  )]
  async fn resolve(
    self,
    WriteArgs { user }: &WriteArgs,
  ) -> mogh_error::Result<Stack> {
    let Stack { config, .. } = get_check_permissions::<Stack>(
      &self.id,
      user,
      PermissionLevel::Read.into(),
    )
    .await?;

    resource::create::<Stack>(&self.name, config.into(), None, user)
      .await
  }
}

impl Resolve<WriteArgs> for DeleteStack {
  #[instrument(
    "DeleteStack",
    skip_all,
    fields(
      operator = user.id,
      stack = self.id,
    )
  )]
  async fn resolve(
    self,
    WriteArgs { user }: &WriteArgs,
  ) -> mogh_error::Result<Stack> {
    Ok(resource::delete::<Stack>(&self.id, user).await?)
  }
}

impl Resolve<WriteArgs> for UpdateStack {
  #[instrument(
    "UpdateStack",
    skip_all,
    fields(
      operator = user.id,
      stack = self.id,
      update = serde_json::to_string(&self.config).unwrap(),
    )
  )]
  async fn resolve(
    self,
    WriteArgs { user }: &WriteArgs,
  ) -> mogh_error::Result<Stack> {
    Ok(resource::update::<Stack>(&self.id, self.config, user).await?)
  }
}

impl Resolve<WriteArgs> for RenameStack {
  #[instrument(
    "RenameStack",
    skip_all,
    fields(
      operator = user.id,
      stack = self.id,
      new_name = self.name
    )
  )]
  async fn resolve(
    self,
    WriteArgs { user }: &WriteArgs,
  ) -> mogh_error::Result<Update> {
    Ok(resource::rename::<Stack>(&self.id, &self.name, user).await?)
  }
}

impl Resolve<WriteArgs> for RefreshStackCache {
  async fn resolve(
    self,
    WriteArgs { user }: &WriteArgs,
  ) -> mogh_error::Result<NoData> {
    // Even though this is a write request, this doesn't change any config. Anyone that can execute the
    // stack should be able to do this.
    let stack = get_check_permissions::<Stack>(
      &self.stack,
      user,
      PermissionLevel::Execute.into(),
    )
    .await?;

    refresh_stack_cache(&stack).await?;

    Ok(NoData {})
  }
}

/// Pull the stack repo into Core's cache, read the stack directory,
/// and store the latest files / units / commit on the Stack.
pub async fn refresh_stack_cache(
  stack: &Stack,
) -> anyhow::Result<RemoteStackContents> {
  let repo = if !stack.config.linked_repo.is_empty() {
    resource::get::<Repo>(&stack.config.linked_repo)
      .await?
      .into()
  } else {
    None
  };

  if stack.config.repo.is_empty() && repo.is_none() {
    // Nothing to read without a repo
    return Ok(RemoteStackContents::default());
  }

  let contents =
    get_repo_stack_contents(stack, repo.as_ref()).await?;

  let info = StackInfo {
    deployed_units: stack.info.deployed_units.clone(),
    deployed_contents: stack.info.deployed_contents.clone(),
    deployed_contents_hash: stack.info.deployed_contents_hash.clone(),
    deployed_hash: stack.info.deployed_hash.clone(),
    deployed_message: stack.info.deployed_message.clone(),
    latest_units: contents.units.clone(),
    remote_contents: Some(contents.files.clone()),
    remote_errors: Some(contents.errors.clone()),
    latest_hash: contents.hash.clone(),
    latest_message: contents.message.clone(),
  };

  let info = to_document(&info)
    .context("failed to serialize stack info to bson")?;

  db_client()
    .stacks
    .update_one(
      doc! { "name": &stack.name },
      doc! { "$set": { "info": info } },
    )
    .await
    .context("failed to update stack info on db")?;

  Ok(contents)
}
