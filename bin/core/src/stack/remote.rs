//! Read a Stack's files out of Core's own clone of its repo.

use std::path::{Path, PathBuf};

use anyhow::Context;
use formatting::format_serror;
use komodo_client::entities::{
  FileContents, RepoExecutionArgs,
  repo::Repo,
  stack::{QuadletUnit, Stack},
  update::Log,
};

use crate::{config::core_config, helpers::git_token};

/// Largest file read out of a stack directory.
const MAX_FILE_BYTES: u64 = 1024 * 1024;

#[derive(Default)]
pub struct RemoteStackContents {
  /// Every regular file under the run directory, relative paths with `/`.
  pub files: Vec<FileContents>,
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

  let (files, errors) = match read_stack_files(&run_directory) {
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
    errors,
    units,
    hash,
    message,
  })
}

/// Read every regular file under the run directory, skipping `.git`.
fn read_stack_files(
  run_directory: &Path,
) -> anyhow::Result<(Vec<FileContents>, Vec<FileContents>)> {
  let run_directory =
    run_directory.canonicalize().with_context(|| {
      format!(
        "Stack run directory does not exist in repo: {}",
        run_directory.display()
      )
    })?;
  let mut files = Vec::new();
  let mut errors = Vec::new();
  let mut dirs = vec![run_directory.clone()];
  while let Some(dir) = dirs.pop() {
    let entries = match std::fs::read_dir(&dir) {
      Ok(entries) => entries,
      Err(e) => {
        errors.push(FileContents {
          path: rel(&run_directory, &dir),
          contents: format!("Failed to read directory: {e}"),
        });
        continue;
      }
    };
    for entry in entries.flatten() {
      let path = entry.path();
      if entry.file_name() == ".git" {
        continue;
      }
      let Ok(meta) = entry.metadata() else { continue };
      if meta.is_dir() {
        dirs.push(path);
        continue;
      }
      if !meta.is_file() {
        continue;
      }
      let rel_path = rel(&run_directory, &path);
      if meta.len() > MAX_FILE_BYTES {
        errors.push(FileContents {
          path: rel_path,
          contents: format!(
            "File is larger than {MAX_FILE_BYTES} bytes, not read"
          ),
        });
        continue;
      }
      match std::fs::read_to_string(&path) {
        Ok(contents) => files.push(FileContents {
          path: rel_path,
          contents,
        }),
        Err(e) => errors.push(FileContents {
          path: rel_path,
          contents: format!("Failed to read file: {e}"),
        }),
      }
    }
  }
  files.sort_by(|a, b| a.path.cmp(&b.path));
  errors.sort_by(|a, b| a.path.cmp(&b.path));
  Ok((files, errors))
}

fn rel(root: &Path, path: &Path) -> String {
  path
    .strip_prefix(root)
    .unwrap_or(path)
    .components()
    .map(|c| c.as_os_str().to_string_lossy().to_string())
    .collect::<Vec<_>>()
    .join("/")
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
