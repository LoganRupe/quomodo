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

/// Fail if a unit of `stack` would generate the same systemd service as a
/// quadlet file already installed outside the stack's directory: by another
/// stack, or by hand. Two stacks shipping `newt.container` would otherwise
/// overwrite each other's `newt.service`.
pub async fn check_unit_names(
  stack: &str,
  units: &[ParsedUnit],
) -> anyhow::Result<()> {
  let root = periphery_config().quadlet_dir.clone();
  let own = path_name(stack);
  // As root, `users/` holds quadlets for every user's systemd, not ours.
  let skip_users = !crate::host::scope().is_rootless();
  let installed = tokio::task::spawn_blocking(move || {
    installed_elsewhere(&root, &own, skip_users)
  })
  .await
  .context("Quadlet directory reader panicked")?;
  let collisions = unit::name_collisions(
    units,
    installed.iter().map(|(o, f)| (o.as_str(), f.as_str())),
  );
  if collisions.is_empty() {
    return Ok(());
  }
  Err(anyhow!(
    "{}\nRename the unit files in this stack, or destroy the other stack first",
    collisions.join("\n")
  ))
}

/// How long a deploy waits for unit files it references from other
/// stacks. Stacks on one Server deploy in parallel, so a stack joining
/// another's network can start before that stack has written its
/// `.network` file; quadlet then generates no unit and the start fails.
const EXTERNAL_DEPS_WAIT: std::time::Duration =
  std::time::Duration::from_secs(300);

/// Wait for the unit files `units` reference but the stack does not have
/// (`Network=other.network`, ...) to be installed by another stack. Gives
/// up after EXTERNAL_DEPS_WAIT with a warning and lets the deploy go on:
/// the file may live in a quadlet directory the agent does not manage.
pub async fn wait_for_external_deps(
  stack: &str,
  units: &[ParsedUnit],
) -> Option<Log> {
  let wanted = unit::external_deps(units);
  if wanted.is_empty() {
    return None;
  }
  let root = periphery_config().quadlet_dir.clone();
  let own = path_name(stack);
  let skip_users = !crate::host::scope().is_rootless();
  let start = std::time::Instant::now();
  loop {
    let (root, own) = (root.clone(), own.clone());
    let installed = tokio::task::spawn_blocking(move || {
      installed_elsewhere(&root, &own, skip_users)
    })
    .await
    .unwrap_or_default();
    let missing = wanted
      .iter()
      .filter(|w| !installed.iter().any(|(_, f)| f == *w))
      .cloned()
      .collect::<Vec<_>>();
    if missing.is_empty() {
      let waited = start.elapsed().as_secs();
      return Some(Log::simple(
        "Other Stacks' Units",
        format!(
          "Installed by other stacks: {}{}",
          wanted.into_iter().collect::<Vec<_>>().join(", "),
          if waited > 0 {
            format!(" (waited {waited}s)")
          } else {
            String::new()
          }
        ),
      ));
    }
    if start.elapsed() >= EXTERNAL_DEPS_WAIT {
      return Some(Log::simple(
        "Other Stacks' Units",
        format!(
          "WARNING: no stack on this Server has installed {} after {}s. Deploying anyway; the units referencing them will not start until they exist",
          missing.join(", "),
          EXTERNAL_DEPS_WAIT.as_secs()
        ),
      ));
    }
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
  }
}

/// Every file under `root` outside `root/own`, as (where it is, file name).
/// Symlinks are not followed.
fn installed_elsewhere(
  root: &Path,
  own: &str,
  skip_users: bool,
) -> Vec<(String, String)> {
  let mut out = Vec::new();
  let Ok(entries) = std::fs::read_dir(root) else {
    return out;
  };
  for entry in entries.flatten() {
    let Ok(file_type) = entry.file_type() else {
      continue;
    };
    let name = entry.file_name().to_string_lossy().into_owned();
    if file_type.is_file() {
      out.push((format!("{}", root.display()), name));
      continue;
    }
    if !file_type.is_dir()
      || name == own
      || (skip_users && name == "users")
    {
      continue;
    }
    let owner = format!("stack directory '{name}'");
    let mut dirs = vec![entry.path()];
    while let Some(dir) = dirs.pop() {
      let Ok(entries) = std::fs::read_dir(&dir) else {
        continue;
      };
      for entry in entries.flatten() {
        match entry.file_type() {
          Ok(t) if t.is_dir() => dirs.push(entry.path()),
          Ok(t) if t.is_file() => out.push((
            owner.clone(),
            entry.file_name().to_string_lossy().into_owned(),
          )),
          _ => {}
        }
      }
    }
  }
  out
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
