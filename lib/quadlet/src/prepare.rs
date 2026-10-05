//! Interpolation of variables and secrets into stack files, and the
//! split between what gets written to the quadlet directory and what
//! becomes a podman secret.
//!
//! Rules:
//! - `[[NAME]]` where NAME is a Variable: replaced everywhere.
//! - `[[NAME]]` where NAME is a secret, inside a `Secret=` line of a unit
//!   file: replaced with the podman secret name `<stack>_NAME`, and the
//!   agent creates that secret.
//! - `[[NAME]]` where NAME is a secret, anywhere else in a unit file: error.
//! - `Secret=file:<path>` in a unit file: the file at <path> is interpolated
//!   in memory and becomes podman secret `<stack>_<path>`; it is not written
//!   to the quadlet directory.
//! - A config file containing a secret reference that no unit mounts with
//!   `Secret=file:<path>`: error.
//! - Anything else is written as is.
//! - Blobs (binary, or text over 1 MiB) are copied byte for byte and never
//!   interpolated, so one cannot be a `Secret=file:`.
//! - A `UserNS=` the agent cannot turn into a fixed map (see `idmap`) is an
//!   error here, so it fails on Core. The map itself is written by the
//!   agent, which holds the host's slice table.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use anyhow::anyhow;
use komodo_client::entities::{
  FileContents,
  stack::{QuadletKind, Stack},
};
use regex::Regex;

use crate::{
  Blob, ParsedUnit, idmap, secret_prefix, secret_safe, sha256_hex,
  unit::{base_name, parse_unit},
};

fn token_regex() -> &'static Regex {
  static RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
  RE.get_or_init(|| {
    Regex::new(r"\[\[\s*([A-Za-z0-9_.\-]+)\s*\]\]").unwrap()
  })
}

/// Names referenced as `[[NAME]]` in the given text.
pub fn referenced_names(text: &str) -> BTreeSet<String> {
  token_regex()
    .captures_iter(text)
    .map(|c| c[1].to_string())
    .collect()
}

fn replace_tokens(
  text: &str,
  mut lookup: impl FnMut(&str) -> Option<String>,
) -> String {
  token_regex()
    .replace_all(text, |caps: &regex::Captures| {
      lookup(&caps[1]).unwrap_or_else(|| caps[0].to_string())
    })
    .into_owned()
}

#[derive(Debug, Default)]
pub struct Prepared {
  /// Relative path -> contents to write under the quadlet dir.
  pub write: BTreeMap<String, String>,
  /// Relative path -> sha256 of a blob to copy verbatim from the run
  /// directory into the quadlet dir.
  pub blobs: BTreeMap<String, String>,
  /// Podman secret name -> value to create.
  pub secrets: BTreeMap<String, String>,
  /// Podman secret name -> sha256 of value.
  pub secret_hashes: BTreeMap<String, String>,
  /// Units parsed from the written unit files.
  pub units: Vec<ParsedUnit>,
}

pub fn prepare(
  stack: &Stack,
  files: &[FileContents],
  blobs: &[Blob],
  variables: &HashMap<String, String>,
  secrets: &HashMap<String, String>,
) -> anyhow::Result<Prepared> {
  let prefix = secret_prefix(&stack.name);
  let skip = stack.config.skip_secret_interp;

  let mut prepared = Prepared {
    blobs: blobs
      .iter()
      .map(|b| (b.path.clone(), b.sha256.clone()))
      .collect(),
    ..Default::default()
  };
  // Config files referenced with `Secret=file:<path>`, by relative path.
  let mut file_secret_refs = BTreeSet::<String>::new();
  // Config files (after variable interpolation) which still reference a secret.
  let mut secret_bearing = BTreeMap::<String, String>::new();
  let mut errors = Vec::<String>::new();

  // Pass 1: variables everywhere, then classify.
  for file in files {
    let is_unit =
      QuadletKind::from_file_name(base_name(&file.path)).is_some();
    let contents = if skip {
      file.contents.clone()
    } else {
      replace_tokens(&file.contents, |name| {
        variables.get(name).cloned()
      })
    };

    if skip {
      prepared.write.insert(file.path.clone(), contents);
      continue;
    }

    if is_unit {
      let mut out = String::with_capacity(contents.len());
      for (idx, line) in contents.split_inclusive('\n').enumerate() {
        let trimmed = line.trim();
        let is_secret_line = trimmed
          .split_once('=')
          .map(|(k, _)| k.trim() == "Secret")
          .unwrap_or(false)
          && !trimmed.starts_with('#')
          && !trimmed.starts_with(';');
        if is_secret_line {
          let (key, value) = trimmed.split_once('=').unwrap();
          let mut value = value.trim().to_string();
          // Secret=file:<path>,...
          if let Some(rest) = value.strip_prefix("file:") {
            let (path, opts) = match rest.split_once(',') {
              Some((p, o)) => (p.trim(), Some(o)),
              None => (rest.trim(), None),
            };
            let path = path.trim_start_matches("./").to_string();
            if prepared.blobs.contains_key(&path) {
              errors.push(format!(
                "{}:{}: Secret=file:{path} is binary or over 1 MiB, so it cannot be a secret file",
                file.path,
                idx + 1
              ));
            } else if !files.iter().any(|f| f.path == path) {
              errors.push(format!(
                "{}:{}: Secret=file:{path} does not match any file in the stack directory",
                file.path,
                idx + 1
              ));
            }
            let secret_name =
              format!("{prefix}_{}", secret_safe(&path));
            file_secret_refs.insert(path);
            value = match opts {
              Some(o) => format!("{secret_name},{o}"),
              None => secret_name,
            };
          }
          // [[NAME]] inside Secret= line -> podman secret name
          let missing = referenced_names(&value)
            .into_iter()
            .filter(|n| !secrets.contains_key(n))
            .collect::<Vec<_>>();
          if !missing.is_empty() {
            errors.push(format!(
              "{}:{}: unresolved reference [[{}]]",
              file.path,
              idx + 1,
              missing.join("]], [[")
            ));
          }
          let value = replace_tokens(&value, |name| {
            secrets.get(name).map(|v| {
              let secret_name =
                format!("{prefix}_{}", secret_safe(name));
              prepared.secrets.insert(secret_name.clone(), v.clone());
              secret_name
            })
          });
          let indent = &line[..line.len() - line.trim_start().len()];
          out.push_str(indent);
          out.push_str(key.trim());
          out.push('=');
          out.push_str(&value);
          if line.ends_with('\n') {
            out.push('\n');
          }
        } else {
          let names = referenced_names(line);
          if !names.is_empty() {
            let (secret, unknown): (Vec<_>, Vec<_>) = names
              .into_iter()
              .partition(|n| secrets.contains_key(n));
            if !secret.is_empty() {
              errors.push(format!(
                "{}:{}: secret [[{}]] may only be referenced on a Secret= line",
                file.path,
                idx + 1,
                secret.join("]], [[")
              ));
            }
            if !unknown.is_empty() {
              errors.push(format!(
                "{}:{}: unresolved reference [[{}]]",
                file.path,
                idx + 1,
                unknown.join("]], [[")
              ));
            }
          }
          out.push_str(line);
        }
      }
      prepared.write.insert(file.path.clone(), out);
    } else {
      let names = referenced_names(&contents);
      if names.is_empty() {
        prepared.write.insert(file.path.clone(), contents);
      } else {
        let unknown = names
          .iter()
          .filter(|n| !secrets.contains_key(*n))
          .cloned()
          .collect::<Vec<_>>();
        if !unknown.is_empty() {
          errors.push(format!(
            "{}: unresolved reference [[{}]]",
            file.path,
            unknown.join("]], [[")
          ));
        }
        secret_bearing.insert(file.path.clone(), contents);
      }
    }
  }

  errors.extend(idmap::check(&prepared.write));

  if skip {
    if !errors.is_empty() {
      return Err(anyhow!(
        "Stack files failed validation:\n  - {}",
        errors.join("\n  - ")
      ));
    }
    prepared.units = crate::parse_units(
      &prepared
        .write
        .iter()
        .map(|(p, c)| FileContents {
          path: p.clone(),
          contents: c.clone(),
        })
        .collect::<Vec<_>>(),
    );
    return Ok(prepared);
  }

  // Pass 2: files mounted as secrets.
  for path in &file_secret_refs {
    let contents = match secret_bearing.remove(path) {
      Some(c) => c,
      None => match prepared.write.remove(path) {
        Some(c) => c,
        None => continue, // already reported as missing
      },
    };
    let value =
      replace_tokens(&contents, |name| secrets.get(name).cloned());
    prepared
      .secrets
      .insert(format!("{prefix}_{}", secret_safe(path)), value);
  }
  for (path, _) in secret_bearing {
    errors.push(format!(
      "{path}: contains a secret reference but no unit mounts it with Secret=file:{path}"
    ));
  }

  if !errors.is_empty() {
    return Err(anyhow!(
      "Stack files failed secret validation:\n  - {}",
      errors.join("\n  - ")
    ));
  }

  prepared.secret_hashes = prepared
    .secrets
    .iter()
    .map(|(name, value)| (name.clone(), sha256_hex(value)))
    .collect();

  prepared.units = prepared
    .write
    .iter()
    .filter_map(|(path, contents)| parse_unit(path, contents))
    .collect();
  prepared.units.sort_by(|a, b| a.unit.file.cmp(&b.unit.file));

  Ok(prepared)
}

#[cfg(test)]
mod tests {
  use super::*;

  fn stack(skip: bool) -> Stack {
    let mut stack = Stack::default();
    stack.name = "my stack".to_string();
    stack.config.skip_secret_interp = skip;
    stack
  }

  fn files(list: &[(&str, &str)]) -> Vec<FileContents> {
    list
      .iter()
      .map(|(p, c)| FileContents {
        path: p.to_string(),
        contents: c.to_string(),
      })
      .collect()
  }

  fn maps() -> (HashMap<String, String>, HashMap<String, String>) {
    let vars = HashMap::from([(
      "DOMAIN".to_string(),
      "example.com".to_string(),
    )]);
    let secrets = HashMap::from([
      ("DB_PASSWORD".to_string(), "hunter2".to_string()),
      ("API_KEY".to_string(), "k-123".to_string()),
    ]);
    (vars, secrets)
  }

  #[test]
  fn variables_secrets_and_file_secrets() {
    let (vars, secrets) = maps();
    let files = files(&[
      (
        "web.container",
        "[Container]\nImage=x\nEnvironment=DOMAIN=[[DOMAIN]]\nSecret=[[DB_PASSWORD]],type=env,target=DB_PASSWORD\nSecret=file:config.yml,type=mount,target=/etc/app.yml\n",
      ),
      ("config.yml", "domain: [[DOMAIN]]\napi_key: [[API_KEY]]\n"),
      ("plain.txt", "hello [[DOMAIN]]\n"),
    ]);
    let p =
      prepare(&stack(false), &files, &[], &vars, &secrets).unwrap();
    let web = &p.write["web.container"];
    assert!(web.contains("Environment=DOMAIN=example.com"));
    assert!(web.contains(
      "Secret=my_stack_DB_PASSWORD,type=env,target=DB_PASSWORD"
    ));
    assert!(web.contains(
      "Secret=my_stack_config.yml,type=mount,target=/etc/app.yml"
    ));
    assert!(
      !web.contains("UserNS"),
      "the agent writes the map: {web}"
    );
    assert!(!p.write.contains_key("config.yml"));
    assert_eq!(p.write["plain.txt"], "hello example.com\n");
    assert_eq!(p.secrets["my_stack_DB_PASSWORD"], "hunter2");
    assert_eq!(
      p.secrets["my_stack_config.yml"],
      "domain: example.com\napi_key: k-123\n"
    );
    assert_eq!(p.units.len(), 1);
    assert_eq!(
      p.units[0].secrets,
      vec!["my_stack_DB_PASSWORD", "my_stack_config.yml"]
    );
    assert!(p.secret_hashes.contains_key("my_stack_DB_PASSWORD"));
  }

  #[test]
  fn secret_outside_secret_line_is_an_error() {
    let (vars, secrets) = maps();
    let files = files(&[(
      "web.container",
      "[Container]\nImage=x\nEnvironment=PW=[[DB_PASSWORD]]\n",
    )]);
    let err = prepare(&stack(false), &files, &[], &vars, &secrets)
      .unwrap_err()
      .to_string();
    assert!(err.contains("may only be referenced on a Secret= line"));
  }

  #[test]
  fn unmounted_secret_file_is_an_error() {
    let (vars, secrets) = maps();
    let files = files(&[
      ("web.container", "[Container]\nImage=x\n"),
      ("config.yml", "api_key: [[API_KEY]]\n"),
    ]);
    let err = prepare(&stack(false), &files, &[], &vars, &secrets)
      .unwrap_err()
      .to_string();
    assert!(
      err.contains("no unit mounts it with Secret=file:config.yml")
    );
  }

  #[test]
  fn unresolved_reference_is_an_error() {
    let (vars, secrets) = maps();
    let files = files(&[(
      "web.container",
      "[Container]\nImage=x\nEnvironment=A=[[NOPE]]\n",
    )]);
    let err = prepare(&stack(false), &files, &[], &vars, &secrets)
      .unwrap_err()
      .to_string();
    assert!(err.contains("unresolved reference [[NOPE]]"));
  }

  #[test]
  fn skip_interp_writes_verbatim() {
    let (vars, secrets) = maps();
    let files = files(&[(
      "web.container",
      "[Container]\nImage=x\nEnvironment=A=[[NOPE]]\n",
    )]);
    let p =
      prepare(&stack(true), &files, &[], &vars, &secrets).unwrap();
    assert_eq!(
      p.write["web.container"],
      "[Container]\nImage=x\nEnvironment=A=[[NOPE]]\n"
    );
    assert!(p.secrets.is_empty());
  }

  #[test]
  fn bad_userns_fails_on_core() {
    let (vars, secrets) = maps();
    let files = files(&[
      ("app.pod", "[Pod]\n"),
      (
        "web.container",
        "[Container]\nImage=x\nPod=app.pod\nUserNS=auto\n",
      ),
    ]);
    let err = prepare(&stack(false), &files, &[], &vars, &secrets)
      .unwrap_err()
      .to_string();
    assert!(
      err.contains("a pod member cannot use UserNS=auto"),
      "{err}"
    );
    let err = prepare(&stack(true), &files, &[], &vars, &secrets)
      .unwrap_err()
      .to_string();
    assert!(
      err.contains("a pod member cannot use UserNS=auto"),
      "{err}"
    );
  }

  #[test]
  fn blobs_pass_through_and_cannot_be_secret_files() {
    let (vars, secrets) = maps();
    let blobs = [Blob {
      path: "assets/logo.png".into(),
      sha256: "abc".into(),
    }];
    let ok = files(&[("web.container", "[Container]\nImage=x\n")]);
    let p =
      prepare(&stack(false), &ok, &blobs, &vars, &secrets).unwrap();
    assert_eq!(p.blobs["assets/logo.png"], "abc");
    assert!(!p.write.contains_key("assets/logo.png"));

    let bad = files(&[(
      "web.container",
      "[Container]\nImage=x\nSecret=file:assets/logo.png,type=mount\n",
    )]);
    let err = prepare(&stack(false), &bad, &blobs, &vars, &secrets)
      .unwrap_err()
      .to_string();
    assert!(err.contains("cannot be a secret file"), "{err}");
  }
}
