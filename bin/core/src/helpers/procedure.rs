use std::time::{Duration, Instant};

use anyhow::{Context, anyhow};
use database::mungos::by_id::find_one_by_id;
use formatting::{Color, bold, colored, format_serror, muted};
use futures_util::future::join_all;
use komodo_client::{
  api::execute::*,
  entities::{
    action::Action,
    permission::PermissionLevel,
    procedure::{Procedure, ProcedureStage},
    repo::Repo,
    resource::ResourceQuery,
    stack::Stack,
    update::Update,
    user::procedure_user,
  },
};
use mogh_resolver::Resolve;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::{
  api::{
    execute::{ExecuteArgs, ExecuteRequest},
    write::WriteArgs,
  },
  config::core_config,
  resource::{KomodoResource, list_full_for_user_using_pattern},
  state::{all_resources_cache, db_client},
};

use super::{
  query::get_all_tags,
  update::{init_execution_update, update_update},
};

pub async fn execute_procedure(
  procedure: &Procedure,
  update: &Mutex<Update>,
  cancel: CancellationToken,
) -> anyhow::Result<()> {
  for stage in &procedure.config.stages {
    if cancel.is_cancelled() {
      add_line_to_update(
        update,
        &format!(
          "{}: Cancelled before stage: '{}'",
          muted("ERROR"),
          bold(&stage.name)
        ),
      )
      .await;
      return Err(anyhow!("Procedure cancelled before completion"));
    }
    if !stage.enabled {
      continue;
    }
    add_line_to_update(
      update,
      &format!(
        "{}: Executing stage: '{}'",
        muted("INFO"),
        bold(&stage.name)
      ),
    )
    .await;
    let timer = Instant::now();
    execute_procedure_stage(
      stage
        .executions
        .iter()
        .filter(|item| item.enabled)
        .map(|item| item.execution.clone())
        .collect(),
      &procedure.id,
      &procedure.name,
      update,
    )
    .await
    .with_context(|| {
      format!(
        "Failed stage '{}' execution after {:?}",
        bold(&stage.name),
        timer.elapsed(),
      )
    })?;
    add_line_to_update(
      update,
      &format!(
        "{}: {} stage '{}' execution in {:?}",
        muted("INFO"),
        colored("Finished", Color::Green),
        bold(&stage.name),
        timer.elapsed()
      ),
    )
    .await;
  }

  Ok(())
}

#[instrument("ExecuteProcedureStage", skip_all)]
async fn execute_procedure_stage(
  _executions: Vec<Execution>,
  parent_id: &str,
  parent_name: &str,
  update: &Mutex<Update>,
) -> anyhow::Result<()> {
  let mut executions = Vec::with_capacity(_executions.capacity());
  // Expands the batch executions into their matching
  // single executions, and passes through the rest.
  macro_rules! expand_batch_executions {
    ($($Variant:ident),* $(,)?) => {
      for execution in _executions {
        match execution {
          $(
            Execution::$Variant(exec) => {
              extend_batch_execution::<$Variant>(
                &exec.pattern,
                &exec.tags,
                &mut executions,
              )
              .await?;
            }
          )*
          execution => executions.push(execution),
        }
      }
    };
  }
  expand_batch_executions!(
    BatchRunAction,
    BatchRunProcedure,
    BatchCloneRepo,
    BatchPullRepo,
    BatchDeployStack,
    BatchDeployStackIfChanged,
    BatchPullStack,
    BatchDestroyStack,
  );
  let futures = executions.into_iter().map(|execution| async move {
    let now = Instant::now();
    add_line_to_update(
      update,
      &format!("{}: Executing: {execution:?}", muted("INFO")),
    )
    .await;
    let fail_log = format!(
      "{}: Failed on {execution:?}",
      colored("ERROR", Color::Red)
    );
    let res =
      execute_execution(execution.clone(), parent_id, parent_name)
        .await
        .context(fail_log);
    add_line_to_update(
      update,
      &format!(
        "{}: {} execution in {:?}: {execution:?}",
        muted("INFO"),
        colored("Finished", Color::Green),
        now.elapsed()
      ),
    )
    .await;
    res
  });
  join_all(futures)
    .await
    .into_iter()
    .collect::<anyhow::Result<Vec<_>>>()?;
  Ok(())
}

#[instrument(
  "ExecuteProcedureExecution",
  skip(parent_id, parent_name)
)]
async fn execute_execution(
  execution: Execution,
  // used to prevent recursive procedure
  parent_id: &str,
  parent_name: &str,
) -> anyhow::Result<()> {
  let user = procedure_user().to_owned();
  let task_id = Uuid::new_v4();
  // Standard pattern: init update, resolve with ExecuteArgs, handle result.
  macro_rules! resolve_execute {
    ($Variant:ident, $req:expr) => {{
      let req = ExecuteRequest::$Variant($req);
      let update = init_execution_update(&req, &user).await?;
      let ExecuteRequest::$Variant(req) = req else {
        unreachable!()
      };
      let update_id = update.id.clone();
      handle_resolve_result(
        req
          .resolve(&ExecuteArgs {
            user,
            update,
            task_id,
          })
          .await
          .map_err(|e| e.error)
          .context(concat!("Failed at ", stringify!($Variant))),
        &update_id,
      )
      .await?
    }};
  }

  // Batch methods must be expanded in `execute_stage`.
  macro_rules! batch_not_implemented {
    ($Variant:ident) => {
      return Err(anyhow!(concat!(
        "Batch method ",
        stringify!($Variant),
        " not implemented correctly"
      )))
    };
  }

  let update = match execution {
    Execution::None(_) => return Ok(()),
    // Special: self-referential guard
    Execution::RunProcedure(req) => {
      if req.procedure == parent_id || req.procedure == parent_name {
        return Err(anyhow!("Self referential procedure detected"));
      }
      resolve_execute!(RunProcedure, req)
    }
    Execution::CancelProcedure(req) => {
      resolve_execute!(CancelProcedure, req)
    }
    // Special: write operation
    Execution::CommitSync(req) => req
      .resolve(&WriteArgs { user })
      .await
      .map_err(|e| e.error)
      .context("Failed at CommitSync")?,
    // Special: sleep
    Execution::Sleep(req) => {
      let duration = Duration::from_millis(req.duration_ms as u64);
      tokio::time::sleep(duration).await;
      Update {
        success: true,
        ..Default::default()
      }
    }
    // Batch variants
    Execution::BatchRunProcedure(_) => {
      batch_not_implemented!(BatchRunProcedure)
    }
    Execution::BatchRunAction(_) => {
      batch_not_implemented!(BatchRunAction)
    }
    Execution::BatchCloneRepo(_) => {
      batch_not_implemented!(BatchCloneRepo)
    }
    Execution::BatchPullRepo(_) => {
      batch_not_implemented!(BatchPullRepo)
    }
    Execution::BatchDeployStack(_) => {
      batch_not_implemented!(BatchDeployStack)
    }
    Execution::BatchDeployStackIfChanged(_) => {
      batch_not_implemented!(BatchDeployStackIfChanged)
    }
    Execution::BatchPullStack(_) => {
      batch_not_implemented!(BatchPullStack)
    }
    Execution::BatchDestroyStack(_) => {
      batch_not_implemented!(BatchDestroyStack)
    }
    // Standard executions
    Execution::RunAction(req) => resolve_execute!(RunAction, req),
    Execution::CancelAction(req) => {
      resolve_execute!(CancelAction, req)
    }
    Execution::CloneRepo(req) => resolve_execute!(CloneRepo, req),
    Execution::PullRepo(req) => resolve_execute!(PullRepo, req),
    Execution::PruneImages(req) => resolve_execute!(PruneImages, req),
    Execution::PruneSystem(req) => resolve_execute!(PruneSystem, req),
    Execution::RunSync(req) => resolve_execute!(RunSync, req),
    Execution::DeployStack(req) => resolve_execute!(DeployStack, req),
    Execution::DeployStackIfChanged(req) => {
      resolve_execute!(DeployStackIfChanged, req)
    }
    Execution::PullStack(req) => resolve_execute!(PullStack, req),
    Execution::StartStack(req) => resolve_execute!(StartStack, req),
    Execution::RestartStack(req) => {
      resolve_execute!(RestartStack, req)
    }
    Execution::StopStack(req) => resolve_execute!(StopStack, req),
    Execution::DestroyStack(req) => {
      resolve_execute!(DestroyStack, req)
    }
    Execution::ClearRepoCache(req) => {
      resolve_execute!(ClearRepoCache, req)
    }
    Execution::BackupCoreDatabase(req) => {
      resolve_execute!(BackupCoreDatabase, req)
    }
    Execution::RotateAllServerKeys(req) => {
      resolve_execute!(RotateAllServerKeys, req)
    }
    Execution::RotateCoreKeys(req) => {
      resolve_execute!(RotateCoreKeys, req)
    }
  };

  if update.success {
    Ok(())
  } else {
    Err(anyhow!(
      "{}: Execution not successful, see update: '{}/updates/{}'",
      colored("ERROR", Color::Red),
      core_config().host,
      update.id,
    ))
  }
}

/// If the call to .resolve returns Err, the update may not be closed.
/// This will ensure it is closed with error log attached.
async fn handle_resolve_result(
  res: anyhow::Result<Update>,
  update_id: &str,
) -> anyhow::Result<Update> {
  match res {
    Ok(res) => Ok(res),
    Err(e) => {
      let mut update =
        find_one_by_id(&db_client().updates, update_id)
          .await
          .context("Failed to query to db")?
          .context("no update exists with given id")?;
      update
        .push_error_log("Execution error", format_serror(&e.into()));
      update.finalize();
      update_update(update.clone()).await?;
      Ok(update)
    }
  }
}

/// ASSUMES FIRST LOG IS ALREADY CREATED
async fn add_line_to_update(update: &Mutex<Update>, line: &str) {
  let mut lock = update.lock().await;
  let log = &mut lock.logs[0];
  log.stdout.push('\n');
  log.stdout.push_str(line);
  let update = lock.clone();
  drop(lock);
  if let Err(e) = update_update(update).await {
    error!("Failed to update an update during procedure | {e:#}");
  };
}

async fn extend_batch_execution<E: ExtendBatch>(
  pattern: &str,
  tags: &[String],
  executions: &mut Vec<Execution>,
) -> anyhow::Result<()> {
  let all_tags = if tags.is_empty() {
    Vec::new()
  } else {
    get_all_tags(None).await?
  };
  let more = list_full_for_user_using_pattern::<E::Resource>(
    pattern,
    ResourceQuery {
      tags: tags.to_vec(),
      ..Default::default()
    },
    None,
    None,
    procedure_user(),
    PermissionLevel::Read.into(),
    &all_tags,
  )
  .await?
  .into_iter()
  .map(|resource| E::single_execution(resource.name));
  executions.extend(more);
  Ok(())
}

trait ExtendBatch {
  type Resource: KomodoResource;
  fn single_execution(name: String) -> Execution;
}

impl ExtendBatch for BatchRunProcedure {
  type Resource = Procedure;
  fn single_execution(procedure: String) -> Execution {
    Execution::RunProcedure(RunProcedure { procedure })
  }
}

impl ExtendBatch for BatchRunAction {
  type Resource = Action;
  fn single_execution(action: String) -> Execution {
    Execution::RunAction(RunAction {
      action,
      args: Default::default(),
    })
  }
}

impl ExtendBatch for BatchCloneRepo {
  type Resource = Repo;
  fn single_execution(repo: String) -> Execution {
    Execution::CloneRepo(CloneRepo { repo })
  }
}

impl ExtendBatch for BatchPullRepo {
  type Resource = Repo;
  fn single_execution(repo: String) -> Execution {
    Execution::PullRepo(PullRepo { repo })
  }
}

impl ExtendBatch for BatchDeployStack {
  type Resource = Stack;
  fn single_execution(stack: String) -> Execution {
    Execution::DeployStack(DeployStack {
      stack,
      services: Vec::new(),
    })
  }
}

impl ExtendBatch for BatchDeployStackIfChanged {
  type Resource = Stack;
  fn single_execution(stack: String) -> Execution {
    Execution::DeployStackIfChanged(DeployStackIfChanged { stack })
  }
}

impl ExtendBatch for BatchPullStack {
  type Resource = Stack;
  fn single_execution(stack: String) -> Execution {
    Execution::PullStack(PullStack {
      stack,
      services: Vec::new(),
    })
  }
}

impl ExtendBatch for BatchDestroyStack {
  type Resource = Stack;
  fn single_execution(stack: String) -> Execution {
    Execution::DestroyStack(DestroyStack { stack })
  }
}

pub fn replace_procedure_stage_ids_with_names(
  stages: &mut Vec<ProcedureStage>,
) {
  let all = all_resources_cache().load();
  for stage in stages {
    for execution in &mut stage.executions {
      // Replaces an id field on config with the resource name from `all`.
      macro_rules! replace_id_with_name {
          ($($Variant:ident => $field:ident, $collection:ident);* $(;)?) => {
            match &mut execution.execution {
              $(
                Execution::$Variant(config) => {
                  config.$field = all
                    .$collection
                    .get(&config.$field)
                    .map(|r| r.name.clone())
                    .unwrap_or_default();
                }
              )*
              // No-op variants
              Execution::None(_)
              | Execution::BatchRunProcedure(_)
              | Execution::BatchRunAction(_)
              | Execution::BatchCloneRepo(_)
              | Execution::BatchPullRepo(_)
              | Execution::BatchDeployStack(_)
              | Execution::BatchDeployStackIfChanged(_)
              | Execution::BatchPullStack(_)
              | Execution::BatchDestroyStack(_)
              | Execution::ClearRepoCache(_)
              | Execution::BackupCoreDatabase(_)
              | Execution::RotateAllServerKeys(_)
              | Execution::RotateCoreKeys(_)
              | Execution::Sleep(_) => {}
            }
          };
        }

      replace_id_with_name!(
        RunProcedure => procedure, procedures;
        CancelProcedure => procedure, procedures;
        RunAction => action, actions;
        CancelAction => action, actions;
        CloneRepo => repo, repos;
        PullRepo => repo, repos;
        PruneImages => server, servers;
        PruneSystem => server, servers;
        RunSync => sync, syncs;
        CommitSync => sync, syncs;
        DeployStack => stack, stacks;
        DeployStackIfChanged => stack, stacks;
        PullStack => stack, stacks;
        StartStack => stack, stacks;
        RestartStack => stack, stacks;
        StopStack => stack, stacks;
        DestroyStack => stack, stacks;
      );
    }
  }
}
