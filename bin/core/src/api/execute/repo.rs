use std::collections::HashSet;
use interpolate::Interpolator;

use anyhow::{Context, anyhow};
use database::mungos::{
  by_id::update_one_by_id,
  mongodb::{
    bson::{doc, to_document},
  },
};
use formatting::format_serror;
use komodo_client::{
  api::{execute::*, write::RefreshRepoCache},
  entities::{
    komodo_timestamp,
    permission::PermissionLevel,
    repo::Repo,
    server::Server,
    update::{Log, Update},
  },
};
use mogh_resolver::Resolve;
use periphery_client::api;

use crate::{
  api::write::WriteArgs,
  helpers::{
    git_token, periphery_client, query::get_variables,
    update::update_update,
  },
  permission::get_check_permissions,
  resource::{self, refresh_repo_state_cache},
  state::{action_states, db_client},
};

use super::{ExecuteArgs, ExecuteRequest};

impl super::BatchExecute for BatchCloneRepo {
  type Resource = Repo;
  fn single_request(repo: String) -> ExecuteRequest {
    ExecuteRequest::CloneRepo(CloneRepo { repo })
  }
}

impl Resolve<ExecuteArgs> for BatchCloneRepo {
  #[instrument(
    "BatchCloneRepo",
    skip_all,
    fields(
      task_id = task_id.to_string(),
      operator = user.id,
      pattern = self.pattern,
      tags = self.tags.join(","),
    )
  )]
  async fn resolve(
    self,
    ExecuteArgs { user, task_id, .. }: &ExecuteArgs,
  ) -> mogh_error::Result<BatchExecutionResponse> {
    Ok(
      super::batch_execute::<BatchCloneRepo>(
        &self.pattern,
        self.tags,
        user,
      )
      .await?,
    )
  }
}

impl Resolve<ExecuteArgs> for CloneRepo {
  #[instrument(
    "CloneRepo",
    skip_all,
    fields(
      task_id = task_id.to_string(),
      operator = user.id,
      update_id = update.id,
      repo = self.repo,
    )
  )]
  async fn resolve(
    self,
    ExecuteArgs {
      user,
      update,
      task_id,
    }: &ExecuteArgs,
  ) -> mogh_error::Result<Update> {
    let mut repo = get_check_permissions::<Repo>(
      &self.repo,
      user,
      PermissionLevel::Execute.into(),
    )
    .await?;

    // get the action state for the repo (or insert default).
    let action_state =
      action_states().repo.get_or_insert_default(&repo.id).await;

    // This will set action state back to default when dropped.
    // Will also check to ensure repo not already busy before updating.
    let action_guard =
      action_state.update(|state| state.cloning = true)?;

    let mut update = update.clone();
    update_update(update.clone()).await?;

    if repo.config.server_id.is_empty() {
      return Err(anyhow!("repo has no server attached").into());
    }

    let git_token = git_token(
      &repo.config.git_provider,
      &repo.config.git_account,
      |https| repo.config.git_https = https,
    )
    .await
    .with_context(
      || format!("Failed to get git token in call to db. This is a database error, not a token exisitence error. Stopping run. | {} | {}", repo.config.git_provider, repo.config.git_account),
    )?;

    let server =
      resource::get::<Server>(&repo.config.server_id).await?;

    let periphery = periphery_client(&server).await?;

    // interpolate variables / secrets, returning the sanitizing replacers to send to
    // periphery so it may sanitize the final command for safe logging (avoids exposing secret values)
    let secret_replacers =
      interpolate(&mut repo, &mut update).await?;

    let logs = match periphery
      .request(api::git::CloneRepo {
        args: (&repo).into(),
        git_token,
        environment: repo.config.env_vars()?,
        env_file_path: repo.config.env_file_path,
        on_clone: repo.config.on_clone.into(),
        on_pull: repo.config.on_pull.into(),
        skip_secret_interp: repo.config.skip_secret_interp,
        replacers: secret_replacers.into_iter().collect(),
      })
      .await
    {
      Ok(res) => res.res.logs,
      Err(e) => {
        vec![Log::error(
          "Clone Repo",
          format_serror(&e.context("Failed to clone repo").into()),
        )]
      }
    };

    update.logs.extend(logs);
    update.finalize();

    if update.success {
      update_last_pulled_time(&repo.name).await;
    }

    if let Err(e) = (RefreshRepoCache { repo: repo.id })
      .resolve(&WriteArgs { user: user.clone() })
      .await
      .map_err(|e| e.error)
      .context("Failed to refresh repo cache")
    {
      update.push_error_log(
        "Refresh Repo cache",
        format_serror(&e.into()),
      );
    };

    // Drop action guard before updating
    // clients to requery action state
    drop(action_guard);

    handle_repo_update_return(update).await
  }
}

impl super::BatchExecute for BatchPullRepo {
  type Resource = Repo;
  fn single_request(repo: String) -> ExecuteRequest {
    ExecuteRequest::PullRepo(PullRepo { repo })
  }
}

impl Resolve<ExecuteArgs> for BatchPullRepo {
  #[instrument(
    "BatchPullRepo",
    skip_all,
    fields(
      task_id = task_id.to_string(),
      operator = user.id,
      pattern = self.pattern,
      tags = self.tags.join(","),
    )
  )]
  async fn resolve(
    self,
    ExecuteArgs { user, task_id, .. }: &ExecuteArgs,
  ) -> mogh_error::Result<BatchExecutionResponse> {
    Ok(
      super::batch_execute::<BatchPullRepo>(
        &self.pattern,
        self.tags,
        user,
      )
      .await?,
    )
  }
}

impl Resolve<ExecuteArgs> for PullRepo {
  #[instrument(
    "PullRepo",
    skip_all,
    fields(
      task_id = task_id.to_string(),
      operator = user.id,
      update_id = update.id,
      repo = self.repo,
    )
  )]
  async fn resolve(
    self,
    ExecuteArgs {
      user,
      update,
      task_id,
    }: &ExecuteArgs,
  ) -> mogh_error::Result<Update> {
    let mut repo = get_check_permissions::<Repo>(
      &self.repo,
      user,
      PermissionLevel::Execute.into(),
    )
    .await?;

    // get the action state for the repo (or insert default).
    let action_state =
      action_states().repo.get_or_insert_default(&repo.id).await;

    // This will set action state back to default when dropped.
    // Will also check to ensure repo not already busy before updating.
    let action_guard =
      action_state.update(|state| state.pulling = true)?;

    let mut update = update.clone();

    update_update(update.clone()).await?;

    if repo.config.server_id.is_empty() {
      return Err(anyhow!("repo has no server attached").into());
    }

    let git_token = git_token(
      &repo.config.git_provider,
      &repo.config.git_account,
      |https| repo.config.git_https = https,
    )
    .await
    .with_context(
      || format!("Failed to get git token in call to db. This is a database error, not a token exisitence error. Stopping run. | {} | {}", repo.config.git_provider, repo.config.git_account),
    )?;

    let server =
      resource::get::<Server>(&repo.config.server_id).await?;

    let periphery = periphery_client(&server).await?;

    // interpolate variables / secrets, returning the sanitizing replacers to send to
    // periphery so it may sanitize the final command for safe logging (avoids exposing secret values)
    let secret_replacers =
      interpolate(&mut repo, &mut update).await?;

    let logs = match periphery
      .request(api::git::PullRepo {
        args: (&repo).into(),
        git_token,
        environment: repo.config.env_vars()?,
        env_file_path: repo.config.env_file_path,
        on_pull: repo.config.on_pull.into(),
        skip_secret_interp: repo.config.skip_secret_interp,
        replacers: secret_replacers.into_iter().collect(),
      })
      .await
    {
      Ok(res) => {
        update.commit_hash = res.res.commit_hash.unwrap_or_default();
        res.res.logs
      }
      Err(e) => {
        vec![Log::error(
          "pull repo",
          format_serror(&e.context("failed to pull repo").into()),
        )]
      }
    };

    update.logs.extend(logs);

    update.finalize();

    if update.success {
      update_last_pulled_time(&repo.name).await;
    }

    if let Err(e) = (RefreshRepoCache { repo: repo.id })
      .resolve(&WriteArgs { user: user.clone() })
      .await
      .map_err(|e| e.error)
      .context("Failed to refresh repo cache")
    {
      update.push_error_log(
        "Refresh Repo cache",
        format_serror(&e.into()),
      );
    };

    // Drop action guard before updating
    // clients to requery action state
    drop(action_guard);

    handle_repo_update_return(update).await
  }
}

#[instrument(
  "HandleRepoEarlyReturn",
  skip_all,
  fields(update_id = update.id)
)]
async fn handle_repo_update_return(
  update: Update,
) -> mogh_error::Result<Update> {
  // Need to manually update the update before cache refresh,
  // and before broadcast with add_update.
  // The Err case of to_document should be unreachable,
  // but will fail to update cache in that case.
  if let Ok(update_doc) = to_document(&update) {
    let _ = update_one_by_id(
      &db_client().updates,
      &update.id,
      database::mungos::update::Update::Set(update_doc),
      None,
    )
    .await;
    refresh_repo_state_cache().await;
  }
  update_update(update.clone()).await?;
  Ok(update)
}

#[instrument("UpdateLastPulledTime")]
async fn update_last_pulled_time(repo_name: &str) {
  let res = db_client()
    .repos
    .update_one(
      doc! { "name": repo_name },
      doc! { "$set": { "info.last_pulled_at": komodo_timestamp() } },
    )
    .await;
  if let Err(e) = res {
    warn!(
      "failed to update repo last_pulled_at | repo: {repo_name} | {e:#}",
    );
  }
}

/// Non-secret Variables into the repo environment and on_clone / on_pull commands.
#[instrument(
  "Interpolate",
  skip_all,
  fields(
    skip_secret_interp = repo.config.skip_secret_interp
  )
)]
async fn interpolate(
  repo: &mut Repo,
  update: &mut Update,
) -> anyhow::Result<HashSet<(String, String)>> {
  if !repo.config.skip_secret_interp {
    let variables = get_variables().await?;
    let empty = Default::default();
    let mut interpolator = Interpolator::new(Some(&variables), &empty);
    interpolator
      .interpolate_repo(repo)?
      .push_logs(&mut update.logs);
    Ok(interpolator.secret_replacers)
  } else {
    Ok(Default::default())
  }
}
