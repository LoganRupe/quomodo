use ::quadlet as quadlet_lib;
use anyhow::{Context, anyhow};
use database::mungos::mongodb::bson::{doc, to_document};
use formatting::format_serror;
use interpolate::Interpolator;
use komodo_client::{
  api::execute::*,
  entities::{
    permission::PermissionLevel,
    repo::Repo,
    stack::{Stack, StackInfo, StackState},
    update::{Log, Update},
  },
};
use mogh_resolver::Resolve;
use periphery_client::api::quadlet::{self, StackUnitAction};

use crate::{
  api::write::refresh_stack_cache,
  helpers::{
    periphery_client,
    query::get_variables,
    stack_git_token,
    update::{add_update_without_send, update_update},
  },
  monitor::refresh_server_cache,
  permission::get_check_permissions,
  resource,
  secrets::resolve_stack_secrets,
  stack::{execute::execute_stack_units, setup_stack_execution},
  state::{action_states, db_client, stack_status_cache},
};

use super::{ExecuteArgs, ExecuteRequest};

impl super::BatchExecute for BatchDeployStack {
  type Resource = Stack;
  fn single_request(stack: String) -> ExecuteRequest {
    ExecuteRequest::DeployStack(DeployStack {
      stack,
      services: Vec::new(),
    })
  }
}

impl Resolve<ExecuteArgs> for BatchDeployStack {
  #[instrument(
    "BatchDeployStack",
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
      super::batch_execute::<BatchDeployStack>(
        &self.pattern,
        self.tags,
        user,
      )
      .await?,
    )
  }
}

impl Resolve<ExecuteArgs> for DeployStack {
  #[instrument(
    "DeployStack",
    skip_all,
    fields(
      task_id = task_id.to_string(),
      operator = user.id,
      update_id = update.id,
      stack = self.stack,
      services = format!("{:?}", self.services),
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
    let (mut stack, server) = setup_stack_execution(
      &self.stack,
      user,
      PermissionLevel::Execute.into(),
    )
    .await?;

    let mut repo = if !stack.config.linked_repo.is_empty() {
      crate::resource::get::<Repo>(&stack.config.linked_repo)
        .await?
        .into()
    } else {
      None
    };

    // get the action state for the stack (or insert default).
    let action_state =
      action_states().stack.get_or_insert_default(&stack.id).await;

    // Will check to ensure stack not already busy before updating, and return Err if so.
    // The returned guard will set the action state back to default when dropped.
    let action_guard =
      action_state.update(|state| state.deploying = true)?;

    let mut update = update.clone();

    update_update(update.clone()).await?;

    if !self.services.is_empty() {
      update.logs.push(Log::simple(
        "Unit/s",
        format!(
          "Execution requested for Stack unit/s {}",
          self.services.join(", ")
        ),
      ))
    }

    let git_token =
      stack_git_token(&mut stack, repo.as_mut()).await?;

    let registry_token = crate::helpers::registry_token(
      &stack.config.registry_provider,
      &stack.config.registry_account,
    ).await.with_context(
      || format!("Failed to get registry token in call to db. Stopping run. | {} | {}", stack.config.registry_provider, stack.config.registry_account),
    )?;

    // Read the latest files from Core's clone so the secret
    // references can be resolved before the agent deploys.
    let contents = refresh_stack_cache(&stack)
      .await
      .context("Failed to refresh stack files before deploy")?;
    if !contents.errors.is_empty() {
      return Err(
        anyhow!(
          "Stack files have errors:\n{}",
          contents
            .errors
            .iter()
            .map(|e| format!("{}: {}", e.path, e.contents))
            .collect::<Vec<_>>()
            .join("\n")
        )
        .into(),
      );
    }

    let variables = get_variables().await?;

    // Non-secret variables into the pre / post deploy commands and repo commands.
    let mut replacers = if !stack.config.skip_secret_interp {
      let empty = Default::default();
      let mut interpolator =
        Interpolator::new(Some(&variables), &empty);
      interpolator.interpolate_stack(&mut stack)?;
      if let Some(repo) = repo.as_mut()
        && !repo.config.skip_secret_interp
      {
        interpolator.interpolate_repo(repo)?;
      }
      interpolator.push_logs(&mut update.logs);
      interpolator.secret_replacers
    } else {
      Default::default()
    };

    let resolved = if stack.config.skip_secret_interp {
      Default::default()
    } else {
      resolve_stack_secrets(&stack, &contents.files, &variables)
        .await?
    };
    if !resolved.secrets.is_empty() {
      update.logs.push(Log::simple(
        "Resolve Secrets",
        format!(
          "Resolved from collection '{}': {}",
          stack.config.secrets_collection,
          resolved
            .secrets
            .keys()
            .cloned()
            .collect::<Vec<_>>()
            .join(", ")
        ),
      ));
      for (name, value) in &resolved.secrets {
        replacers.insert((value.clone(), format!("[[{name}]]")));
      }
    }

    let quadlet::DeployStackResponse {
      logs,
      deployed,
      units,
      file_contents,
      remote_errors,
      contents_hash,
      commit_hash,
      commit_message,
    } = periphery_client(&server)
      .await?
      .request(quadlet::DeployStack {
        stack: stack.clone(),
        repo,
        git_token,
        registry_token,
        variables,
        secrets: resolved.secrets,
        replacers: replacers.into_iter().collect(),
        services: self.services,
      })
      .await?;

    update.logs.extend(logs);

    let update_info = async {
      let latest_units = if units.is_empty() {
        stack.info.latest_units.clone()
      } else {
        units
      };

      let (
        deployed_units,
        deployed_contents,
        deployed_contents_hash,
        deployed_hash,
        deployed_message,
      ) = if deployed {
        (
          Some(latest_units.clone()),
          Some(file_contents.clone()),
          contents_hash,
          commit_hash.clone(),
          commit_message.clone(),
        )
      } else {
        (
          stack.info.deployed_units,
          stack.info.deployed_contents,
          stack.info.deployed_contents_hash,
          stack.info.deployed_hash,
          stack.info.deployed_message,
        )
      };

      let info = StackInfo {
        deployed_units,
        deployed_contents,
        deployed_contents_hash,
        deployed_hash,
        deployed_message,
        latest_units,
        remote_contents: Some(file_contents),
        remote_errors: Some(remote_errors),
        latest_hash: commit_hash,
        latest_message: commit_message,
      };

      let info = to_document(&info)
        .context("Failed to serialize stack info to bson")?;

      db_client()
        .stacks
        .update_one(
          doc! { "name": &stack.name },
          doc! { "$set": { "info": info } },
        )
        .await
        .context("Failed to update stack info on db")?;
      anyhow::Ok(())
    };

    if let Err(e) = update_info.await {
      update.push_error_log(
        "Refresh Stack Info",
        format_serror(
          &e.context("Failed to refresh stack info on db").into(),
        ),
      )
    }

    // Ensure cached stack state up to date by updating server cache
    refresh_server_cache(&server, true).await;

    update.finalize();

    // Drop action guard before updating
    // clients to requery action state
    drop(action_guard);
    update_update(update.clone()).await?;

    Ok(update)
  }
}

impl super::BatchExecute for BatchDeployStackIfChanged {
  type Resource = Stack;
  fn single_request(stack: String) -> ExecuteRequest {
    ExecuteRequest::DeployStackIfChanged(DeployStackIfChanged {
      stack,
    })
  }
}

impl Resolve<ExecuteArgs> for BatchDeployStackIfChanged {
  #[instrument(
    "BatchDeployStackIfChanged",
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
      super::batch_execute::<BatchDeployStackIfChanged>(
        &self.pattern,
        self.tags,
        user,
      )
      .await?,
    )
  }
}

impl Resolve<ExecuteArgs> for DeployStackIfChanged {
  #[instrument(
    "DeployStackIfChanged",
    skip_all,
    fields(
      task_id = task_id.to_string(),
      operator = user.id,
      update_id = update.id,
      stack = self.stack,
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
    let stack = get_check_permissions::<Stack>(
      &self.stack,
      user,
      PermissionLevel::Execute.into(),
    )
    .await?;

    let contents = refresh_stack_cache(&stack).await?;
    let stack = resource::get::<Stack>(&stack.id).await?;

    let mut update = update.clone();

    let state = stack_status_cache()
      .get(&stack.id)
      .await
      .map(|s| s.curr.state)
      .unwrap_or_default();

    // Hash the latest files together with the resolved secret values,
    // so a rotated secret also counts as a change.
    let latest_hash =
      if contents.errors.is_empty() && !contents.files.is_empty() {
        let variables = get_variables().await?;
        let resolved = if stack.config.skip_secret_interp {
          Default::default()
        } else {
          resolve_stack_secrets(&stack, &contents.files, &variables)
            .await?
        };
        Some(quadlet_lib::contents_hash(
          &contents.files,
          &resolved.hashes,
        ))
      } else {
        None
      };

    let unchanged = latest_hash.is_some()
      && latest_hash == stack.info.deployed_contents_hash
      && state == StackState::Running;

    if unchanged {
      update.push_simple_log(
        "Diff stack files",
        String::from("Deploy cancelled after no changes detected."),
      );
      update.finalize();
      return Ok(update);
    }

    // Don't actually send it here, let the handler send it after it can set action state.
    // This is usually done in crate::helpers::update::init_execution_update.
    update.id = add_update_without_send(&update).await?;

    DeployStack {
      stack: stack.name,
      services: Vec::new(),
    }
    .resolve(&ExecuteArgs {
      user: user.clone(),
      update,
      task_id: *task_id,
    })
    .await
  }
}

impl super::BatchExecute for BatchPullStack {
  type Resource = Stack;
  fn single_request(stack: String) -> ExecuteRequest {
    ExecuteRequest::PullStack(PullStack {
      stack,
      services: Vec::new(),
    })
  }
}

impl Resolve<ExecuteArgs> for BatchPullStack {
  #[instrument(
    "BatchPullStack",
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
      super::batch_execute::<BatchPullStack>(
        &self.pattern,
        self.tags,
        user,
      )
      .await?,
    )
  }
}

impl Resolve<ExecuteArgs> for PullStack {
  #[instrument(
    "PullStack",
    skip_all,
    fields(
      task_id = task_id.to_string(),
      operator = user.id,
      update_id = update.id,
      stack = self.stack,
      services = format!("{:?}", self.services),
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
    let (mut stack, server) = setup_stack_execution(
      &self.stack,
      user,
      PermissionLevel::Execute.into(),
    )
    .await?;

    let mut repo = if !stack.config.linked_repo.is_empty() {
      crate::resource::get::<Repo>(&stack.config.linked_repo)
        .await?
        .into()
    } else {
      None
    };

    // get the action state for the stack (or insert default).
    let action_state =
      action_states().stack.get_or_insert_default(&stack.id).await;

    // Will check to ensure stack not already busy before updating, and return Err if so.
    // The returned guard will set the action state back to default when dropped.
    let action_guard =
      action_state.update(|state| state.pulling = true)?;

    let mut update = update.clone();

    update_update(update.clone()).await?;

    let git_token =
      stack_git_token(&mut stack, repo.as_mut()).await?;

    let registry_token = crate::helpers::registry_token(
      &stack.config.registry_provider,
      &stack.config.registry_account,
    ).await.with_context(
      || format!("Failed to get registry token in call to db. Stopping run. | {} | {}", stack.config.registry_provider, stack.config.registry_account),
    )?;

    let quadlet::PullStackResponse { logs } =
      periphery_client(&server)
        .await?
        .request(quadlet::PullStack {
          stack: stack.clone(),
          repo,
          git_token,
          registry_token,
          services: self.services,
        })
        .await?;

    update.logs.extend(logs);

    update.finalize();

    // Drop action guard before updating
    // clients to requery action state
    drop(action_guard);
    update_update(update.clone()).await?;

    Ok(update)
  }
}

impl Resolve<ExecuteArgs> for StartStack {
  #[instrument(
    "StartStack",
    skip_all,
    fields(
      task_id = task_id.to_string(),
      operator = user.id,
      update_id = update.id,
      stack = self.stack,
      services = format!("{:?}", self.services),
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
    execute_stack_units(
      &self.stack,
      StackUnitAction::Start,
      self.services,
      user,
      |state| state.starting = true,
      update.clone(),
    )
    .await
    .map_err(Into::into)
  }
}

impl Resolve<ExecuteArgs> for RestartStack {
  #[instrument(
    "RestartStack",
    skip_all,
    fields(
      task_id = task_id.to_string(),
      operator = user.id,
      update_id = update.id,
      stack = self.stack,
      services = format!("{:?}", self.services),
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
    execute_stack_units(
      &self.stack,
      StackUnitAction::Restart,
      self.services,
      user,
      |state| state.restarting = true,
      update.clone(),
    )
    .await
    .map_err(Into::into)
  }
}

impl Resolve<ExecuteArgs> for StopStack {
  #[instrument(
    "StopStack",
    skip_all,
    fields(
      task_id = task_id.to_string(),
      operator = user.id,
      update_id = update.id,
      stack = self.stack,
      services = format!("{:?}", self.services),
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
    execute_stack_units(
      &self.stack,
      StackUnitAction::Stop,
      self.services,
      user,
      |state| state.stopping = true,
      update.clone(),
    )
    .await
    .map_err(Into::into)
  }
}

impl super::BatchExecute for BatchDestroyStack {
  type Resource = Stack;
  fn single_request(stack: String) -> ExecuteRequest {
    ExecuteRequest::DestroyStack(DestroyStack { stack })
  }
}

impl Resolve<ExecuteArgs> for BatchDestroyStack {
  #[instrument(
    "BatchDestroyStack",
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
      super::batch_execute::<BatchDestroyStack>(
        &self.pattern,
        self.tags,
        user,
      )
      .await?,
    )
  }
}

impl Resolve<ExecuteArgs> for DestroyStack {
  #[instrument(
    "DestroyStack",
    skip_all,
    fields(
      task_id = task_id.to_string(),
      operator = user.id,
      update_id = update.id,
      stack = self.stack,
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
    let (stack, server) = setup_stack_execution(
      &self.stack,
      user,
      PermissionLevel::Execute.into(),
    )
    .await?;

    // get the action state for the stack (or insert default).
    let action_state =
      action_states().stack.get_or_insert_default(&stack.id).await;

    // Will check to ensure stack not already busy before updating, and return Err if so.
    // The returned guard will set the action state back to default when dropped.
    let action_guard =
      action_state.update(|state| state.destroying = true)?;

    let mut update = update.clone();

    update_update(update.clone()).await?;

    let log = periphery_client(&server)
      .await?
      .request(quadlet::DestroyStack {
        stack: stack.name.clone(),
      })
      .await?;

    update.logs.push(log);

    if update.success {
      // Clear the deployed state so the next deploy is a fresh install.
      let info = StackInfo {
        deployed_units: None,
        deployed_contents: None,
        deployed_contents_hash: None,
        deployed_hash: None,
        deployed_message: None,
        latest_units: stack.info.latest_units,
        remote_contents: stack.info.remote_contents,
        remote_errors: stack.info.remote_errors,
        latest_hash: stack.info.latest_hash,
        latest_message: stack.info.latest_message,
      };
      if let Err(e) = async {
        let info = to_document(&info)
          .context("Failed to serialize stack info to bson")?;
        db_client()
          .stacks
          .update_one(
            doc! { "name": &stack.name },
            doc! { "$set": { "info": info } },
          )
          .await
          .context("Failed to update stack info on db")
      }
      .await
      {
        update.push_error_log(
          "Refresh Stack Info",
          format_serror(&e.into()),
        );
      }
    }

    // Ensure cached stack state up to date by updating server cache
    refresh_server_cache(&server, true).await;

    update.finalize();

    // Drop action guard before updating
    // clients to requery action state
    drop(action_guard);
    update_update(update.clone()).await?;

    Ok(update)
  }
}
