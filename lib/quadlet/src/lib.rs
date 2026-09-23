//! Shared quadlet stack logic: unit file parsing and the rules for
//! interpolating variables and secrets into stack files.
//! Used by Core (to know which secrets a stack needs, and to list its
//! units) and by the agent (to prepare the files it installs).

use std::collections::BTreeMap;

use komodo_client::entities::{FileContents, stack::QuadletUnit};
use sha2::{Digest, Sha256};

pub mod prepare;
pub mod unit;

pub use unit::ParsedUnit;

/// Podman secret names allow `[a-zA-Z0-9_.-]`.
pub fn secret_safe(input: &str) -> String {
  input
    .chars()
    .map(|c| {
      if c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == '-' {
        c
      } else {
        '_'
      }
    })
    .collect()
}

/// The podman secret name prefix for a stack.
pub fn secret_prefix(stack: &str) -> String {
  secret_safe(stack.trim())
}

pub fn sha256_hex(input: &str) -> String {
  hex::encode(Sha256::digest(input.as_bytes()))
}

/// Hash over the raw stack files and the hashes of the resolved secret values.
/// Core stores this as `deployed_contents_hash`; the agent computes the same.
pub fn contents_hash(
  files: &[FileContents],
  secret_hashes: &BTreeMap<String, String>,
) -> String {
  let mut files = files.iter().collect::<Vec<_>>();
  files.sort_by(|a, b| a.path.cmp(&b.path));
  let mut hasher = Sha256::new();
  for file in files {
    hasher.update(file.path.as_bytes());
    hasher.update([0]);
    hasher.update(file.contents.as_bytes());
    hasher.update([0]);
  }
  for (name, hash) in secret_hashes {
    hasher.update(name.as_bytes());
    hasher.update([0]);
    hasher.update(hash.as_bytes());
    hasher.update([0]);
  }
  hex::encode(hasher.finalize())
}

/// Parse the quadlet units out of a file list, sorted by file name.
pub fn parse_units(files: &[FileContents]) -> Vec<ParsedUnit> {
  let mut units = files
    .iter()
    .filter_map(|f| unit::parse_unit(&f.path, &f.contents))
    .collect::<Vec<_>>();
  units.sort_by(|a, b| a.unit.file.cmp(&b.unit.file));
  units
}

pub fn units_only(units: &[ParsedUnit]) -> Vec<QuadletUnit> {
  units.iter().map(|u| u.unit.clone()).collect()
}
