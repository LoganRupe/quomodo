use ::slack::types::OwnedBlock as Block;

use super::*;

pub async fn send_alert(
  url: &str,
  alert: &Alert,
) -> anyhow::Result<()> {
  let level = fmt_level(alert.level);
  let (text, blocks): (_, Option<_>) = match &alert.data {
    AlertData::Test { id, name } => {
      let text = format!(
        "{level} | If you see this message, then Alerter *{name}* is *working*"
      );
      let blocks = vec![
        Block::header(level),
        Block::section(format!(
          "If you see this message, then Alerter *{name}* is *working*"
        )),
        Block::section(resource_link(
          ResourceTargetVariant::Alerter,
          id,
        )),
      ];
      (text, blocks.into())
    }
    AlertData::ServerUnreachable {
      id,
      name,
      region,
      err,
    } => {
      let region = fmt_region(region);
      match alert.level {
        SeverityLevel::Ok => {
          let text =
            format!("{level} | *{name}*{region} is now *connected*");
          let blocks = vec![
            Block::header(level),
            Block::section(format!(
              "*{name}*{region} is now *connected*"
            )),
          ];
          (text, blocks.into())
        }
        SeverityLevel::Critical => {
          let text =
            format!("{level} | *{name}*{region} is *unreachable* ❌");
          let err = err
            .as_ref()
            .map(|e| format!("\nerror: {e:#?}"))
            .unwrap_or_default();
          let blocks = vec![
            Block::header(level),
            Block::section(format!(
              "*{name}*{region} is *unreachable* ❌{err}"
            )),
            Block::section(resource_link(
              ResourceTargetVariant::Server,
              id,
            )),
          ];
          (text, blocks.into())
        }
        _ => unreachable!(),
      }
    }
    AlertData::ServerVersionMismatch {
      id,
      name,
      region,
      server_version,
      core_version,
    } => {
      let region = fmt_region(region);
      let text = match alert.level {
        SeverityLevel::Ok => {
          format!(
            "{level} | *{name}*{region} | Periphery version now matches Core version ✅"
          )
        }
        _ => {
          format!(
            "{level} | *{name}*{region} | Version mismatch detected ⚠️\nPeriphery: {server_version} | Core: {core_version}"
          )
        }
      };
      let blocks = vec![
        Block::header(text.clone()),
        Block::section(resource_link(
          ResourceTargetVariant::Server,
          id,
        )),
      ];
      (text, blocks.into())
    }
    AlertData::ServerCpu {
      id,
      name,
      region,
      percentage,
    } => {
      let region = fmt_region(region);
      match alert.level {
        SeverityLevel::Ok => {
          let text = format!(
            "{level} | *{name}*{region} cpu usage at *{percentage:.1}%*"
          );
          let blocks = vec![
            Block::header(level),
            Block::section(format!(
              "*{name}*{region} cpu usage at *{percentage:.1}%*"
            )),
            Block::section(resource_link(
              ResourceTargetVariant::Server,
              id,
            )),
          ];
          (text, blocks.into())
        }
        _ => {
          let text = format!(
            "{level} | *{name}*{region} cpu usage at *{percentage:.1}%* 📈"
          );
          let blocks = vec![
            Block::header(level),
            Block::section(format!(
              "*{name}*{region} cpu usage at *{percentage:.1}%* 📈"
            )),
            Block::section(resource_link(
              ResourceTargetVariant::Server,
              id,
            )),
          ];
          (text, blocks.into())
        }
      }
    }
    AlertData::ServerMem {
      id,
      name,
      region,
      used_gb,
      total_gb,
    } => {
      let region = fmt_region(region);
      let percentage = 100.0 * used_gb / total_gb;
      match alert.level {
        SeverityLevel::Ok => {
          let text = format!(
            "{level} | *{name}*{region} memory usage at *{percentage:.1}%* 💾"
          );
          let blocks = vec![
            Block::header(level),
            Block::section(format!(
              "*{name}*{region} memory usage at *{percentage:.1}%* 💾"
            )),
            Block::section(format!(
              "using *{used_gb:.1} GiB* / *{total_gb:.1} GiB*"
            )),
            Block::section(resource_link(
              ResourceTargetVariant::Server,
              id,
            )),
          ];
          (text, blocks.into())
        }
        _ => {
          let text = format!(
            "{level} | *{name}*{region} memory usage at *{percentage:.1}%* 💾"
          );
          let blocks = vec![
            Block::header(level),
            Block::section(format!(
              "*{name}*{region} memory usage at *{percentage:.1}%* 💾"
            )),
            Block::section(format!(
              "using *{used_gb:.1} GiB* / *{total_gb:.1} GiB*"
            )),
            Block::section(resource_link(
              ResourceTargetVariant::Server,
              id,
            )),
          ];
          (text, blocks.into())
        }
      }
    }
    AlertData::ServerDisk {
      id,
      name,
      region,
      path,
      used_gb,
      total_gb,
    } => {
      let region = fmt_region(region);
      let percentage = 100.0 * used_gb / total_gb;
      match alert.level {
        SeverityLevel::Ok => {
          let text = format!(
            "{level} | *{name}*{region} disk usage at *{percentage:.1}%* | mount point: *{path:?}* 💿"
          );
          let blocks = vec![
            Block::header(level),
            Block::section(format!(
              "*{name}*{region} disk usage at *{percentage:.1}%* 💿"
            )),
            Block::section(format!(
              "mount point: {path:?} | using *{used_gb:.1} GiB* / *{total_gb:.1} GiB*"
            )),
            Block::section(resource_link(
              ResourceTargetVariant::Server,
              id,
            )),
          ];
          (text, blocks.into())
        }
        _ => {
          let text = format!(
            "{level} | *{name}*{region} disk usage at *{percentage:.1}%* | mount point: *{path:?}* 💿"
          );
          let blocks = vec![
            Block::header(level),
            Block::section(format!(
              "*{name}*{region} disk usage at *{percentage:.1}%* 💿"
            )),
            Block::section(format!(
              "mount point: {path:?} | using *{used_gb:.1} GiB* / *{total_gb:.1} GiB*"
            )),
            Block::section(resource_link(
              ResourceTargetVariant::Server,
              id,
            )),
          ];
          (text, blocks.into())
        }
      }
    }
    AlertData::StackStateChange {
      name,
      server_id: _server_id,
      server_name,
      from,
      to,
      id,
    } => {
      let to = fmt_stack_state(to);
      let text = format!("🥞 Stack *{name}* is now *{to}*");
      let target = if let Some(server) = server_name {
        format!("server: *{server}*\n")
      } else {
        String::new()
      };
      let blocks = vec![
        Block::header(text.clone()),
        Block::section(format!("{target}previous: *{from}*",)),
        Block::section(resource_link(
          ResourceTargetVariant::Stack,
          id,
        )),
      ];
      (text, blocks.into())
    }
    AlertData::ResourceSyncPendingUpdates { id, name } => {
      let text = format!(
        "{level} | Pending resource sync updates on *{name}*"
      );
      let blocks = vec![
        Block::header(text.clone()),
        Block::section(format!(
          "sync id: *{id}*\nsync name: *{name}*",
        )),
        Block::section(resource_link(
          ResourceTargetVariant::ResourceSync,
          id,
        )),
      ];
      (text, blocks.into())
    }
    AlertData::ProcedureFailed { id, name } => {
      let text = format!("{level} | Procedure *{name}* has *failed*");
      let blocks = vec![
        Block::header(text.clone()),
        Block::section(resource_link(
          ResourceTargetVariant::Procedure,
          id,
        )),
      ];
      (text, blocks.into())
    }
    AlertData::ActionFailed { id, name } => {
      let text = format!("{level} | Action *{name}* has *failed*");
      let blocks = vec![
        Block::header(text.clone()),
        Block::section(resource_link(
          ResourceTargetVariant::Action,
          id,
        )),
      ];
      (text, blocks.into())
    }
    AlertData::ScheduleRun {
      resource_type,
      id,
      name,
    } => {
      let text = format!(
        "{level} | *{name}* ({resource_type}) | Scheduled run started 🕝"
      );
      let blocks = vec![
        Block::header(text.clone()),
        Block::section(resource_link(*resource_type, id)),
      ];
      (text, blocks.into())
    }
    AlertData::Custom { message, details } => {
      let text = format!("{level} | {message}");
      let blocks =
        vec![Block::header(text.clone()), Block::section(details)];
      (text, blocks.into())
    }
    AlertData::None {} => Default::default(),
  };
  if text.is_empty() {
    return Ok(());
  }
  let slack = ::slack::Client::new(url.to_string());
  slack
    .send_owned_message_single(&text, None, blocks.as_deref())
    .await
    .map_err(|e| anyhow::Error::msg(format!("{e:?}")))
    .context("Error with request to Slack")?;
  Ok(())
}
