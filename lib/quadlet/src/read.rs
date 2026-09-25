//! Reading a stack directory. Core reads its own clone to list units and
//! resolve secrets; the agent reads the run directory to deploy it. Both
//! must classify files the same way, or their contents hashes disagree.

use std::{
  fs::File,
  io::{self, Read as _},
  path::Path,
};

use anyhow::Context;
use komodo_client::entities::FileContents;
use sha2::{Digest, Sha256};

/// Text files up to this size are read, interpolated and shown in the UI.
/// Anything larger, or not UTF-8, is a blob.
pub const MAX_TEXT_BYTES: u64 = 1024 * 1024;

/// A file copied into the quadlet directory byte for byte: binary, or text
/// over `MAX_TEXT_BYTES`. Never interpolated, never shown in the UI.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Blob {
  /// Relative to the run directory, with `/` separators.
  pub path: String,
  pub sha256: String,
}

#[derive(Debug, Default)]
pub struct StackDir {
  /// UTF-8 files up to `MAX_TEXT_BYTES`, sorted by path.
  pub files: Vec<FileContents>,
  /// Every other regular file, sorted by path.
  pub blobs: Vec<Blob>,
  /// Files and directories which could not be read, with the error
  /// as contents.
  pub errors: Vec<FileContents>,
}

/// Read every regular file under the run directory, recursively,
/// skipping `.git`. Blocking; the agent calls it via `spawn_blocking`.
pub fn read_stack_dir(
  run_directory: &Path,
) -> anyhow::Result<StackDir> {
  let run_directory =
    run_directory.canonicalize().with_context(|| {
      format!(
        "Stack run directory does not exist: {}",
        run_directory.display()
      )
    })?;
  let mut out = StackDir::default();
  let mut dirs = vec![run_directory.clone()];
  while let Some(dir) = dirs.pop() {
    let entries = match std::fs::read_dir(&dir) {
      Ok(entries) => entries,
      Err(e) => {
        out.errors.push(FileContents {
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
      // Does not follow symlinks, so a link out of the repo is skipped.
      let Ok(file_type) = entry.file_type() else {
        continue;
      };
      if file_type.is_dir() {
        dirs.push(path);
        continue;
      }
      if !file_type.is_file() {
        continue;
      }
      let rel_path = rel(&run_directory, &path);
      match read_file(&path) {
        Ok(Read::Text(contents)) => out.files.push(FileContents {
          path: rel_path,
          contents,
        }),
        Ok(Read::Blob(sha256)) => out.blobs.push(Blob {
          path: rel_path,
          sha256,
        }),
        Err(e) => out.errors.push(FileContents {
          path: rel_path,
          contents: format!("Failed to read file: {e}"),
        }),
      }
    }
  }
  out.files.sort_by(|a, b| a.path.cmp(&b.path));
  out.blobs.sort_by(|a, b| a.path.cmp(&b.path));
  out.errors.sort_by(|a, b| a.path.cmp(&b.path));
  Ok(out)
}

enum Read {
  Text(String),
  Blob(String),
}

fn read_file(path: &Path) -> io::Result<Read> {
  if std::fs::metadata(path)?.len() > MAX_TEXT_BYTES {
    return sha256_file(path).map(Read::Blob);
  }
  let bytes = std::fs::read(path)?;
  match String::from_utf8(bytes) {
    Ok(text) => Ok(Read::Text(text)),
    Err(e) => {
      Ok(Read::Blob(hex::encode(Sha256::digest(e.as_bytes()))))
    }
  }
}

/// sha256 of a file's bytes, streamed.
pub fn sha256_file(path: &Path) -> io::Result<String> {
  let mut file = File::open(path)?;
  let mut hasher = Sha256::new();
  let mut buf = vec![0; 64 * 1024];
  loop {
    match file.read(&mut buf)? {
      0 => break,
      n => hasher.update(&buf[..n]),
    }
  }
  Ok(hex::encode(hasher.finalize()))
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

/// A fresh empty directory under the system temp dir, for tests.
#[cfg(test)]
pub(crate) fn test_dir(name: &str) -> std::path::PathBuf {
  let dir = std::env::temp_dir()
    .join(format!("quadlet-{name}-{}", std::process::id()));
  let _ = std::fs::remove_dir_all(&dir);
  std::fs::create_dir_all(&dir).unwrap();
  dir
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn splits_text_and_blobs() {
    let dir = test_dir("read");
    std::fs::create_dir_all(dir.join("assets")).unwrap();
    std::fs::create_dir_all(dir.join(".git")).unwrap();
    std::fs::write(dir.join("web.container"), "[Container]\n")
      .unwrap();
    std::fs::write(dir.join("assets/logo.png"), [0x89, b'P', 0xff])
      .unwrap();
    std::fs::write(
      dir.join("assets/big.txt"),
      "a".repeat(MAX_TEXT_BYTES as usize + 1),
    )
    .unwrap();
    std::fs::write(dir.join(".git/HEAD"), "ref").unwrap();

    let read = read_stack_dir(&dir).unwrap();
    assert_eq!(
      read
        .files
        .iter()
        .map(|f| f.path.as_str())
        .collect::<Vec<_>>(),
      vec!["web.container"]
    );
    assert_eq!(
      read
        .blobs
        .iter()
        .map(|b| b.path.as_str())
        .collect::<Vec<_>>(),
      vec!["assets/big.txt", "assets/logo.png"]
    );
    assert!(read.errors.is_empty());
    assert_eq!(
      read.blobs[1].sha256,
      hex::encode(Sha256::digest([0x89, b'P', 0xff]))
    );
    assert_eq!(
      read.blobs[1].sha256,
      sha256_file(&dir.join("assets/logo.png")).unwrap()
    );
    std::fs::remove_dir_all(&dir).unwrap();
  }
}
