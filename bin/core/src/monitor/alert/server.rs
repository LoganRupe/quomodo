use std::{
  collections::HashMap, path::PathBuf, str::FromStr, sync::OnceLock,
};

use anyhow::Context;
use database::mongo_indexed::Indexed;
use database::mungos::{
  bulk_update::{self, BulkUpdate},
  find::find_collect,
  mongodb::bson::{doc, oid::ObjectId, to_bson},
};
use komodo_client::entities::{
  ResourceTarget,
  alert::{Alert, AlertData, AlertDataVariant, SeverityLevel},
  komodo_timestamp, optional_string,
  server::{Server, ServerState},
};

use crate::{
  alert::send_alerts,
  helpers::maintenance::is_in_maintenance,
  monitor::alert::AlertBuffer,
  state::{db_client, server_status_cache},
};

type SendAlerts = bool;
type OpenAlertMap<T = AlertDataVariant> =
  HashMap<ResourceTarget, HashMap<T, Alert>>;
type OpenDiskAlertMap = OpenAlertMap<PathBuf>;

/// Server alert buffer instance
fn alert_buffer() -> &'static AlertBuffer {
  static BUFFER: OnceLock<AlertBuffer> = OnceLock::new();
  BUFFER.get_or_init(AlertBuffer::new)
}

pub async fn alert_servers(
  ts: i64,
  mut servers: HashMap<String, Server>,
) {
  let server_statuses = server_status_cache().get_values().await;

  let (open_alerts, open_disk_alerts) = match get_open_alerts().await
  {
    Ok(alerts) => alerts,
    Err(e) => {
      error!("{e:#}");
      return;
    }
  };

  let mut alerts_to_open = Vec::<(Alert, SendAlerts)>::new();
  let mut alerts_to_update = Vec::<(Alert, SendAlerts)>::new();
  let mut alerts_to_close = Vec::<(Alert, SendAlerts)>::new();

  let buffer = alert_buffer();

  for server_status in server_statuses {
    let Some(server) = servers.remove(&server_status.id) else {
      continue;
    };
    let server_alerts = open_alerts
      .get(&ResourceTarget::Server(server_status.id.clone()));

    // Check if server is in maintenance mode
    let in_maintenance =
      is_in_maintenance(&server.config.maintenance_windows, ts);

    // ===============
    //  SERVER HEALTH
    // ===============
    let health_alert = server_alerts.as_ref().and_then(|alerts| {
      alerts.get(&AlertDataVariant::ServerUnreachable)
    });
    match (
      server_status.state,
      health_alert,
      server.config.send_unreachable_alerts,
    ) {
      // Skip alerting if alerts disabled
      (ServerState::NotOk, None, false) => {}
      (ServerState::NotOk, None, true) => {
        // Only open unreachable alert if unreachable alerts enabled and not in maintenance and buffer is ready
        if !in_maintenance
          && buffer.ready_to_open(
            server_status.id.clone(),
            AlertDataVariant::ServerUnreachable,
          )
        {
          let alert = Alert {
            id: Default::default(),
            ts,
            resolved: false,
            resolved_ts: None,
            level: SeverityLevel::Critical,
            target: ResourceTarget::Server(server_status.id.clone()),
            data: AlertData::ServerUnreachable {
              id: server_status.id.clone(),
              name: server.name.clone(),
              region: optional_string(&server.config.region),
              err: server_status.err.clone(),
            },
          };
          alerts_to_open
            .push((alert, server.config.send_unreachable_alerts))
        }
      }
      (ServerState::NotOk, Some(alert), true) => {
        // update alert err
        let mut alert = alert.clone();
        let (id, name, region) = match alert.data {
          AlertData::ServerUnreachable {
            id, name, region, ..
          } => (id, name, region),
          _ => unreachable!(),
        };
        alert.data = AlertData::ServerUnreachable {
          id,
          name,
          region,
          err: server_status.err.clone(),
        };

        // Never send this alert, severity is always 'Critical'
        alerts_to_update.push((alert, false));
      }

      (ServerState::NotOk, Some(alert), false) => {
        alerts_to_close.push((
          alert.clone(),
          server.config.send_unreachable_alerts,
        ));
      }

      // Close an open alert
      (ServerState::Ok | ServerState::Disabled, Some(alert), _) => {
        alerts_to_close.push((
          alert.clone(),
          server.config.send_unreachable_alerts,
        ));
      }
      (ServerState::Ok | ServerState::Disabled, None, _) => buffer
        .reset(
          server_status.id.clone(),
          AlertDataVariant::ServerUnreachable,
        ),
    }

    // ========================
    // SERVER VERSION MISMATCH
    // ========================
    let core_version = env!("CARGO_PKG_VERSION");
    let mismatched_server_version =
      if server_status.state != ServerState::Ok {
        None
      } else if let Some(version) =
        server_status.periphery_info.as_ref().map(|i| &i.version)
        && version != core_version
      {
        Some(version)
      } else {
        None
      };

    let version_alert = server_alerts.as_ref().and_then(|alerts| {
      alerts.get(&AlertDataVariant::ServerVersionMismatch)
    });

    match (
      mismatched_server_version,
      version_alert,
      server.config.send_version_mismatch_alerts,
    ) {
      (Some(_), None, false) => {}
      (Some(version), None, true) => {
        // Only open version mismatch alert if version mismatch alerts enabled and not in maintenance and buffer is ready
        if !in_maintenance
          && buffer.ready_to_open(
            server_status.id.clone(),
            AlertDataVariant::ServerVersionMismatch,
          )
        {
          let alert = Alert {
            id: Default::default(),
            ts,
            resolved: false,
            resolved_ts: None,
            level: SeverityLevel::Warning,
            target: ResourceTarget::Server(server_status.id.clone()),
            data: AlertData::ServerVersionMismatch {
              id: server_status.id.clone(),
              name: server.name.clone(),
              region: optional_string(&server.config.region),
              server_version: version.clone(),
              core_version: core_version.to_string(),
            },
          };
          // Use send_unreachable_alerts as a proxy for general server alerts
          alerts_to_open
            .push((alert, server.config.send_version_mismatch_alerts))
        }
      }
      (Some(version), Some(alert), true) => {
        // Update existing alert with current version info if changed
        let (alert_server_version, alert_core_version) =
          match &alert.data {
            AlertData::ServerVersionMismatch {
              server_version,
              core_version,
              ..
            } => (server_version, core_version),
            _ => unreachable!(),
          };

        if alert_server_version != version
          || alert_core_version != core_version
        {
          let mut alert = alert.clone();
          alert.data = AlertData::ServerVersionMismatch {
            id: server_status.id.clone(),
            name: server.name.clone(),
            region: optional_string(&server.config.region),
            server_version: version.clone(),
            core_version: core_version.to_string(),
          };
          // Don't send notification for updates
          alerts_to_update.push((alert, false));
        }
      }
      (Some(_), Some(alert), false) => {
        alerts_to_close.push((
          alert.clone(),
          server.config.send_version_mismatch_alerts,
        ));
      }
      (None, Some(alert), _) => {
        // Version is now correct, close the alert
        alerts_to_close.push((
          alert.clone(),
          server.config.send_version_mismatch_alerts,
        ));
      }
      (None, None, _) => {
        // Reset buffer state when no mismatch and no alert
        buffer.reset(
          server_status.id.clone(),
          AlertDataVariant::ServerVersionMismatch,
        )
      }
    }

    // ======================
    // SERVER PODMAN LISTING
    // ======================
    let podman_alert = server_alerts.as_ref().and_then(|alerts| {
      alerts.get(&AlertDataVariant::ServerPodmanUnresponsive)
    });
    // Uses the unreachable switch: both mean Core can't see what is
    // running on the server.
    let send = server.config.send_unreachable_alerts;
    match podman_alert_action(
      server_status.state,
      server_status.containers_failed_since,
      podman_alert.is_some(),
      send,
      ts,
    ) {
      PodmanAlertAction::Open(since) if !in_maintenance => {
        let alert = Alert {
          id: Default::default(),
          ts,
          resolved: false,
          resolved_ts: None,
          level: SeverityLevel::Warning,
          target: ResourceTarget::Server(server_status.id.clone()),
          data: AlertData::ServerPodmanUnresponsive {
            id: server_status.id.clone(),
            name: server.name.clone(),
            region: optional_string(&server.config.region),
            since,
          },
        };
        alerts_to_open.push((alert, send));
      }
      PodmanAlertAction::Close => {
        if let Some(alert) = podman_alert {
          alerts_to_close.push((alert.clone(), send));
        }
      }
      PodmanAlertAction::Open(_) | PodmanAlertAction::Nothing => {}
    }

    let Some(health) = &server_status.health else {
      continue;
    };

    // ===================
    // SERVER CPU
    // ===================
    let cpu_alert = server_alerts
      .as_ref()
      .and_then(|alerts| alerts.get(&AlertDataVariant::ServerCpu))
      .cloned();
    match (
      health.cpu.level,
      cpu_alert,
      health.cpu.should_close_alert,
      server.config.send_cpu_alerts,
    ) {
      (
        SeverityLevel::Warning | SeverityLevel::Critical,
        None,
        _,
        false,
      ) => {}
      (
        SeverityLevel::Warning | SeverityLevel::Critical,
        None,
        _,
        true,
      ) => {
        // Only open CPU alert if cpu alerts enabled and not in maintenance and buffer is ready
        if !in_maintenance
          && buffer.ready_to_open(
            server_status.id.clone(),
            AlertDataVariant::ServerCpu,
          )
        {
          let alert = Alert {
            id: Default::default(),
            ts,
            resolved: false,
            resolved_ts: None,
            level: health.cpu.level,
            target: ResourceTarget::Server(server_status.id.clone()),
            data: AlertData::ServerCpu {
              id: server_status.id.clone(),
              name: server.name.clone(),
              region: optional_string(&server.config.region),
              percentage: server_status
                .system_stats
                .as_ref()
                .map(|s| s.cpu_perc as f64)
                .unwrap_or(0.0),
            },
          };
          alerts_to_open.push((alert, server.config.send_cpu_alerts));
        }
      }
      (
        SeverityLevel::Warning | SeverityLevel::Critical,
        Some(mut alert),
        _,
        true,
      ) => {
        // modify alert level only if it has increased and not in maintenance
        if !in_maintenance && alert.level < health.cpu.level {
          alert.level = health.cpu.level;
          alert.data = AlertData::ServerCpu {
            id: server_status.id.clone(),
            name: server.name.clone(),
            region: optional_string(&server.config.region),
            percentage: server_status
              .system_stats
              .as_ref()
              .map(|s| s.cpu_perc as f64)
              .unwrap_or(0.0),
          };
          alerts_to_update
            .push((alert, server.config.send_cpu_alerts));
        }
      }
      (
        SeverityLevel::Warning | SeverityLevel::Critical,
        Some(alert),
        _,
        false,
      ) => {
        alerts_to_close.push((alert, server.config.send_cpu_alerts));
      }
      (SeverityLevel::Ok, Some(mut alert), true, _) => {
        alert.data = AlertData::ServerCpu {
          id: server_status.id.clone(),
          name: server.name.clone(),
          region: optional_string(&server.config.region),
          percentage: server_status
            .system_stats
            .as_ref()
            .map(|s| s.cpu_perc as f64)
            .unwrap_or(0.0),
        };
        alerts_to_close.push((alert, server.config.send_cpu_alerts))
      }
      (SeverityLevel::Ok, _, _, _) => buffer
        .reset(server_status.id.clone(), AlertDataVariant::ServerCpu),
    }

    // ===================
    // SERVER MEM
    // ===================
    let mem_alert = server_alerts
      .as_ref()
      .and_then(|alerts| alerts.get(&AlertDataVariant::ServerMem))
      .cloned();
    match (
      health.mem.level,
      mem_alert,
      health.mem.should_close_alert,
      server.config.send_mem_alerts,
    ) {
      (
        SeverityLevel::Warning | SeverityLevel::Critical,
        None,
        _,
        false,
      ) => {}
      (
        SeverityLevel::Warning | SeverityLevel::Critical,
        None,
        _,
        true,
      ) => {
        // Only open memory alert if memory alerts enabled and not in maintenance and buffer is ready
        if !in_maintenance
          && buffer.ready_to_open(
            server_status.id.clone(),
            AlertDataVariant::ServerMem,
          )
        {
          let alert = Alert {
            id: Default::default(),
            ts,
            resolved: false,
            resolved_ts: None,
            level: health.mem.level,
            target: ResourceTarget::Server(server_status.id.clone()),
            data: AlertData::ServerMem {
              id: server_status.id.clone(),
              name: server.name.clone(),
              region: optional_string(&server.config.region),
              total_gb: server_status
                .system_stats
                .as_ref()
                .map(|s| s.mem_total_gb)
                .unwrap_or(0.0),
              used_gb: server_status
                .system_stats
                .as_ref()
                .map(|s| s.mem_used_gb)
                .unwrap_or(0.0),
            },
          };
          alerts_to_open.push((alert, server.config.send_mem_alerts));
        }
      }
      (
        SeverityLevel::Warning | SeverityLevel::Critical,
        Some(mut alert),
        _,
        true,
      ) => {
        // modify alert level only if it has increased and not in maintenance
        if !in_maintenance && alert.level < health.mem.level {
          alert.level = health.mem.level;
          alert.data = AlertData::ServerMem {
            id: server_status.id.clone(),
            name: server.name.clone(),
            region: optional_string(&server.config.region),
            total_gb: server_status
              .system_stats
              .as_ref()
              .map(|s| s.mem_total_gb)
              .unwrap_or(0.0),
            used_gb: server_status
              .system_stats
              .as_ref()
              .map(|s| s.mem_used_gb)
              .unwrap_or(0.0),
          };
          alerts_to_update
            .push((alert, server.config.send_mem_alerts));
        }
      }
      (
        SeverityLevel::Warning | SeverityLevel::Critical,
        Some(alert),
        _,
        false,
      ) => {
        alerts_to_close
          .push((alert.clone(), server.config.send_mem_alerts));
      }
      (SeverityLevel::Ok, Some(alert), true, _) => {
        let mut alert = alert.clone();
        alert.data = AlertData::ServerMem {
          id: server_status.id.clone(),
          name: server.name.clone(),
          region: optional_string(&server.config.region),
          total_gb: server_status
            .system_stats
            .as_ref()
            .map(|s| s.mem_total_gb)
            .unwrap_or(0.0),
          used_gb: server_status
            .system_stats
            .as_ref()
            .map(|s| s.mem_used_gb)
            .unwrap_or(0.0),
        };
        alerts_to_close.push((alert, server.config.send_mem_alerts))
      }
      (SeverityLevel::Ok, _, _, _) => buffer
        .reset(server_status.id.clone(), AlertDataVariant::ServerMem),
    }

    // ===================
    // SERVER DISK
    // ===================

    let server_disk_alerts = open_disk_alerts
      .get(&ResourceTarget::Server(server_status.id.clone()));

    for (path, health) in &health.disks {
      let disk_alert = server_disk_alerts
        .as_ref()
        .and_then(|alerts| alerts.get(path))
        .cloned();
      match (
        health.level,
        disk_alert,
        health.should_close_alert,
        server.config.send_disk_alerts,
      ) {
        (
          SeverityLevel::Warning | SeverityLevel::Critical,
          None,
          _,
          false,
        ) => {}
        (
          SeverityLevel::Warning | SeverityLevel::Critical,
          None,
          _,
          true,
        ) => {
          // Only open disk alert if disk alerts enabled and not in maintenance and buffer is ready
          if !in_maintenance
            && buffer.ready_to_open(
              server_status.id.clone(),
              AlertDataVariant::ServerDisk,
            )
          {
            let disk =
              server_status.system_stats.as_ref().and_then(|stats| {
                stats.disks.iter().find(|disk| disk.mount == *path)
              });
            let alert = Alert {
              id: Default::default(),
              ts,
              resolved: false,
              resolved_ts: None,
              level: health.level,
              target: ResourceTarget::Server(
                server_status.id.clone(),
              ),
              data: AlertData::ServerDisk {
                id: server_status.id.clone(),
                name: server.name.clone(),
                region: optional_string(&server.config.region),
                path: path.to_owned(),
                total_gb: disk
                  .map(|d| d.total_gb)
                  .unwrap_or_default(),
                used_gb: disk.map(|d| d.used_gb).unwrap_or_default(),
              },
            };
            alerts_to_open
              .push((alert, server.config.send_disk_alerts));
          }
        }
        (
          SeverityLevel::Warning | SeverityLevel::Critical,
          Some(mut alert),
          _,
          true,
        ) => {
          // modify alert level only if it has increased and not in maintenance
          if !in_maintenance && health.level < alert.level {
            let disk =
              server_status.system_stats.as_ref().and_then(|stats| {
                stats.disks.iter().find(|disk| disk.mount == *path)
              });
            alert.level = health.level;
            alert.data = AlertData::ServerDisk {
              id: server_status.id.clone(),
              name: server.name.clone(),
              region: optional_string(&server.config.region),
              path: path.to_owned(),
              total_gb: disk.map(|d| d.total_gb).unwrap_or_default(),
              used_gb: disk.map(|d| d.used_gb).unwrap_or_default(),
            };
            alerts_to_update
              .push((alert, server.config.send_disk_alerts));
          }
        }
        (
          SeverityLevel::Warning | SeverityLevel::Critical,
          Some(alert),
          _,
          false,
        ) => {
          alerts_to_close
            .push((alert, server.config.send_disk_alerts));
        }
        (SeverityLevel::Ok, Some(mut alert), true, _) => {
          let disk =
            server_status.system_stats.as_ref().and_then(|stats| {
              stats.disks.iter().find(|disk| disk.mount == *path)
            });
          alert.level = health.level;
          alert.data = AlertData::ServerDisk {
            id: server_status.id.clone(),
            name: server.name.clone(),
            region: optional_string(&server.config.region),
            path: path.to_owned(),
            total_gb: disk.map(|d| d.total_gb).unwrap_or_default(),
            used_gb: disk.map(|d| d.used_gb).unwrap_or_default(),
          };
          alerts_to_close
            .push((alert, server.config.send_disk_alerts))
        }
        (SeverityLevel::Ok, _, _, _) => buffer.reset(
          server_status.id.clone(),
          AlertDataVariant::ServerDisk,
        ),
      }
    }

    // Need to close any open ones on disks no longer reported
    if let Some(disk_alerts) = server_disk_alerts {
      for (path, alert) in disk_alerts {
        if !health.disks.contains_key(path) {
          let mut alert = alert.clone();
          alert.level = SeverityLevel::Ok;
          alerts_to_close
            .push((alert, server.config.send_disk_alerts));
        }
      }
    }
  }

  tokio::join!(
    open_new_alerts(&alerts_to_open),
    update_alerts(&alerts_to_update),
    resolve_alerts(&alerts_to_close),
  );
}

/// How long the agent must fail to list containers before the
/// ServerPodmanUnresponsive alert opens. An image prune holds
/// podman's storage lock while it deletes: removing a 1.4 GB image
/// held it for 63 s, and a prune that removes several large images
/// takes longer. Five minutes is several times that, and still pages
/// on a podman that is really stuck. It is a duration, not a poll
/// count, so forced refreshes and `monitoring_interval` don't move it.
const PODMAN_UNRESPONSIVE_AFTER_MS: i64 = 5 * 60 * 1000;

#[derive(Debug, PartialEq)]
enum PodmanAlertAction {
  /// Open the alert. Carries when the failed listings started.
  Open(i64),
  Close,
  Nothing,
}

fn podman_alert_action(
  state: ServerState,
  failed_since: Option<i64>,
  open: bool,
  send: bool,
  ts: i64,
) -> PodmanAlertAction {
  if open && !send {
    return PodmanAlertAction::Close;
  }
  match (state, failed_since, open) {
    // An unreachable agent says nothing about podman, and its own
    // alert covers it.
    (ServerState::NotOk, _, _) => PodmanAlertAction::Nothing,
    // A listing succeeded (even an empty one), or the server was
    // disabled.
    (ServerState::Ok | ServerState::Disabled, None, true) => {
      PodmanAlertAction::Close
    }
    (ServerState::Ok, Some(since), false)
      if send && ts - since >= PODMAN_UNRESPONSIVE_AFTER_MS =>
    {
      PodmanAlertAction::Open(since)
    }
    _ => PodmanAlertAction::Nothing,
  }
}

async fn open_new_alerts(alerts: &[(Alert, SendAlerts)]) {
  if alerts.is_empty() {
    return;
  }

  let db = db_client();

  let open = || async {
    let ids = db
      .alerts
      .insert_many(alerts.iter().map(|(alert, _)| alert))
      .await?
      .inserted_ids
      .into_iter()
      .filter_map(|(index, id)| {
        alerts.get(index)?.1.then(|| id.as_object_id())
      })
      .flatten()
      .collect::<Vec<_>>();
    anyhow::Ok(ids)
  };

  let ids_to_send = match open().await {
    Ok(ids) => ids,
    Err(e) => {
      error!("failed to open alerts on db | {e:?}");
      return;
    }
  };

  let alerts = match find_collect(
    &db.alerts,
    doc! { "_id": { "$in": ids_to_send } },
    None,
  )
  .await
  {
    Ok(alerts) => alerts,
    Err(e) => {
      error!("failed to pull created alerts from mongo | {e:?}");
      return;
    }
  };

  send_alerts(&alerts).await
}

async fn update_alerts(alerts: &[(Alert, SendAlerts)]) {
  if alerts.is_empty() {
    return;
  }

  let open = || async {
    let updates = alerts.iter().map(|(alert, _)| {
        let update = BulkUpdate {
          query: doc! { "_id": ObjectId::from_str(&alert.id).context("failed to convert alert id to ObjectId")? },
          update: doc! { "$set": to_bson(alert).context("failed to convert alert to bson")? }
        };
        anyhow::Ok(update)
      })
      .filter_map(|update| match update {
        Ok(update) => Some(update),
        Err(e) => {
          warn!("failed to generate bulk update for alert | {e:#}");
          None
        }
      }).collect::<Vec<_>>();

    bulk_update::bulk_update(
      &db_client().db,
      Alert::default_collection_name(),
      &updates,
      false,
    )
    .await
    .context("failed to bulk update alerts")?;

    anyhow::Ok(())
  };

  let alerts = alerts
    .iter()
    .filter(|(_, send)| *send)
    .map(|(alert, _)| alert)
    .cloned()
    .collect::<Vec<_>>();

  let (res, _) = tokio::join!(open(), send_alerts(&alerts));

  if let Err(e) = res {
    error!("failed to create alerts on db | {e:#}");
  }
}

async fn resolve_alerts(alerts: &[(Alert, SendAlerts)]) {
  if alerts.is_empty() {
    return;
  }

  let close = || async move {
    let alert_ids = alerts
      .iter()
      .map(|(alert, _)| {
        ObjectId::from_str(&alert.id)
          .context("failed to convert alert id to ObjectId")
      })
      .collect::<anyhow::Result<Vec<_>>>()?;

    db_client()
      .alerts
      .update_many(
        doc! { "_id": { "$in": &alert_ids } },
        doc! {
          "$set": {
            "resolved": true,
            "resolved_ts": komodo_timestamp()
          }
        },
      )
      .await
      .context("failed to resolve alerts on db")
      .inspect_err(|e| warn!("{e:#}"))
      .ok();

    let ts = komodo_timestamp();

    let closed = alerts
      .iter()
      .filter(|(_, send)| *send)
      .map(|(alert, _)| {
        let mut alert = alert.clone();

        alert.resolved = true;
        alert.resolved_ts = Some(ts);
        alert.level = SeverityLevel::Ok;

        alert
      })
      .collect::<Vec<_>>();

    send_alerts(&closed).await;

    anyhow::Ok(())
  };

  if let Err(e) = close().await {
    error!("failed to resolve alerts | {e:#?}");
  }
}

async fn get_open_alerts()
-> anyhow::Result<(OpenAlertMap, OpenDiskAlertMap)> {
  let alerts = find_collect(
    &db_client().alerts,
    doc! { "resolved": false },
    None,
  )
  .await
  .context("failed to get open alerts from db")?;

  let mut map = OpenAlertMap::new();
  let mut disk_map = OpenDiskAlertMap::new();

  for alert in alerts {
    match &alert.data {
      AlertData::ServerDisk { path, .. } => {
        let inner = disk_map.entry(alert.target.clone()).or_default();
        inner.insert(path.to_owned(), alert);
      }
      _ => {
        let inner = map.entry(alert.target.clone()).or_default();
        inner.insert((&alert.data).into(), alert);
      }
    }
  }

  Ok((map, disk_map))
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::monitor::helpers::containers_failed_since;

  const POLL_MS: i64 = 15_000;

  /// Feed polls through the streak tracker and the alert decision,
  /// the way the monitor loop does. Each poll is (state, listed).
  /// Returns the action after each poll, opening and closing the
  /// alert as the loop would.
  fn run(polls: &[(ServerState, bool)]) -> Vec<PodmanAlertAction> {
    let mut since = None;
    let mut open = false;
    polls
      .iter()
      .enumerate()
      .map(|(i, &(state, listed))| {
        let now = i as i64 * POLL_MS;
        since = containers_failed_since(state, listed, since, now);
        let action =
          podman_alert_action(state, since, open, true, now);
        match action {
          PodmanAlertAction::Open(_) => open = true,
          PodmanAlertAction::Close => open = false,
          PodmanAlertAction::Nothing => {}
        }
        action
      })
      .collect()
  }

  fn failing(n: usize) -> Vec<(ServerState, bool)> {
    vec![(ServerState::Ok, false); n]
  }

  #[test]
  fn prune_length_failure_does_not_alert() {
    // The 63 s prune on 2026-11-25 failed a few polls in a row.
    let mut polls = failing(6);
    polls.push((ServerState::Ok, true));
    assert!(
      run(&polls).iter().all(|a| *a == PodmanAlertAction::Nothing)
    );
  }

  #[test]
  fn opens_after_five_minutes_then_resolves_on_listing() {
    // 21 polls 15 s apart span 300 s.
    let mut polls = failing(21);
    polls.push((ServerState::Ok, false));
    polls.push((ServerState::Ok, true));
    let actions = run(&polls);
    assert!(
      actions[..20]
        .iter()
        .all(|a| *a == PodmanAlertAction::Nothing)
    );
    assert_eq!(actions[20], PodmanAlertAction::Open(0));
    assert_eq!(actions[21], PodmanAlertAction::Nothing);
    assert_eq!(actions[22], PodmanAlertAction::Close);
  }

  #[test]
  fn unreachable_poll_keeps_the_run() {
    let mut polls = failing(10);
    polls.push((ServerState::NotOk, false));
    polls.extend(failing(10));
    let actions = run(&polls);
    assert_eq!(actions[20], PodmanAlertAction::Open(0));
  }

  #[test]
  fn alert_stays_open_while_unreachable() {
    assert_eq!(
      podman_alert_action(ServerState::NotOk, Some(0), true, true, 0),
      PodmanAlertAction::Nothing
    );
  }

  #[test]
  fn disabled_server_or_alerts_close_it() {
    assert_eq!(
      podman_alert_action(ServerState::Disabled, None, true, true, 0),
      PodmanAlertAction::Close
    );
    assert_eq!(
      podman_alert_action(ServerState::Ok, Some(0), true, false, 0),
      PodmanAlertAction::Close
    );
    assert_eq!(
      podman_alert_action(
        ServerState::Ok,
        Some(0),
        false,
        false,
        PODMAN_UNRESPONSIVE_AFTER_MS
      ),
      PodmanAlertAction::Nothing
    );
  }
}
