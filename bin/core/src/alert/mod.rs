use anyhow::{Context, anyhow};
use database::mungos::{find::find_collect, mongodb::bson::doc};
use futures_util::future::join_all;
use interpolate::Interpolator;
use komodo_client::entities::{
  ResourceTargetVariant,
  alert::{Alert, AlertData, AlertDataVariant, SeverityLevel},
  alerter::*,
  komodo_timestamp,
  stack::StackState,
};

use crate::helpers::maintenance::is_in_maintenance;
use crate::{config::core_config, state::db_client};

mod discord;
mod ntfy;
mod pushover;
mod slack;

pub async fn send_alerts(alerts: &[Alert]) {
  if alerts.is_empty() {
    return;
  }

  let Ok(alerters) = find_collect(
    &db_client().alerters,
    doc! { "config.enabled": true },
    None,
  )
  .await
  .inspect_err(|e| {
    error!(
      "ERROR sending alerts | failed to get alerters from db | {e:#}"
    )
  }) else {
    return;
  };

  let handles = alerts
    .iter()
    .map(|alert| send_alert_to_alerters(&alerters, alert));

  join_all(handles).await;
}

async fn send_alert_to_alerters(alerters: &[Alerter], alert: &Alert) {
  if alerters.is_empty() {
    return;
  }

  let handles = alerters
    .iter()
    .map(|alerter| send_alert_to_alerter(alerter, alert));

  join_all(handles)
    .await
    .into_iter()
    .filter_map(|res| res.err())
    .for_each(|e| error!("{e:#}"));
}

pub async fn send_alert_to_alerter(
  alerter: &Alerter,
  alert: &Alert,
) -> anyhow::Result<()> {
  // Don't send if not enabled
  if !alerter.config.enabled {
    return Ok(());
  }

  if is_in_maintenance(
    &alerter.config.maintenance_windows,
    komodo_timestamp(),
  ) {
    return Ok(());
  }

  let alert_variant: AlertDataVariant = (&alert.data).into();

  // In the test case, we don't want the filters inside this
  // block to stop the test from being sent to the alerting endpoint.
  if alert_variant != AlertDataVariant::Test {
    // Don't send if alert type not configured on the alerter
    if !alerter.config.alert_types.is_empty()
      && !alerter.config.alert_types.contains(&alert_variant)
    {
      return Ok(());
    }

    // Don't send if resource is in the blacklist
    if alerter.config.except_resources.contains(&alert.target) {
      return Ok(());
    }

    // Don't send if whitelist configured and target is not included
    if !alerter.config.resources.is_empty()
      && !alerter.config.resources.contains(&alert.target)
    {
      return Ok(());
    }
  }

  let (endpoint, replacers) =
    resolve_endpoint(alerter).await.with_context(|| {
      format!(
        "Failed to resolve the endpoint of Alerter {}",
        alerter.name
      )
    })?;

  let res = match &endpoint {
    AlerterEndpoint::Custom(CustomAlerterEndpoint { url }) => {
      send_custom_alert(url, alert).await
    }
    AlerterEndpoint::Slack(SlackAlerterEndpoint { url }) => {
      slack::send_alert(url, alert).await
    }
    AlerterEndpoint::Discord(DiscordAlerterEndpoint { url }) => {
      discord::send_alert(url, alert).await
    }
    AlerterEndpoint::Ntfy(NtfyAlerterEndpoint { url, email }) => {
      ntfy::send_alert(url, email.as_deref(), alert).await
    }
    AlerterEndpoint::Pushover(PushoverAlerterEndpoint { url }) => {
      pushover::send_alert(url, alert).await
    }
  };

  // Request errors tend to include the url, and the url carries the token.
  res
    .map_err(|e| {
      anyhow!(svi::replace_in_string(&format!("{e:#}"), &replacers))
    })
    .with_context(|| {
      format!(
        "Failed to send alert to {} Alerter {}",
        AlerterEndpointVariant::from(&endpoint),
        alerter.name
      )
    })
}

/// The alerter's endpoint with `[[NAME]]` references in its url (and the
/// ntfy email) resolved from the alerter's secrets collection, plus the
/// `(value, [[NAME]])` pairs to scrub from errors.
async fn resolve_endpoint(
  alerter: &Alerter,
) -> anyhow::Result<(AlerterEndpoint, Vec<(String, String)>)> {
  let mut endpoint = alerter.config.endpoint.clone();
  let mut fields = match &mut endpoint {
    AlerterEndpoint::Custom(e) => vec![&mut e.url],
    AlerterEndpoint::Slack(e) => vec![&mut e.url],
    AlerterEndpoint::Discord(e) => vec![&mut e.url],
    AlerterEndpoint::Pushover(e) => vec![&mut e.url],
    AlerterEndpoint::Ntfy(e) => {
      let mut fields = vec![&mut e.url];
      if let Some(email) = e.email.as_mut() {
        fields.push(email);
      }
      fields
    }
  };

  let mut names = fields
    .iter()
    .flat_map(|field| quadlet::prepare::referenced_names(field))
    .collect::<Vec<_>>();
  names.sort();
  names.dedup();
  if names.is_empty() {
    return Ok((endpoint, Vec::new()));
  }

  let collection = &alerter.config.secrets_collection;
  if collection.is_empty() {
    return Err(anyhow!(
      "The endpoint references [[{}]] but the Alerter has no 'secrets_collection'",
      names.join("]], [[")
    ));
  }
  let secrets =
    crate::secrets::resolve_from_provider(collection, &names)
      .await
      .with_context(|| {
        format!(
          "Failed to resolve secrets from collection '{collection}'"
        )
      })?;

  let mut interpolator = Interpolator::new(&secrets);
  for field in fields.iter_mut() {
    interpolator.interpolate_string(field)?;
  }
  let replacers = interpolator.secret_replacers.into_iter().collect();
  drop(fields);
  Ok((endpoint, replacers))
}

async fn send_custom_alert(
  url: &str,
  alert: &Alert,
) -> anyhow::Result<()> {
  let res = reqwest::Client::new()
    .post(url)
    .json(alert)
    .send()
    .await
    .context("failed at post request to alerter")?;
  let status = res.status();
  if !status.is_success() {
    let text = res
      .text()
      .await
      .context("failed to get response text on alerter response")?;
    return Err(anyhow!(
      "post to alerter failed | {status} | {text}"
    ));
  }
  Ok(())
}

fn fmt_region(region: &Option<String>) -> String {
  match region {
    Some(region) => format!(" ({region})"),
    None => String::new(),
  }
}

fn fmt_stack_state(state: &StackState) -> String {
  match state {
    StackState::Running => String::from("Running ▶️"),
    StackState::Stopped => String::from("Stopped 🛑"),
    StackState::Restarting => String::from("Restarting 🔄"),
    StackState::Down => String::from("Down ⬇️"),
    _ => state.to_string(),
  }
}

fn fmt_level(level: SeverityLevel) -> &'static str {
  match level {
    SeverityLevel::Critical => "CRITICAL 🚨",
    SeverityLevel::Warning => "WARNING ‼️",
    SeverityLevel::Ok => "OK ✅",
  }
}

fn resource_link(
  resource_type: ResourceTargetVariant,
  id: &str,
) -> String {
  komodo_client::entities::resource_link(
    &core_config().host,
    resource_type,
    id,
  )
}

/// Standard message content format
/// used by Ntfy, Pushover.
fn standard_alert_content(alert: &Alert) -> String {
  let level = fmt_level(alert.level);
  match &alert.data {
    AlertData::Test { id, name } => {
      let link = resource_link(ResourceTargetVariant::Alerter, id);
      format!(
        "{level} | If you see this message, then Alerter {name} is working\n{link}",
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
          format!("{level} | {name}{region} is now connected\n{link}")
        }
        SeverityLevel::Critical => {
          let err = err
            .as_ref()
            .map(|e| format!("\nerror: {e:#?}"))
            .unwrap_or_default();
          format!(
            "{level} | {name}{region} is unreachable ❌\n{link}{err}"
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
            "{level} | {name}{region} | Periphery version now matches Core version ✅\n{link}"
          )
        }
        _ => {
          format!(
            "{level} | {name}{region} | Version mismatch detected ⚠️\nPeriphery: {server_version} | Core: {core_version}\n{link}"
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
        "{level} | {name}{region} cpu usage at {percentage:.1}%\n{link}",
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
        "{level} | {name}{region} memory usage at {percentage:.1}%💾\n\nUsing {used_gb:.1} GiB / {total_gb:.1} GiB\n{link}",
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
        "{level} | {name}{region} disk usage at {percentage:.1}%💿\nmount point: {path:?}\nusing {used_gb:.1} GiB / {total_gb:.1} GiB\n{link}",
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
      let to_state = fmt_stack_state(to);
      let target = if let Some(server) = server_name {
        format!("\nserver: {server}")
      } else {
        String::new()
      };
      format!(
        "🥞 Stack {name} is now {to_state}{target}\nprevious: {from}\n{link}",
      )
    }
    AlertData::ResourceSyncPendingUpdates { id, name } => {
      let link =
        resource_link(ResourceTargetVariant::ResourceSync, id);
      format!(
        "{level} | Pending resource sync updates on {name}\n{link}",
      )
    }
    AlertData::ProcedureFailed { id, name } => {
      let link = resource_link(ResourceTargetVariant::Procedure, id);
      format!("{level} | Procedure {name} failed\n{link}")
    }
    AlertData::ActionFailed { id, name } => {
      let link = resource_link(ResourceTargetVariant::Action, id);
      format!("{level} | Action {name} failed\n{link}")
    }
    AlertData::ScheduleRun {
      resource_type,
      id,
      name,
    } => {
      let link = resource_link(*resource_type, id);
      format!(
        "{level} | {name} ({resource_type}) | Scheduled run started 🕝\n{link}"
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
  }
}

#[cfg(test)]
mod tests {
  use komodo_client::entities::alerter::{
    Alerter, AlerterConfig, AlerterEndpoint, CustomAlerterEndpoint,
    NtfyAlerterEndpoint,
  };

  use super::resolve_endpoint;

  fn alerter(endpoint: AlerterEndpoint, collection: &str) -> Alerter {
    Alerter {
      name: "test".to_string(),
      config: AlerterConfig {
        endpoint,
        secrets_collection: collection.to_string(),
        ..Default::default()
      },
      ..Default::default()
    }
  }

  #[tokio::test]
  async fn endpoint_without_references_is_used_as_is() {
    let endpoint = AlerterEndpoint::Custom(CustomAlerterEndpoint {
      url: "https://hooks.example.com/plain".to_string(),
    });
    let (resolved, replacers) =
      resolve_endpoint(&alerter(endpoint.clone(), ""))
        .await
        .unwrap();
    assert_eq!(resolved, endpoint);
    assert!(replacers.is_empty());
  }

  #[tokio::test]
  async fn reference_without_collection_is_an_error() {
    let endpoint = AlerterEndpoint::Ntfy(NtfyAlerterEndpoint {
      url: "https://ntfy.example.com/alerts".to_string(),
      email: Some("[[NTFY_EMAIL]]".to_string()),
    });
    let err = resolve_endpoint(&alerter(endpoint, ""))
      .await
      .unwrap_err()
      .to_string();
    assert!(err.contains("[[NTFY_EMAIL]]"), "{err}");
    assert!(err.contains("secrets_collection"), "{err}");
  }
}
