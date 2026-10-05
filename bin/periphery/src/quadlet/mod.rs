//! Quadlet stacks: a git directory of unit files installed under
//! `${quadlet_dir}/<stack>/` and driven with systemctl.

use std::path::{Path, PathBuf};

use anyhow::{Context, anyhow};
use formatting::format_serror;
use komodo_client::entities::{
  RepoExecutionArgs, repo::Repo, stack::Stack,
  to_path_compatible_name, update::Log,
};
use mogh_resolver::Resolve as _;
use periphery_client::api::git::{CloneRepo, PullOrCloneRepo};
pub use quadlet::{
  ParsedUnit, StackDir, contents_hash, parse_units, read_stack_dir,
  secret_prefix, sha256_hex, unit, units_only,
};

use crate::{api::Args, config::periphery_config};

pub mod deploy;
pub mod idmap;
pub mod state;

/// Where stacks with their own (non linked) repo are cloned.
pub fn stack_clone_root() -> PathBuf {
  periphery_config().root_directory.join("stacks")
}

/// The directory a stack is installed into.
pub fn stack_quadlet_dir(stack: &str) -> PathBuf {
  periphery_config().quadlet_dir.join(path_name(stack))
}

/// Stack name as a directory name.
pub fn path_name(stack: &str) -> String {
  to_path_compatible_name(stack).replace('/', "_")
}

/// Pull or clone the stack repo (linked or inline).
/// Returns the run directory holding the quadlet files.
#[instrument(
  "PullOrCloneStack",
  skip_all,
  fields(
    stack = stack.name,
    repo = repo.as_ref().map(|repo| &repo.name),
  )
)]
pub async fn pull_or_clone_stack(
  stack: &Stack,
  repo: Option<&Repo>,
  git_token: Option<String>,
  logs: &mut Vec<Log>,
  req_args: &Args,
) -> anyhow::Result<PulledStack> {
  if repo.is_none() && stack.config.repo.is_empty() {
    return Err(anyhow!(
      "Stack has no repo configured. Attach a linked Repo or set repo / branch."
    ));
  }

  let (root, mut args, environment, env_file_path, on_clone, on_pull) =
    if let Some(repo) = repo {
      let root = periphery_config()
        .repo_dir()
        .join(to_path_compatible_name(&repo.name))
        .join(&repo.config.path)
        .components()
        .collect::<PathBuf>();
      let args: RepoExecutionArgs = repo.into();
      let env_file_path = root
        .join(&repo.config.env_file_path)
        .components()
        .collect::<PathBuf>()
        .display()
        .to_string();
      let on_clone = (!repo.config.on_clone.is_none())
        .then_some(repo.config.on_clone.clone());
      let on_pull = (!repo.config.on_pull.is_none())
        .then_some(repo.config.on_pull.clone());
      (
        root,
        args,
        repo.config.env_vars()?,
        env_file_path,
        on_clone,
        on_pull,
      )
    } else {
      let root = stack_clone_root()
        .join(to_path_compatible_name(&stack.name))
        .join(&stack.config.clone_path)
        .components()
        .collect::<PathBuf>();
      let args: RepoExecutionArgs = stack.into();
      (root, args, Vec::new(), String::new(), None, None)
    };
  args.destination = Some(root.display().to_string());

  let git_token = crate::helpers::git_token(git_token, &args)
    .map_err(|e| {
      let error = format_serror(&e.into());
      logs.push(Log::error("Missing git token", error));
      anyhow!("failed to find required git token, stopping run")
    })?;

  let res = if stack.config.reclone {
    CloneRepo {
      args,
      git_token,
      environment,
      env_file_path,
      on_clone,
      on_pull,
      skip_secret_interp: true,
      replacers: Default::default(),
    }
    .resolve(req_args)
    .await?
  } else {
    PullOrCloneRepo {
      args,
      git_token,
      environment,
      env_file_path,
      on_clone,
      on_pull,
      skip_secret_interp: true,
      replacers: Default::default(),
    }
    .resolve(req_args)
    .await?
  };

  logs.extend(res.res.logs);

  let run_directory = root
    .join(&stack.config.run_directory)
    .components()
    .collect::<PathBuf>();

  Ok(PulledStack {
    run_directory,
    commit_hash: res.res.commit_hash,
    commit_message: res.res.commit_message,
  })
}

pub struct PulledStack {
  pub run_directory: PathBuf,
  pub commit_hash: Option<String>,
  pub commit_message: Option<String>,
}

/// Read the run directory off the async runtime. See `quadlet::read`.
pub async fn read_stack_files(
  run_directory: &Path,
) -> anyhow::Result<StackDir> {
  let run_directory = run_directory.to_path_buf();
  tokio::task::spawn_blocking(move || read_stack_dir(&run_directory))
    .await
    .context("Stack directory reader panicked")?
}

/// The units currently installed for a stack, read from the quadlet dir.
pub async fn installed_units(
  stack: &str,
) -> anyhow::Result<Vec<ParsedUnit>> {
  let dir = stack_quadlet_dir(stack);
  if !dir.is_dir() {
    return Ok(Vec::new());
  }
  Ok(parse_units(&read_stack_files(&dir).await?.files))
}

/// Resolve the requested unit files (eg `web.container`) against the units,
/// accepting the file name, its stem, or its service name.
pub fn select_units<'a>(
  units: &'a [ParsedUnit],
  services: &[String],
) -> anyhow::Result<Vec<&'a ParsedUnit>> {
  if services.is_empty() {
    return Ok(units.iter().collect());
  }
  let mut out = Vec::new();
  for service in services {
    let found = units.iter().find(|u| {
      u.unit.file == *service
        || unit::base_name(&u.unit.file) == service
        || unit::stem(unit::base_name(&u.unit.file)) == service
        || u.unit.service == *service
    });
    match found {
      Some(u) => out.push(u),
      None => {
        return Err(anyhow!(
          "No unit named '{service}' in this stack"
        ));
      }
    }
  }
  Ok(out)
}
