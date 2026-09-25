//! Read a Stack's files out of Core's own clone of its repo.

use std::path::PathBuf;

use anyhow::Context;
use formatting::format_serror;
use komodo_client::entities::{
  FileContents, RepoExecutionArgs,
  repo::Repo,
  stack::{QuadletUnit, Stack},
  update::Log,
};

use crate::{config::core_config, helpers::git_token};

#[derive(Default)]
pub struct RemoteStackContents {
  /// Text files under the run directory, relative paths with `/`.
  pub files: Vec<FileContents>,
  /// Binary or large files, hashed but not read into memory.
  pub blobs: Vec<quadlet::Blob>,
  /// Files which could not be read, with the error as contents.
  pub errors: Vec<FileContents>,
  /// The quadlet units parsed from `files`.
  pub units: Vec<QuadletUnit>,
  pub hash: Option<String>,
  pub message: Option<String>,
}

pub async fn get_repo_stack_contents(
  stack: &Stack,
  repo: Option<&Repo>,
) -> anyhow::Result<RemoteStackContents> {
  let clone_args: RepoExecutionArgs =
    repo.map(Into::into).unwrap_or(stack.into());
  let (repo_path, logs, hash, message) =
    ensure_remote_repo(clone_args)
      .await
      .context("Failed to clone stack repo")?;

  // Ensure clone / pull successful,
  // propogate error log -> 'errors' and return.
  if let Some(failure) = logs.iter().find(|log| !log.success) {
    return Ok(RemoteStackContents {
      errors: vec![FileContents {
        path: format!("Failed at: {}", failure.stage),
        contents: failure.combined(),
      }],
      ..Default::default()
    });
  }

  let run_directory = repo_path
    .join(&stack.config.run_directory)
    .components()
    .collect::<PathBuf>();

  let quadlet::StackDir {
    files,
    blobs,
    errors,
  } = match quadlet::read_stack_dir(&run_directory) {
    Ok(res) => res,
    Err(e) => {
      return Ok(RemoteStackContents {
        errors: vec![FileContents {
          path: stack.config.run_directory.clone(),
          contents: format_serror(&e.into()),
        }],
        hash,
        message,
        ..Default::default()
      });
    }
  };

  let units = quadlet::units_only(&quadlet::parse_units(&files));

  Ok(RemoteStackContents {
    files,
    blobs,
    errors,
    units,
    hash,
    message,
  })
}

/// Returns (destination, logs, hash, message)
pub async fn ensure_remote_repo(
  mut clone_args: RepoExecutionArgs,
) -> anyhow::Result<(PathBuf, Vec<Log>, Option<String>, Option<String>)>
{
  let config = core_config();

  let access_token = if let Some(username) = &clone_args.account {
    git_token(&clone_args.provider, username, |https| {
        clone_args.https = https
      })
      .await
      .with_context(
        || format!("Failed to get git token in call to db. Stopping run. | {} | {username}", clone_args.provider),
      )?
  } else {
    None
  };

  let repo_path =
    clone_args.unique_path(&core_config().repo_directory)?;
  clone_args.destination = Some(repo_path.display().to_string());

  git::pull_or_clone(clone_args, &config.repo_directory, access_token)
    .await
    .context("Failed to clone stack repo")
    .map(|(res, _)| {
      (repo_path, res.logs, res.commit_hash, res.commit_message)
    })
}
