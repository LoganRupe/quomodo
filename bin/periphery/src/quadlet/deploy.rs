//! Deploy planning and execution for a quadlet stack.

use std::{
  collections::{BTreeMap, BTreeSet},
  os::unix::fs::PermissionsExt,
  path::{Path, PathBuf},
  time::Duration,
};

use anyhow::{Context, anyhow};
use komodo_client::entities::{stack::QuadletKind, update::Log};

use super::{sha256_hex, state::StackState, unit::base_name};
use crate::{podman, systemd};
use quadlet::prepare::Prepared;

/// What the deploy will do to systemd units.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Plan {
  /// Services of units that were installed before and are gone now.
  pub stop_removed: Vec<String>,
  /// Services to `systemctl start` (new runnable units).
  pub start: Vec<String>,
  /// Services to `systemctl restart` (changed runnable units, or dependents of changed ones).
  pub restart: Vec<String>,
  /// Services among `start` / `restart` which must stay active: the
  /// runnable ones. A `.build` / `.image` service is a oneshot that exits
  /// when done; `systemctl` already fails if it fails.
  pub wait: Vec<String>,
  /// Human readable reasons, for the log.
  pub reasons: Vec<String>,
  /// Warnings about changes systemd will not apply on its own.
  pub warnings: Vec<String>,
}

/// Decide which units to (re)start.
///
/// - `services` non-empty restricts restarts to those unit files (and forces them).
/// - a changed `.pod` restarts the pod service; members follow via BindsTo.
/// - a changed `.network` / `.volume` / `.image` / `.build` restarts every
///   unit referencing it, plus its own service.
/// - a changed `.container` / `.kube` restarts only itself.
/// - a changed or removed non-unit file restarts the units that read it:
///   bind-mount sources, and `.build` contexts minus what the build's
///   `.containerignore` / `.dockerignore` leaves out (a rebuild restarts
///   the image's dependents).
/// - a unit referencing a changed podman secret restarts.
pub fn plan(
  prepared: &Prepared,
  previous: Option<&StackState>,
  services: &[String],
) -> Plan {
  let mut plan = Plan::default();
  let units = &prepared.units;
  let by_file = units
    .iter()
    .map(|u| (u.unit.file.as_str(), u))
    .collect::<BTreeMap<_, _>>();

  let new_hashes = prepared
    .write
    .iter()
    .map(|(p, c)| (p.clone(), sha256_hex(c)))
    .chain(prepared.blobs.iter().map(|(p, h)| (p.clone(), h.clone())))
    .collect::<BTreeMap<_, _>>();

  let Some(previous) = previous else {
    // No saved state: either a first deploy or the state file was lost while
    // units may still be running. Restart, which also starts inactive units.
    plan.reasons.push(String::from(
      "No deploy state for this stack: restarting every runnable unit",
    ));
    plan.restart = units
      .iter()
      .filter(|u| u.unit.kind.is_runnable())
      .map(|u| u.unit.service.clone())
      .collect();
    plan.restart.sort();
    plan.restart.dedup();
    plan.wait = plan.restart.clone();
    return plan;
  };

  // Removed units
  for file in &previous.units {
    if !by_file.contains_key(file.as_str())
      && let Some(kind) = QuadletKind::from_file_name(base_name(file))
      && kind.is_runnable()
    {
      let stem = super::unit::stem(base_name(file));
      plan.stop_removed.push(kind.service_name(stem));
      plan.reasons.push(format!("{file}: removed, stopping"));
    }
  }

  // Changed unit files
  let mut changed_units = BTreeSet::<&str>::new();
  for u in units {
    let file = u.unit.file.as_str();
    match (previous.files.get(file), new_hashes.get(file)) {
      (Some(old), Some(new)) if old == new => {}
      (Some(_), _) => {
        changed_units.insert(file);
        plan.reasons.push(format!("{file}: contents changed"));
      }
      (None, _) => {
        changed_units.insert(file);
        plan.reasons.push(format!("{file}: new unit"));
      }
    }
  }

  // Changed or removed non-unit files restart the units that read them.
  let changed_files = new_hashes
    .iter()
    .filter(|(path, hash)| previous.files.get(*path) != Some(*hash))
    .map(|(path, _)| path.as_str())
    .chain(
      previous
        .files
        .keys()
        .filter(|path| !new_hashes.contains_key(*path))
        .map(String::as_str),
    )
    .filter(|path| {
      QuadletKind::from_file_name(base_name(path)).is_none()
    })
    .collect::<BTreeSet<_>>();
  for path in changed_files {
    let mut mounted = false;
    for u in units.iter().filter(|u| u.reads(path, &prepared.write)) {
      mounted = true;
      if changed_units.insert(u.unit.file.as_str()) {
        plan.reasons.push(format!(
          "{}: {} changed file {path}",
          u.unit.file,
          if u.unit.kind == QuadletKind::Build {
            "build context has"
          } else {
            "mounts"
          }
        ));
      }
    }
    if !mounted {
      plan.warnings.push(format!(
        "{path}: changed, but no unit in this stack mounts or builds from it, so nothing restarts for it"
      ));
    }
  }

  // Units referencing changed secrets
  let changed_secrets = prepared
    .secret_hashes
    .iter()
    .filter(|(name, hash)| previous.secrets.get(*name) != Some(*hash))
    .map(|(name, _)| name.as_str())
    .collect::<BTreeSet<_>>();
  for u in units {
    if u
      .secrets
      .iter()
      .any(|s| changed_secrets.contains(s.as_str()))
      && changed_units.insert(u.unit.file.as_str())
    {
      plan.reasons.push(format!(
        "{}: references a changed secret",
        u.unit.file
      ));
    }
  }

  // Forced services
  let forced = services
    .iter()
    .filter_map(|s| {
      units
        .iter()
        .find(|u| {
          u.unit.file == *s
            || base_name(&u.unit.file) == s
            || super::unit::stem(base_name(&u.unit.file)) == s
            || u.unit.service == *s
        })
        .map(|u| u.unit.file.as_str())
    })
    .collect::<BTreeSet<_>>();

  // Expand to dependents of changed non-runnable units.
  let mut to_restart = BTreeSet::<&str>::new();
  for file in &changed_units {
    let Some(u) = by_file.get(file) else { continue };
    match u.unit.kind {
      QuadletKind::Container
      | QuadletKind::Kube
      | QuadletKind::Pod => {
        to_restart.insert(file);
      }
      QuadletKind::Network | QuadletKind::Volume => {
        to_restart.insert(file);
        plan.warnings.push(format!(
          "{file}: podman does not recreate an existing {}; remove it by hand for the change to apply",
          u.unit.kind
        ));
        for d in units
          .iter()
          .filter(|d| d.deps.iter().any(|dep| dep == file))
        {
          if to_restart.insert(d.unit.file.as_str()) {
            plan.reasons.push(format!(
              "{}: depends on changed {file}",
              d.unit.file
            ));
          }
        }
      }
      QuadletKind::Image | QuadletKind::Build => {
        to_restart.insert(file);
        for d in units
          .iter()
          .filter(|d| d.deps.iter().any(|dep| dep == file))
        {
          if to_restart.insert(d.unit.file.as_str()) {
            plan.reasons.push(format!(
              "{}: depends on changed {file}",
              d.unit.file
            ));
          }
        }
      }
    }
  }

  if !services.is_empty() {
    // Restrict to the requested units, but always include them.
    to_restart.retain(|f| forced.contains(f));
    for f in &forced {
      if to_restart.insert(f) {
        plan.reasons.push(format!("{f}: restart requested"));
      }
    }
  }

  // A container whose pod is restarting follows the pod.
  let restarting_pods = to_restart
    .iter()
    .filter(|f| {
      by_file
        .get(*f)
        .map(|u| u.unit.kind == QuadletKind::Pod)
        .unwrap_or(false)
    })
    .map(|f| f.to_string())
    .collect::<BTreeSet<_>>();
  to_restart.retain(|f| {
    by_file
      .get(f)
      .and_then(|u| u.unit.pod.as_deref())
      .map(|pod| !restarting_pods.contains(pod))
      .unwrap_or(true)
  });

  for file in to_restart {
    let Some(u) = by_file.get(file) else { continue };
    let service = u.unit.service.clone();
    if u.unit.kind.is_runnable() {
      plan.wait.push(service.clone());
    }
    if previous.units.iter().any(|f| f == file) {
      plan.restart.push(service);
    } else {
      plan.start.push(service);
    }
  }
  plan.start.sort();
  plan.start.dedup();
  plan.restart.sort();
  plan.restart.dedup();
  plan.wait.sort();
  plan.wait.dedup();
  plan
}

/// Write the prepared files under the target directory, copy the blobs
/// from the run directory, and remove files which are no longer part of
/// the stack. Every file takes its mode from the run directory, so an
/// executable in a build context stays executable.
/// Returns the installed file hashes.
pub async fn write_files(
  target: &Path,
  source: &Path,
  prepared: &Prepared,
  logs: &mut Vec<Log>,
) -> anyhow::Result<BTreeMap<String, String>> {
  tokio::fs::create_dir_all(target).await.with_context(|| {
    format!("Failed to create {}", target.display())
  })?;

  // Remove stale files.
  let existing = super::read_stack_files(target).await?;
  let mut removed = Vec::new();
  for rel in existing
    .files
    .iter()
    .map(|f| &f.path)
    .chain(existing.blobs.iter().map(|b| &b.path))
  {
    if !prepared.write.contains_key(rel)
      && !prepared.blobs.contains_key(rel)
    {
      let path = target.join(rel);
      tokio::fs::remove_file(&path).await.with_context(|| {
        format!("Failed to remove {}", path.display())
      })?;
      removed.push(rel.clone());
    }
  }
  let existing_blobs = existing
    .blobs
    .iter()
    .map(|b| (b.path.as_str(), b.sha256.as_str()))
    .collect::<BTreeMap<_, _>>();

  let mut hashes = BTreeMap::new();
  let mut written = Vec::new();
  for (rel, contents) in &prepared.write {
    let path = target_path(target, rel).await?;
    let current = tokio::fs::read_to_string(&path).await.ok();
    if current.as_deref() != Some(contents.as_str()) {
      let tmp = tmp_path(&path);
      tokio::fs::write(&tmp, contents).await.with_context(|| {
        format!("Failed to write {}", tmp.display())
      })?;
      tokio::fs::rename(&tmp, &path).await.with_context(|| {
        format!("Failed to move {} into place", tmp.display())
      })?;
      written.push(rel.clone());
    }
    copy_mode(&source.join(rel), &path).await?;
    hashes.insert(rel.clone(), sha256_hex(contents));
  }
  for (rel, sha256) in &prepared.blobs {
    let path = target_path(target, rel).await?;
    if existing_blobs.get(rel.as_str()) != Some(&sha256.as_str()) {
      let from = source.join(rel);
      let tmp = tmp_path(&path);
      tokio::fs::copy(&from, &tmp).await.with_context(|| {
        format!(
          "Failed to copy {} to {}",
          from.display(),
          tmp.display()
        )
      })?;
      tokio::fs::rename(&tmp, &path).await.with_context(|| {
        format!("Failed to move {} into place", tmp.display())
      })?;
      written.push(rel.clone());
    }
    copy_mode(&source.join(rel), &path).await?;
    hashes.insert(rel.clone(), sha256.clone());
  }

  let mut msg = format!("Target directory: {}", target.display());
  if !written.is_empty() {
    msg.push_str(&format!("\nWritten: {}", written.join(", ")));
  }
  if !removed.is_empty() {
    msg.push_str(&format!("\nRemoved: {}", removed.join(", ")));
  }
  if written.is_empty() && removed.is_empty() {
    msg.push_str("\nNo file changes");
  }
  logs.push(Log::simple("Write Stack Files", msg));
  Ok(hashes)
}

/// The install path for a stack file, with its parent directory created.
async fn target_path(
  target: &Path,
  rel: &str,
) -> anyhow::Result<PathBuf> {
  if rel.contains("..") {
    return Err(anyhow!("Refusing to write path with '..': {rel}"));
  }
  let path = target.join(rel);
  if let Some(parent) = path.parent() {
    tokio::fs::create_dir_all(parent).await.with_context(|| {
      format!("Failed to create {}", parent.display())
    })?;
  }
  Ok(path)
}

fn tmp_path(path: &Path) -> PathBuf {
  path.with_extension(format!(
    "{}.quomodo-tmp",
    path.extension().and_then(|e| e.to_str()).unwrap_or("")
  ))
}

/// Give `to` the permission bits of `from`, if they differ.
async fn copy_mode(from: &Path, to: &Path) -> anyhow::Result<()> {
  let Ok(from_meta) = tokio::fs::metadata(from).await else {
    return Ok(());
  };
  let mode = from_meta.permissions().mode() & 0o7777;
  let current = tokio::fs::metadata(to)
    .await
    .with_context(|| format!("Failed to stat {}", to.display()))?
    .permissions()
    .mode()
    & 0o7777;
  if current != mode {
    tokio::fs::set_permissions(
      to,
      std::fs::Permissions::from_mode(mode),
    )
    .await
    .with_context(|| format!("Failed to chmod {}", to.display()))?;
  }
  Ok(())
}

/// Create / replace the stack's podman secrets. Values never touch the log.
pub async fn create_secrets(
  prepared: &Prepared,
  previous: Option<&StackState>,
  logs: &mut Vec<Log>,
) -> anyhow::Result<()> {
  let mut created = Vec::new();
  for (name, value) in &prepared.secrets {
    let unchanged = previous
      .and_then(|p| p.secrets.get(name))
      .map(|h| Some(h) == prepared.secret_hashes.get(name))
      .unwrap_or(false);
    if unchanged {
      continue;
    }
    podman::secret_create(name, value).await?;
    created.push(name.clone());
  }
  // Remove secrets from the previous deploy which are no longer referenced.
  let mut removed = Vec::new();
  if let Some(previous) = previous {
    for name in previous.secrets.keys() {
      if !prepared.secrets.contains_key(name) {
        let log = podman::secret_rm(name).await;
        if log.success {
          removed.push(name.clone());
        } else {
          warn!(
            "Failed to remove stale secret {name} | {}",
            log.stderr
          );
        }
      }
    }
  }
  let msg = match (created.is_empty(), removed.is_empty()) {
    (true, true) => String::from("No secret changes"),
    _ => format!(
      "Created / replaced: {}\nRemoved: {}",
      if created.is_empty() {
        "-".into()
      } else {
        created.join(", ")
      },
      if removed.is_empty() {
        "-".into()
      } else {
        removed.join(", ")
      },
    ),
  };
  logs.push(Log::simple("Podman Secrets", msg));
  Ok(())
}

/// Apply the plan. Returns Ok(false) when a unit failed to come up.
pub async fn apply(
  plan: &Plan,
  logs: &mut Vec<Log>,
) -> anyhow::Result<bool> {
  if !plan.reasons.is_empty() || !plan.warnings.is_empty() {
    let mut msg = plan.reasons.join("\n");
    if !plan.warnings.is_empty() {
      msg.push_str("\nWarnings:\n");
      msg.push_str(&plan.warnings.join("\n"));
    }
    logs.push(Log::simple("Deploy Plan", msg));
  }

  if !plan.stop_removed.is_empty() {
    let log = systemd::systemctl(
      "Stop Removed Units",
      "stop",
      &plan.stop_removed,
    )
    .await;
    // Stopping a unit that is already gone is not fatal.
    logs.push(log);
  }

  let reload = systemd::daemon_reload().await;
  let ok = reload.success;
  logs.push(reload);
  if !ok {
    return Ok(false);
  }

  if !plan.restart.is_empty() {
    let log =
      systemd::systemctl("Restart Units", "restart", &plan.restart)
        .await;
    let ok = log.success;
    logs.push(log);
    if !ok {
      push_journals(&plan.restart, logs).await;
      return Ok(false);
    }
  }
  if !plan.start.is_empty() {
    let log =
      systemd::systemctl("Start Units", "start", &plan.start).await;
    let ok = log.success;
    logs.push(log);
    if !ok {
      push_journals(&plan.start, logs).await;
      return Ok(false);
    }
  }

  let touched = plan
    .restart
    .iter()
    .chain(plan.start.iter())
    .cloned()
    .collect::<Vec<_>>();
  if touched.is_empty() {
    logs.push(Log::simple(
      "Units",
      String::from("No units needed a restart"),
    ));
    return Ok(true);
  }
  let failed =
    systemd::wait_active(&plan.wait, Duration::from_secs(20)).await;
  if failed.is_empty() {
    logs.push(Log::simple(
      "Units",
      format!(
        "Active: {}",
        if plan.wait.is_empty() {
          String::from("-")
        } else {
          plan.wait.join(", ")
        }
      ),
    ));
    Ok(true)
  } else {
    logs.push(Log::error(
      "Units",
      format!("Not active after start: {}", failed.join(", ")),
    ));
    push_journals(&failed, logs).await;
    Ok(false)
  }
}

async fn push_journals(units: &[String], logs: &mut Vec<Log>) {
  for unit in units {
    let journal = systemd::journal_tail(unit, 40).await;
    logs.push(Log::error(&format!("journalctl -u {unit}"), journal));
  }
}

/// Everything the deploy needs to remember for next time.
pub fn next_state(
  files: BTreeMap<String, String>,
  prepared: &Prepared,
) -> StackState {
  StackState {
    files,
    secrets: prepared.secret_hashes.clone(),
    units: prepared
      .units
      .iter()
      .map(|u| u.unit.file.clone())
      .collect(),
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::quadlet::unit::parse_unit;

  fn prepared(
    files: &[(&str, &str)],
    secrets: &[(&str, &str)],
  ) -> Prepared {
    let mut p = Prepared::default();
    for (path, contents) in files {
      p.write.insert(path.to_string(), contents.to_string());
    }
    for (name, hash) in secrets {
      p.secret_hashes.insert(name.to_string(), hash.to_string());
    }
    p.units = p
      .write
      .iter()
      .filter_map(|(path, c)| parse_unit(path, c))
      .collect();
    p
  }

  fn state(p: &Prepared) -> StackState {
    next_state(
      p.write
        .iter()
        .map(|(k, v)| (k.clone(), sha256_hex(v)))
        .chain(p.blobs.iter().map(|(k, v)| (k.clone(), v.clone())))
        .collect(),
      p,
    )
  }

  const POD: &str = "[Pod]\nPodName=app\n";
  const WEB: &str = "[Container]\nImage=nginx\nPod=app.pod\nNetwork=app.network\nSecret=s_PW\n";
  const DB: &str = "[Container]\nImage=postgres\nPod=app.pod\nVolume=data.volume:/var/lib/postgresql\n";
  const NET: &str = "[Network]\nSubnet=10.9.0.0/24\n";
  const VOL: &str = "[Volume]\n";
  const SOLO: &str = "[Container]\nImage=busybox\n";

  fn base() -> Prepared {
    prepared(
      &[
        ("app.pod", POD),
        ("web.container", WEB),
        ("db.container", DB),
        ("app.network", NET),
        ("data.volume", VOL),
        ("solo.container", SOLO),
      ],
      &[("s_PW", "h1")],
    )
  }

  #[test]
  fn first_deploy_or_lost_state_restarts_runnable_units() {
    let p = base();
    let plan = plan(&p, None, &[]);
    assert_eq!(
      plan.restart,
      vec![
        "app-pod.service",
        "db.service",
        "solo.service",
        "web.service"
      ]
    );
    assert!(plan.start.is_empty());
  }

  #[test]
  fn changed_config_file_restarts_mounting_unit() {
    let mut p = base();
    p.write.insert(
      "caddy.container".into(),
      "[Container]\nImage=caddy\nVolume=./Caddyfile:/etc/caddy/Caddyfile:ro\n".into(),
    );
    p.write.insert("Caddyfile".into(), "v1".into());
    p.write.insert("notes.txt".into(), "a".into());
    p.units = p
      .write
      .iter()
      .filter_map(|(k, v)| parse_unit(k, v))
      .collect();
    let s = state(&p);
    let mut p2 = p;
    p2.write.insert("Caddyfile".into(), "v2".into());
    p2.write.insert("notes.txt".into(), "b".into());
    let plan = plan(&p2, Some(&s), &[]);
    assert_eq!(plan.restart, vec!["caddy.service"]);
    assert!(plan.warnings.iter().any(|w| w.starts_with("notes.txt")));
  }

  #[test]
  fn unchanged_is_a_no_op() {
    let p = base();
    let s = state(&p);
    let plan = plan(&p, Some(&s), &[]);
    assert!(
      plan.start.is_empty()
        && plan.restart.is_empty()
        && plan.stop_removed.is_empty()
    );
  }

  #[test]
  fn changed_container_restarts_only_itself() {
    let p = base();
    let s = state(&p);
    let mut p2 = base();
    p2.write.insert(
      "solo.container".into(),
      "[Container]\nImage=busybox:2\n".into(),
    );
    p2.units = p2
      .write
      .iter()
      .filter_map(|(k, v)| parse_unit(k, v))
      .collect();
    let plan = plan(&p2, Some(&s), &[]);
    assert_eq!(plan.restart, vec!["solo.service"]);
    assert!(plan.start.is_empty());
  }

  #[test]
  fn changed_pod_restarts_pod_not_members() {
    let p = base();
    let s = state(&p);
    let mut p2 = base();
    p2.write.insert(
      "app.pod".into(),
      "[Pod]\nPodName=app\nPublishPort=8080:80\n".into(),
    );
    p2.write
      .insert("web.container".into(), format!("{WEB}Label=x=y\n"));
    p2.units = p2
      .write
      .iter()
      .filter_map(|(k, v)| parse_unit(k, v))
      .collect();
    let plan = plan(&p2, Some(&s), &[]);
    assert_eq!(plan.restart, vec!["app-pod.service"]);
  }

  #[test]
  fn changed_network_restarts_dependents_with_warning() {
    let p = base();
    let s = state(&p);
    let mut p2 = base();
    p2.write.insert(
      "app.network".into(),
      "[Network]\nSubnet=10.9.1.0/24\n".into(),
    );
    p2.units = p2
      .write
      .iter()
      .filter_map(|(k, v)| parse_unit(k, v))
      .collect();
    let plan = plan(&p2, Some(&s), &[]);
    assert_eq!(
      plan.restart,
      vec!["app-network.service", "web.service"]
    );
    assert_eq!(plan.warnings.len(), 1);
  }

  #[test]
  fn changed_secret_restarts_referencing_unit() {
    let p = base();
    let s = state(&p);
    let mut p2 = base();
    p2.secret_hashes.insert("s_PW".into(), "h2".into());
    let plan = plan(&p2, Some(&s), &[]);
    assert_eq!(plan.restart, vec!["web.service"]);
  }

  #[test]
  fn removed_unit_is_stopped_and_new_unit_started() {
    let p = base();
    let s = state(&p);
    let mut p2 = base();
    p2.write.remove("solo.container");
    p2.write.insert("extra.container".into(), SOLO.into());
    p2.units = p2
      .write
      .iter()
      .filter_map(|(k, v)| parse_unit(k, v))
      .collect();
    let plan = plan(&p2, Some(&s), &[]);
    assert_eq!(plan.stop_removed, vec!["solo.service"]);
    assert_eq!(plan.start, vec!["extra.service"]);
    assert!(plan.restart.is_empty());
  }

  #[test]
  fn requested_services_are_forced_and_limit_the_rest() {
    let p = base();
    let s = state(&p);
    let mut p2 = base();
    p2.write.insert(
      "solo.container".into(),
      "[Container]\nImage=busybox:2\n".into(),
    );
    p2.units = p2
      .write
      .iter()
      .filter_map(|(k, v)| parse_unit(k, v))
      .collect();
    let plan = plan(&p2, Some(&s), &["db.container".to_string()]);
    assert_eq!(plan.restart, vec!["db.service"]);
  }

  const APP_BUILD: &str = "[Build]\nImageTag=localhost/app\nSetWorkingDirectory=unit\nFile=Dockerfile\n";
  const APP: &str = "[Container]\nImage=app.build\n";

  fn build_stack() -> Prepared {
    let mut p = prepared(
      &[
        ("app.build", APP_BUILD),
        ("app.container", APP),
        ("Dockerfile", "FROM alpine\n"),
        ("main.go", "package main\n"),
      ],
      &[],
    );
    p.blobs.insert("assets/logo.png".into(), "h1".into());
    p
  }

  #[test]
  fn changed_build_context_rebuilds_and_restarts_dependents() {
    let p = build_stack();
    let s = state(&p);

    let mut dockerfile = build_stack();
    dockerfile
      .write
      .insert("Dockerfile".into(), "FROM alpine:3\n".into());
    let a = plan(&dockerfile, Some(&s), &[]);
    assert_eq!(a.restart, vec!["app-build.service", "app.service"]);
    assert_eq!(a.wait, vec!["app.service"]);
    assert!(a.warnings.is_empty(), "{:?}", a.warnings);

    let mut blob = build_stack();
    blob.blobs.insert("assets/logo.png".into(), "h2".into());
    let b = plan(&blob, Some(&s), &[]);
    assert_eq!(b.restart, vec!["app-build.service", "app.service"]);

    let mut removed = build_stack();
    removed.write.remove("main.go");
    let c = plan(&removed, Some(&s), &[]);
    assert_eq!(c.restart, vec!["app-build.service", "app.service"]);

    let d = plan(&build_stack(), Some(&s), &[]);
    assert!(d.restart.is_empty() && d.start.is_empty());
  }

  #[test]
  fn ignored_build_file_does_not_rebuild() {
    let mut p = build_stack();
    p.write
      .insert(".containerignore".into(), "*.md\nassets\n".into());
    p.write.insert("README.md".into(), "v1".into());
    let s = state(&p);

    let mut docs = p;
    docs.write.insert("README.md".into(), "v2".into());
    docs.blobs.insert("assets/logo.png".into(), "h2".into());
    let a = plan(&docs, Some(&s), &[]);
    assert!(a.restart.is_empty(), "{:?}", a.restart);
    assert_eq!(a.warnings.len(), 2, "{:?}", a.warnings);

    let mut rules = docs;
    rules
      .write
      .insert(".containerignore".into(), "*.md\n".into());
    let b = plan(&rules, Some(&s), &[]);
    assert_eq!(b.restart, vec!["app-build.service", "app.service"]);
  }

  #[test]
  fn changed_blob_restarts_mounting_unit() {
    let mut p = prepared(
      &[(
        "caddy.container",
        "[Container]\nImage=caddy\nVolume=./assets:/srv/assets:ro\n",
      )],
      &[],
    );
    p.blobs.insert("assets/bg.png".into(), "h1".into());
    let s = state(&p);
    p.blobs.insert("assets/bg.png".into(), "h2".into());
    let plan = plan(&p, Some(&s), &[]);
    assert_eq!(plan.restart, vec!["caddy.service"]);
  }

  #[tokio::test]
  async fn write_files_copies_blobs_keeps_modes_and_removes_stale() {
    let root = std::env::temp_dir()
      .join(format!("quomodo-write-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let (source, target) = (root.join("src"), root.join("dst"));
    std::fs::create_dir_all(source.join("assets")).unwrap();
    std::fs::create_dir_all(&target).unwrap();
    let png = [0x89, b'P', b'N', b'G', 0xff, 0x00];
    std::fs::write(source.join("assets/logo.png"), png).unwrap();
    std::fs::write(source.join("entrypoint.sh"), "#!/bin/sh\n")
      .unwrap();
    std::fs::set_permissions(
      source.join("entrypoint.sh"),
      std::fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    std::fs::write(target.join("old.txt"), "stale").unwrap();
    std::fs::write(target.join("old.bin"), [0xffu8, 0xfe]).unwrap();

    let mut p = Prepared::default();
    p.write.insert("entrypoint.sh".into(), "#!/bin/sh\n".into());
    p.blobs.insert(
      "assets/logo.png".into(),
      quadlet::sha256_file(&source.join("assets/logo.png")).unwrap(),
    );
    let mut logs = Vec::new();
    let hashes =
      write_files(&target, &source, &p, &mut logs).await.unwrap();

    assert_eq!(
      std::fs::read(target.join("assets/logo.png")).unwrap(),
      png
    );
    assert_eq!(
      std::fs::metadata(target.join("entrypoint.sh"))
        .unwrap()
        .permissions()
        .mode()
        & 0o777,
      0o755
    );
    assert!(!target.join("old.txt").exists());
    assert!(!target.join("old.bin").exists());
    assert_eq!(hashes["assets/logo.png"], p.blobs["assets/logo.png"]);

    // A second run with nothing changed writes nothing.
    let mut logs = Vec::new();
    write_files(&target, &source, &p, &mut logs).await.unwrap();
    assert!(
      logs[0].stdout.contains("No file changes"),
      "{}",
      logs[0].stdout
    );
    std::fs::remove_dir_all(&root).unwrap();
  }
}
