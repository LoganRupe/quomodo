use std::sync::OnceLock;

use serde::Serialize;

use super::*;

pub async fn send_alert(
  url: &str,
  alert: &Alert,
) -> anyhow::Result<()> {
  let level = fmt_level(alert.level);
  let content = match &alert.data {
    AlertData::Test { id, name } => {
      let link = resource_link(ResourceTargetVariant::Alerter, id);
      format!(
        "{level} | If you see this message, then Alerter **{name}** is **working**\n{link}"
      )
    }
    AlertData::ServerUnreachable {
      id,
      name,
      region,
      err,
    } => {
      let region = fmt_region(region);
      let link = resource_link(ResourceTargetVariant::Server, id);
      match alert.level {
        SeverityLevel::Ok => {
          format!(
            "{level} | **{name}**{region} is now **connected**\n{link}"
          )
        }
        SeverityLevel::Critical => {
          let err = err
            .as_ref()
            .map(|e| format!("\n**error**: {e:#?}"))
            .unwrap_or_default();
          format!(
            "{level} | **{name}**{region} is **unreachable** ❌\n{link}{err}"
          )
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
      let link = resource_link(ResourceTargetVariant::Server, id);
      match alert.level {
        SeverityLevel::Ok => {
          format!(
            "{level} | **{name}**{region} | Periphery version now matches Core version ✅\n{link}"
          )
        }
        _ => {
          format!(
            "{level} | **{name}**{region} | Version mismatch detected ⚠️\nPeriphery: **{server_version}** | Core: **{core_version}**\n{link}"
          )
        }
      }
    }
    AlertData::ServerCpu {
      id,
      name,
      region,
      percentage,
    } => {
      let region = fmt_region(region);
      let link = resource_link(ResourceTargetVariant::Server, id);
      format!(
        "{level} | **{name}**{region} cpu usage at **{percentage:.1}%**\n{link}"
      )
    }
    AlertData::ServerMem {
      id,
      name,
      region,
      used_gb,
      total_gb,
    } => {
      let region = fmt_region(region);
      let link = resource_link(ResourceTargetVariant::Server, id);
      let percentage = 100.0 * used_gb / total_gb;
      format!(
        "{level} | **{name}**{region} memory usage at **{percentage:.1}%** 💾\n\nUsing **{used_gb:.1} GiB** / **{total_gb:.1} GiB**\n{link}"
      )
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
      let link = resource_link(ResourceTargetVariant::Server, id);
      let percentage = 100.0 * used_gb / total_gb;
      format!(
        "{level} | **{name}**{region} disk usage at **{percentage:.1}%** 💿\nmount point: `{path:?}`\nusing **{used_gb:.1} GiB** / **{total_gb:.1} GiB**\n{link}"
      )
    }
    AlertData::StackStateChange {
      id,
      name,
      server_id: _server_id,
      server_name,
      from,
      to,
    } => {
      let link = resource_link(ResourceTargetVariant::Stack, id);
      let to = fmt_stack_state(to);
      let target = if let Some(server) = server_name {
        format!("\nserver: **{server}**")
      } else {
        String::new()
      };
      format!(
        "🥞 Stack **{name}** is now {to}{target}\nprevious: **{from}**\n{link}"
      )
    }
    AlertData::ResourceSyncPendingUpdates { id, name } => {
      let link =
        resource_link(ResourceTargetVariant::ResourceSync, id);
      format!(
        "{level} | Pending resource sync updates on **{name}**\n{link}"
      )
    }
    AlertData::ProcedureFailed { id, name } => {
      let link = resource_link(ResourceTargetVariant::Procedure, id);
      format!("{level} | Procedure **{name}** failed\n{link}")
    }
    AlertData::ActionFailed { id, name } => {
      let link = resource_link(ResourceTargetVariant::Action, id);
      format!("{level} | Action **{name}** failed\n{link}")
    }
    AlertData::ScheduleRun {
      resource_type,
      id,
      name,
    } => {
      let link = resource_link(*resource_type, id);
      format!(
        "{level} | **{name}** ({resource_type}) | Scheduled run started 🕝\n{link}"
      )
    }
    AlertData::Custom { message, details } => {
      format!(
        "{level} | {message}{}",
        if details.is_empty() {
          String::new()
        } else {
          format!("\n{details}")
        }
      )
    }
    AlertData::None {} => Default::default(),
  };

  if content.is_empty() {
    return Ok(());
  }

  send_message(url, &content).await
}

async fn send_message(
  url: &str,
  content: &str,
) -> anyhow::Result<()> {
  let body = DiscordMessageBody { content };

  let response = http_client()
    .post(url)
    .json(&body)
    .send()
    .await
    .context("Failed to send message")?;

  let status = response.status();

  if status.is_success() {
    Ok(())
  } else {
    let text = response.text().await.with_context(|| {
      format!("Failed to send message to Discord | {status} | failed to get response text")
    })?;
    Err(anyhow::anyhow!(
      "Failed to send message to Discord | {status} | {text}"
    ))
  }
}

fn http_client() -> &'static reqwest::Client {
  static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
  CLIENT.get_or_init(reqwest::Client::new)
}

#[derive(Serialize)]
struct DiscordMessageBody<'a> {
  content: &'a str,
}
