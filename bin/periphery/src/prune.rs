//! Image pruning that keeps the images installed quadlet units use.
//!
//! `podman image prune -a` removes every image no container uses. Quadlet
//! runs containers with `--rm`, so a stopped unit, or a one-shot that a
//! timer starts once a day, has no container between runs, and its image
//! went with the prune. The prune here keeps every image a unit under the
//! quadlet directory names.

use std::{
  collections::{BTreeMap, BTreeSet},
  path::Path,
  time::Duration,
};

use command::{CommandOptions, run_standard_command};
use komodo_client::entities::{
  komodo_timestamp, stack::QuadletKind, update::Log,
};
use quadlet::{ParsedUnit, unit::parse_unit};
use serde::Deserialize;
use shell_escape::unix::escape;

use crate::config::periphery_config;

/// Remove every image that no container uses and no installed unit names.
pub async fn prune_images(stage: &str) -> Log {
  let start_ts = komodo_timestamp();
  let mut stdout = Vec::new();
  let mut stderr = Vec::new();
  let success = prune(&mut stdout, &mut stderr).await;
  Log {
    stage: stage.to_string(),
    command: String::from(
      "podman rmi: images no container uses and no installed unit names",
    ),
    stdout: stdout.join("\n"),
    stderr: stderr.join("\n"),
    success,
    start_ts,
    end_ts: komodo_timestamp(),
  }
}

async fn prune(
  stdout: &mut Vec<String>,
  stderr: &mut Vec<String>,
) -> bool {
  let root = periphery_config().quadlet_dir.clone();
  // As root, `users/` holds quadlets for every user's systemd, whose
  // images live in that user's storage, not ours.
  let skip_users = !crate::host::scope().is_rootless();
  let units = match tokio::task::spawn_blocking(move || {
    installed_units(&root, skip_users)
  })
  .await
  {
    Ok(units) => units,
    Err(e) => {
      stderr.push(format!("Quadlet directory reader panicked: {e}"));
      return false;
    }
  };

  // Resolve each reference through podman, which knows how short names
  // and default tags expand, to the id of the image it names here.
  let mut keep = BTreeMap::<String, Vec<String>>::new();
  for image in image_refs(&units) {
    let output = run_standard_command(
      &format!(
        "podman image inspect --format '{{{{.Id}}}}' -- {}",
        escape(image.as_str().into())
      ),
      CommandOptions::default().timeout(Duration::from_secs(20)),
    )
    .await;
    let id = output.stdout.trim();
    // Not pulled or built yet: nothing to keep.
    if output.success() && !id.is_empty() {
      keep.entry(id.to_string()).or_default().push(image);
    }
  }

  let output = run_standard_command(
    "podman images --format json",
    CommandOptions::default().timeout(Duration::from_secs(20)),
  )
  .await;
  if !output.success() {
    stderr.push(format!(
      "podman images failed | {}",
      output.stderr.trim()
    ));
    return false;
  }
  let images = match parse_images(&output.stdout) {
    Ok(images) => images,
    Err(e) => {
      stderr.push(format!("{e:#}"));
      return false;
    }
  };

  let kept = keep.values().flatten().cloned().collect::<Vec<_>>();
  if !kept.is_empty() {
    stdout.push(format!(
      "Kept, installed units use: {}",
      kept.join(", ")
    ));
  }

  let mut success = true;
  let mut removed = 0;
  for image in unused(&images, &keep) {
    // By name where it has any: `podman rmi <id>` refuses an image with
    // several names unless forced, and forcing removes its containers
    // too, such as one a timer started since the listing.
    let targets = if image.names.is_empty() {
      vec![image.id.as_str()]
    } else {
      image.names.iter().map(String::as_str).collect()
    };
    let output = run_standard_command(
      &format!(
        "podman rmi -- {}",
        targets
          .iter()
          .map(|t| escape((*t).into()))
          .collect::<Vec<_>>()
          .join(" ")
      ),
      CommandOptions::default().timeout(Duration::from_secs(300)),
    )
    .await;
    let label = if image.names.is_empty() {
      short_id(&image.id).to_string()
    } else {
      image.names.join(", ")
    };
    if output.success() {
      removed += 1;
      stdout.push(format!("Removed {label}"));
    } else {
      success = false;
      stderr.push(format!(
        "Failed to remove {label} | {}",
        output.stderr.trim()
      ));
    }
  }
  if removed == 0 && success {
    stdout.push(String::from("No unused images"));
  }
  success
}

/// The images the units name: a `.container`'s `Image=` unless it is
/// another unit, an `.image`'s `Image=`, and the tag a `.build` makes.
/// The images in a `.kube` unit's YAML are not read.
fn image_refs(units: &[ParsedUnit]) -> BTreeSet<String> {
  units
    .iter()
    .filter_map(|u| match u.unit.kind {
      QuadletKind::Container
        if !u.unit.image.ends_with(".image")
          && !u.unit.image.ends_with(".build") =>
      {
        Some(u.unit.image.clone())
      }
      QuadletKind::Image => Some(u.unit.image.clone()),
      QuadletKind::Build => Some(u.unit.name.clone()),
      _ => None,
    })
    .filter(|image| !image.is_empty())
    .collect()
}

/// `.container`, `.image` and `.build` units anywhere under `root`.
/// Directory symlinks are not followed.
fn installed_units(root: &Path, skip_users: bool) -> Vec<ParsedUnit> {
  let mut units = Vec::new();
  let mut dirs = vec![root.to_path_buf()];
  while let Some(dir) = dirs.pop() {
    let Ok(entries) = std::fs::read_dir(&dir) else {
      continue;
    };
    for entry in entries.flatten() {
      let Ok(file_type) = entry.file_type() else {
        continue;
      };
      let path = entry.path();
      let name = entry.file_name().to_string_lossy().into_owned();
      if file_type.is_dir() {
        if !(skip_users && dir == root && name == "users") {
          dirs.push(path);
        }
        continue;
      }
      if !matches!(
        QuadletKind::from_file_name(&name),
        Some(
          QuadletKind::Container
            | QuadletKind::Image
            | QuadletKind::Build
        )
      ) || !path.is_file()
      {
        continue;
      }
      if let Ok(contents) = std::fs::read_to_string(&path)
        && let Some(unit) =
          parse_unit(&path.to_string_lossy(), &contents)
      {
        units.push(unit);
      }
    }
  }
  units
}

#[derive(Debug, Deserialize)]
struct ImageItem {
  #[serde(rename = "Id")]
  id: String,
  #[serde(rename = "Names", default)]
  names: Option<Vec<String>>,
  #[serde(rename = "Containers", default)]
  containers: i64,
}

#[derive(Debug, PartialEq, Eq)]
struct Image {
  id: String,
  names: Vec<String>,
  containers: i64,
}

fn parse_images(json: &str) -> anyhow::Result<Vec<Image>> {
  let json = json.trim();
  if json.is_empty() {
    return Ok(Vec::new());
  }
  let items: Vec<ImageItem> =
    serde_json::from_str(json).map_err(|e| {
      anyhow::anyhow!("Failed to parse podman images json | {e}")
    })?;
  // podman lists an image once for each of its names.
  let mut images = Vec::<Image>::new();
  for item in items {
    if images.iter().any(|image| image.id == item.id) {
      continue;
    }
    images.push(Image {
      id: item.id,
      names: item.names.unwrap_or_default(),
      containers: item.containers,
    });
  }
  Ok(images)
}

/// The images to remove: no container uses them, and no unit names them.
fn unused<'a>(
  images: &'a [Image],
  keep: &BTreeMap<String, Vec<String>>,
) -> impl Iterator<Item = &'a Image> {
  images.iter().filter(|image| {
    image.containers == 0 && !keep.contains_key(&image.id)
  })
}

fn short_id(id: &str) -> &str {
  id.get(..12).unwrap_or(id)
}

#[cfg(test)]
mod tests {
  use super::*;

  fn units(files: &[(&str, &str)]) -> Vec<ParsedUnit> {
    files
      .iter()
      .filter_map(|(path, contents)| parse_unit(path, contents))
      .collect()
  }

  #[test]
  fn refs_cover_containers_images_and_builds() {
    let units = units(&[
      (
        "backup/restic.container",
        "[Container]\nImage=docker.io/restic/restic:0.17\nStartWithPod=false\n[Service]\nType=oneshot\n",
      ),
      ("app/web.container", "[Container]\nImage=web.build\n"),
      ("app/web.build", "[Build]\nImageTag=localhost/web:1\n"),
      ("app/db.container", "[Container]\nImage=pg.image\n"),
      (
        "app/pg.image",
        "[Image]\nImage=docker.io/library/postgres:17\n",
      ),
      ("app/plain.build", "[Build]\nFile=Containerfile\n"),
      ("app/app.pod", "[Pod]\n"),
    ]);
    assert_eq!(
      image_refs(&units).into_iter().collect::<Vec<_>>(),
      vec![
        "docker.io/library/postgres:17",
        "docker.io/restic/restic:0.17",
        "localhost/plain",
        "localhost/web:1",
      ]
    );
  }

  #[test]
  fn walks_the_quadlet_dir_and_skips_users_as_root() {
    let root = std::env::temp_dir()
      .join(format!("quomodo-prune-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let write = |rel: &str, contents: &str| {
      let path = root.join(rel);
      std::fs::create_dir_all(path.parent().unwrap()).unwrap();
      std::fs::write(path, contents).unwrap();
    };
    write("hand.container", "[Container]\nImage=alpine\n");
    write(
      "backup/nested/restic.container",
      "[Container]\nImage=restic\n",
    );
    write("backup/restic.env", "Image=not-a-unit\n");
    write(
      "users/1000/theirs.container",
      "[Container]\nImage=theirs\n",
    );

    let refs = |skip_users| {
      image_refs(&installed_units(&root, skip_users))
        .into_iter()
        .collect::<Vec<_>>()
    };
    assert_eq!(refs(true), vec!["alpine", "restic"]);
    assert_eq!(refs(false), vec!["alpine", "restic", "theirs"]);
    std::fs::remove_dir_all(&root).unwrap();
  }

  #[test]
  fn keeps_used_and_named_images() {
    // Trimmed `podman images --format json` output, which repeats an
    // image for each of its names.
    let images = parse_images(
      r#"[
        {"Id":"aaa","Names":["docker.io/restic/restic:0.17"],"Containers":0},
        {"Id":"bbb","Names":["docker.io/library/nginx:1"],"Containers":1},
        {"Id":"ccc","Names":["docker.io/library/nginx:0.9","old:tag"],"Containers":0},
        {"Id":"ccc","Names":["docker.io/library/nginx:0.9","old:tag"],"Containers":0},
        {"Id":"ddd","Names":null,"Containers":0}
      ]"#,
    )
    .unwrap();
    let keep = BTreeMap::from([(
      String::from("aaa"),
      vec![String::from("restic/restic:0.17")],
    )]);
    let ids = unused(&images, &keep)
      .map(|i| i.id.as_str())
      .collect::<Vec<_>>();
    assert_eq!(ids, vec!["ccc", "ddd"]);
    assert!(parse_images("").unwrap().is_empty());
  }
}
