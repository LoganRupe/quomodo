//! `[Install]` targets for a rootless agent. A user's systemd has no
//! `multi-user.target` or `graphical.target`, so a unit wanted by either is
//! never started at boot. Both become `default.target`, the user manager's
//! equivalent, so stack files work unchanged under either agent.

use std::collections::BTreeMap;

use komodo_client::entities::stack::QuadletKind;

use crate::unit::base_name;

const SYSTEM_TARGETS: [&str; 2] =
  ["multi-user.target", "graphical.target"];
const USER_TARGET: &str = "default.target";

/// Rewrite system targets in the `WantedBy=` / `RequiredBy=` / `UpheldBy=`
/// lines of every unit's `[Install]` section. Returns one line per file
/// changed, for the deploy log.
pub fn to_user_targets(
  write: &mut BTreeMap<String, String>,
) -> Vec<String> {
  let mut changed = Vec::new();
  for (path, contents) in write.iter_mut() {
    if QuadletKind::from_file_name(base_name(path)).is_none() {
      continue;
    }
    let (out, rewritten) = rewrite(contents);
    if !rewritten.is_empty() {
      *contents = out;
      changed.push(format!(
        "{path}: {} -> {USER_TARGET}",
        rewritten.join(", ")
      ));
    }
  }
  changed
}

fn rewrite(contents: &str) -> (String, Vec<&'static str>) {
  let mut out = String::with_capacity(contents.len());
  let mut rewritten = Vec::new();
  let mut in_install = false;
  for line in contents.split_inclusive('\n') {
    let trimmed = line.trim();
    if trimmed.starts_with('[') {
      in_install = trimmed == "[Install]";
    }
    let key = trimmed
      .split_once('=')
      .map(|(k, _)| k.trim())
      .filter(|k| ["WantedBy", "RequiredBy", "UpheldBy"].contains(k));
    let (Some(key), true) = (key, in_install) else {
      out.push_str(line);
      continue;
    };
    let value = trimmed.split_once('=').map(|(_, v)| v).unwrap_or("");
    let mut targets = Vec::new();
    for target in value.split_whitespace() {
      let target = match SYSTEM_TARGETS.iter().find(|t| **t == target)
      {
        Some(system) => {
          if !rewritten.contains(system) {
            rewritten.push(*system);
          }
          USER_TARGET
        }
        None => target,
      };
      if !targets.contains(&target) {
        targets.push(target);
      }
    }
    let indent = &line[..line.len() - line.trim_start().len()];
    out.push_str(&format!("{indent}{key}={}\n", targets.join(" ")));
    if !line.ends_with('\n') {
      out.pop();
    }
  }
  (out, rewritten)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn system_targets_become_default() {
    let mut write = BTreeMap::from([
      (
        "web.container".to_string(),
        "[Container]\nImage=x\n# WantedBy=multi-user.target\n[Install]\nWantedBy=multi-user.target\n".to_string(),
      ),
      (
        "app.pod".to_string(),
        "[Install]\nWantedBy=graphical.target multi-user.target other.target\nRequiredBy=default.target".to_string(),
      ),
      (
        "user.container".to_string(),
        "[Install]\nWantedBy=default.target\n".to_string(),
      ),
      (
        "notes.txt".to_string(),
        "[Install]\nWantedBy=multi-user.target\n".to_string(),
      ),
      (
        "odd.container".to_string(),
        "[Service]\nWantedBy=multi-user.target\n".to_string(),
      ),
    ]);
    let changed = to_user_targets(&mut write);
    assert_eq!(
      changed,
      vec![
        "app.pod: graphical.target, multi-user.target -> default.target",
        "web.container: multi-user.target -> default.target",
      ]
    );
    assert_eq!(
      write["web.container"],
      "[Container]\nImage=x\n# WantedBy=multi-user.target\n[Install]\nWantedBy=default.target\n"
    );
    assert_eq!(
      write["app.pod"],
      "[Install]\nWantedBy=default.target other.target\nRequiredBy=default.target"
    );
    assert_eq!(
      write["user.container"],
      "[Install]\nWantedBy=default.target\n"
    );
    assert_eq!(
      write["notes.txt"],
      "[Install]\nWantedBy=multi-user.target\n"
    );
    assert_eq!(
      write["odd.container"],
      "[Service]\nWantedBy=multi-user.target\n"
    );
  }
}
