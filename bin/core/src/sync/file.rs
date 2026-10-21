use std::{
  collections::HashSet,
  fs,
  path::{Component, Path, PathBuf},
};

use anyhow::{Context, anyhow};
use formatting::{Color, bold, colored, format_serror, muted};
use komodo_client::entities::{
  sync::SyncFileContents,
  toml::{ResourceToml, ResourcesToml},
  update::Log,
};

/// Parses a sync resource or file path into one that stays under
/// the sync root. A leading `/` means the sync root, so `/` reads the
/// whole sync and `/stacks` reads `stacks`. `..` is rejected.
pub fn sync_relative_path(path: &str) -> anyhow::Result<PathBuf> {
  let mut res = PathBuf::new();
  for component in Path::new(path).components() {
    match component {
      Component::Normal(part) => res.push(part),
      Component::RootDir | Component::CurDir => {}
      Component::ParentDir | Component::Prefix(_) => {
        return Err(anyhow!(
          "Path {path:?} must stay inside the sync directory"
        ));
      }
    }
  }
  Ok(res)
}

/// Errors if `path`, a path under `root`, leaves `root` once symlinks
/// are followed. Checks the deepest part of `path` that exists, so it
/// also covers a file that is about to be created. A dangling symlink
/// fails, since writing through it would create its target.
pub fn ensure_inside_root(
  root: &Path,
  path: &Path,
) -> anyhow::Result<()> {
  // Nothing under a root that doesn't exist yet can be a symlink.
  let Ok(root) = root.canonicalize() else {
    return Ok(());
  };
  let Some(existing) =
    path.ancestors().find(|p| p.symlink_metadata().is_ok())
  else {
    return Ok(());
  };
  let resolved = existing
    .canonicalize()
    .with_context(|| format!("Failed to resolve {existing:?}"))?;
  if resolved.starts_with(&root) {
    Ok(())
  } else {
    Err(anyhow!("{path:?} points outside the sync directory"))
  }
}

pub fn read_resources(
  root_path: &Path,
  resource_path: &[String],
  match_tags: &[String],
  logs: &mut Vec<Log>,
  files: &mut Vec<SyncFileContents>,
  file_errors: &mut Vec<SyncFileContents>,
) -> anyhow::Result<ResourcesToml> {
  let mut resources = ResourcesToml::default();

  for resource_path in resource_path {
    let resource_path = sync_relative_path(resource_path)?;
    let full_path = root_path.join(&resource_path);

    let mut log = format!(
      "{}: reading resources from {full_path:?}",
      muted("INFO")
    );

    if let Err(e) = ensure_inside_root(root_path, &full_path) {
      file_errors.push(SyncFileContents {
        resource_path: String::new(),
        path: resource_path.display().to_string(),
        contents: format_serror(&e.into()),
      });
      logs.push(Log::error("Read remote resources", log));
      continue;
    }

    if full_path.is_file() {
      if let Err(e) = read_resource_file(
        root_path,
        None,
        &resource_path,
        match_tags,
        &mut resources,
        &mut log,
        files,
      )
      .with_context(|| {
        format!("failed to read resources from {full_path:?}")
      }) {
        file_errors.push(SyncFileContents {
          resource_path: String::new(),
          path: resource_path.display().to_string(),
          contents: format_serror(&e.into()),
        });
        logs.push(Log::error("Read remote resources", log));
      } else {
        logs.push(Log::simple("Read remote resources", log));
      };
    } else if full_path.is_dir() {
      if let Err(e) = read_resources_directory(
        root_path,
        &resource_path,
        &PathBuf::new(),
        match_tags,
        &mut resources,
        &mut log,
        files,
        file_errors,
        &mut HashSet::new(),
      )
      .with_context(|| {
        format!("Failed to read resources from {full_path:?}")
      }) {
        file_errors.push(SyncFileContents {
          resource_path: String::new(),
          path: resource_path.display().to_string(),
          contents: format_serror(&e.into()),
        });
        logs.push(Log::error("Read remote resources", log));
      } else {
        logs.push(Log::simple("Read remote resources", log));
      };
    } else if !full_path.exists() {
      file_errors.push(SyncFileContents {
        resource_path: String::new(),
        path: resource_path.display().to_string(),
        contents: format_serror(
          &anyhow!("Initialize the file to proceed.")
            .context(format!("Path {full_path:?} does not exist."))
            .into(),
        ),
      });
      log.push_str(&format!(
        "{}: Resoure path {} does not exist.",
        colored("ERROR", Color::Red),
        bold(resource_path.display())
      ));
      logs.push(Log::error("Read remote resources", log));
    } else {
      log.push_str(&format!(
        "{}: Resoure path {} exists, but is neither a file nor a directory.",
        colored("WARN", Color::Red),
        bold(resource_path.display())
      ));
      logs.push(Log::error("Read remote resources", log));
    }
  }

  Ok(resources)
}

/// Use when incoming resource path is a file.
fn read_resource_file(
  root_path: &Path,
  // relative to root path.
  resource_path: Option<&Path>,
  // relative to resource path if provided, or root path.
  file_path: &Path,
  match_tags: &[String],
  resources: &mut ResourcesToml,
  log: &mut String,
  files: &mut Vec<SyncFileContents>,
) -> anyhow::Result<()> {
  let full_path = if let Some(resource_path) = resource_path {
    root_path.join(resource_path).join(file_path)
  } else {
    root_path.join(file_path)
  };
  if !full_path
    .extension()
    .map(|ext| ext == "toml")
    .unwrap_or_default()
  {
    return Ok(());
  }
  let contents = std::fs::read_to_string(&full_path)
    .context("failed to read file contents")?;

  files.push(SyncFileContents {
    resource_path: resource_path
      .map(|path| path.display().to_string())
      .unwrap_or_default(),
    path: file_path.display().to_string(),
    contents: contents.clone(),
  });
  let more = super::deserialize_resources_toml(&contents)
    .context("failed to parse resource file contents")?;
  log.push('\n');
  let path_for_view =
    if let Some(resource_path) = resource_path.as_ref() {
      resource_path.join(file_path)
    } else {
      file_path.to_path_buf()
    };
  log.push_str(&format!(
    "{}: {} from {}",
    muted("INFO"),
    colored("adding resources", Color::Green),
    colored(path_for_view.display(), Color::Blue)
  ));

  extend_resources(resources, more, match_tags);

  Ok(())
}

/// Reads down into directories.
#[allow(clippy::too_many_arguments)]
fn read_resources_directory(
  root_path: &Path,
  // relative to root path.
  resource_path: &Path,
  // relative to resource path. start as empty path
  curr_path: &Path,
  match_tags: &[String],
  resources: &mut ResourcesToml,
  log: &mut String,
  files: &mut Vec<SyncFileContents>,
  file_errors: &mut Vec<SyncFileContents>,
  // Canonical paths of the directories already read.
  visited: &mut HashSet<PathBuf>,
) -> anyhow::Result<()> {
  let full_resource_path = root_path.join(resource_path);
  let full_path = full_resource_path.join(curr_path);
  // A symlink back to a directory already read would loop.
  if !visited.insert(full_path.canonicalize()?) {
    log.push('\n');
    log.push_str(&format!(
      "{}: skipping {}, already read",
      colored("WARN", Color::Red),
      colored(full_path.display(), Color::Blue)
    ));
    return Ok(());
  }
  let directory = fs::read_dir(&full_path).with_context(|| {
    format!("Failed to read directory contents at {full_path:?}")
  })?;
  for entry in directory.into_iter().flatten() {
    let path = entry.path();
    let curr_path =
      path.strip_prefix(&full_resource_path).unwrap_or(&path);
    // A symlink is followed if it stays inside the sync root.
    if entry.file_type().is_ok_and(|t| t.is_symlink())
      && let Err(e) = ensure_inside_root(root_path, &path)
    {
      file_errors.push(SyncFileContents {
        resource_path: resource_path.display().to_string(),
        path: curr_path.display().to_string(),
        contents: format_serror(&e.into()),
      });
      continue;
    }
    if path.is_file() {
      if let Err(e) = read_resource_file(
        root_path,
        Some(resource_path),
        curr_path,
        match_tags,
        resources,
        log,
        files,
      )
      .with_context(|| {
        format!("failed to read resources from {full_path:?}")
      }) {
        file_errors.push(SyncFileContents {
          resource_path: String::new(),
          path: resource_path.display().to_string(),
          contents: format_serror(&e.into()),
        });
      };
    } else if path.is_dir()
      && let Err(e) = read_resources_directory(
        root_path,
        resource_path,
        curr_path,
        match_tags,
        resources,
        log,
        files,
        file_errors,
        visited,
      )
      .with_context(|| {
        format!("failed to read resources from {path:?}")
      })
    {
      file_errors.push(SyncFileContents {
        resource_path: resource_path.display().to_string(),
        path: curr_path.display().to_string(),
        contents: format_serror(&e.into()),
      });
      log.push('\n');
      log.push_str(&format!(
        "{}: {} from {}",
        colored("ERROR", Color::Red),
        colored("adding resources", Color::Green),
        colored(path.display(), Color::Blue)
      ));
    }
  }
  Ok(())
}

pub fn extend_resources(
  resources: &mut ResourcesToml,
  more: ResourcesToml,
  match_tags: &[String],
) {
  macro_rules! extend_filtered {
    ($($field:ident),* $(,)?) => {
      $(
        resources.$field.extend(filter_by_tag(more.$field, match_tags));
      )*
    };
  }
  // New resource types need to be added here manually.
  extend_filtered!(
    servers,
    stacks,
    repos,
    procedures,
    actions,
    alerters,
    resource_syncs,
  );
  resources.user_groups.extend(more.user_groups);
}

fn filter_by_tag<T: Default>(
  resources: Vec<ResourceToml<T>>,
  match_tags: &[String],
) -> Vec<ResourceToml<T>> {
  resources
    .into_iter()
    .filter(|resource| {
      match_tags.iter().all(|tag| resource.tags.contains(tag))
    })
    .collect()
}

#[cfg(test)]
mod tests {
  use komodo_client::entities::toml::ResourcesToml;

  use super::extend_resources;

  /// One of every resource type a sync file can declare. A new type
  /// belongs here too, so `extend_resources` can't silently drop it.
  const EVERY_TYPE: &str = r#"
[[server]]
name = "server"

[[stack]]
name = "stack"

[[repo]]
name = "repo"

[[procedure]]
name = "procedure"

[[action]]
name = "action"

[[alerter]]
name = "alerter"
[alerter.config]
endpoint.type = "Custom"
endpoint.params.url = "http://localhost:7000"

[[resource_sync]]
name = "resource_sync"

[[user_group]]
name = "user_group"
"#;

  /// A fresh directory holding `root/` and `outside/`.
  fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir()
      .join(format!("quomodo-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("root/stacks")).unwrap();
    std::fs::create_dir_all(dir.join("root/shared")).unwrap();
    std::fs::create_dir_all(dir.join("outside")).unwrap();
    dir
  }

  #[test]
  fn ensure_inside_root_follows_symlinks() {
    use std::os::unix::fs::symlink;
    let dir = scratch("inside-root");
    let root = dir.join("root");
    std::fs::write(root.join("shared/a.toml"), "").unwrap();
    std::fs::write(dir.join("outside/b.toml"), "").unwrap();
    symlink("../shared/a.toml", root.join("stacks/in.toml")).unwrap();
    symlink(dir.join("outside/b.toml"), root.join("stacks/out.toml"))
      .unwrap();
    symlink(
      dir.join("outside/none.toml"),
      root.join("dangling.toml"),
    )
    .unwrap();
    symlink(dir.join("outside"), root.join("outdir")).unwrap();

    let check =
      |path: &str| super::ensure_inside_root(&root, &root.join(path));
    for ok in [
      "shared/a.toml",
      "stacks/in.toml",
      "stacks/new.toml",
      "new/dir/x.toml",
    ] {
      assert!(check(ok).is_ok(), "{ok}");
    }
    for bad in [
      "stacks/out.toml",
      "dangling.toml",
      "outdir/new.toml",
      "outdir",
    ] {
      assert!(check(bad).is_err(), "{bad}");
    }
    // A root that doesn't exist yet holds no symlinks.
    assert!(
      super::ensure_inside_root(
        &dir.join("none"),
        &dir.join("none/x.toml")
      )
      .is_ok()
    );
    std::fs::remove_dir_all(&dir).unwrap();
  }

  #[test]
  fn read_resources_skips_symlinks_out_of_root() {
    use std::os::unix::fs::symlink;
    let dir = scratch("read-symlinks");
    let root = dir.join("root");
    let procedure =
      |name: &str| format!("[[procedure]]\nname = \"{name}\"\n");
    std::fs::write(root.join("stacks/real.toml"), procedure("real"))
      .unwrap();
    std::fs::write(
      root.join("shared/linked.toml"),
      procedure("linked"),
    )
    .unwrap();
    std::fs::write(
      dir.join("outside/secret.toml"),
      procedure("secret"),
    )
    .unwrap();
    symlink("../shared/linked.toml", root.join("stacks/linked.toml"))
      .unwrap();
    symlink(
      dir.join("outside/secret.toml"),
      root.join("stacks/secret.toml"),
    )
    .unwrap();
    std::fs::create_dir_all(root.join("lib")).unwrap();
    std::fs::write(root.join("lib/viadir.toml"), procedure("viadir"))
      .unwrap();
    symlink("../lib", root.join("stacks/common")).unwrap();
    // Loops back to stacks, which is read once.
    symlink(".", root.join("stacks/loop")).unwrap();
    symlink(dir.join("outside"), root.join("escape")).unwrap();

    let (mut logs, mut files, mut file_errors) =
      (Vec::new(), Vec::new(), Vec::new());
    let resources = super::read_resources(
      &root,
      &["stacks".to_string(), "escape".to_string()],
      &[],
      &mut logs,
      &mut files,
      &mut file_errors,
    )
    .unwrap();
    let mut names = resources
      .procedures
      .iter()
      .map(|p| p.name.as_str())
      .collect::<Vec<_>>();
    names.sort();
    assert_eq!(names, ["linked", "real", "viadir"]);
    assert!(files.iter().all(|f| !f.contents.contains("secret")));
    let errors = file_errors
      .iter()
      .map(|e| e.path.as_str())
      .collect::<Vec<_>>();
    assert!(errors.contains(&"secret.toml"), "{errors:?}");
    assert!(errors.contains(&"escape"), "{errors:?}");
    std::fs::remove_dir_all(&dir).unwrap();
  }

  #[test]
  fn sync_relative_path_stays_in_root() {
    use std::path::PathBuf;
    for (input, expected) in [
      ("", ""),
      (".", ""),
      ("/", ""),
      ("//", ""),
      ("/stacks", "stacks"),
      ("/./stacks", "stacks"),
      ("stacks/all.toml", "stacks/all.toml"),
      ("/resources.toml", "resources.toml"),
    ] {
      assert_eq!(
        super::sync_relative_path(input).unwrap(),
        PathBuf::from(expected),
        "{input}"
      );
    }
    for input in ["..", "../etc/passwd", "/stacks/../../etc", "a/.."]
    {
      assert!(super::sync_relative_path(input).is_err(), "{input}");
    }
  }

  #[test]
  fn extend_resources_keeps_every_resource_type() {
    let parsed = super::super::deserialize_resources_toml(EVERY_TYPE)
      .expect("parse sync file");
    let expected = serde_json::to_value(&parsed).unwrap();

    let mut merged = ResourcesToml::default();
    extend_resources(&mut merged, parsed, &[]);

    assert_eq!(serde_json::to_value(&merged).unwrap(), expected);
    assert_eq!(merged.alerters.len(), 1);
  }
}
