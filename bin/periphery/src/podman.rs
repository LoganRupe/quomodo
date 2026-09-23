//! Thin wrappers over the podman CLI.
//! The agent runs as root against the rootful podman engine.

use std::{collections::HashMap, time::Duration};

use anyhow::{Context, anyhow};
use command::{
  CommandOptions, run_komodo_shell_command,
  run_komodo_standard_command, run_standard_command,
};
use komodo_client::entities::{
  SearchCombinator,
  docker::container::{
    ContainerListItem, ContainerStateStatusEnum, Port, PortTypeEnum,
  },
  update::Log,
};
use serde::Deserialize;
use shell_escape::unix::escape;

use crate::helpers::format_log_grep;

/// The label quadlet puts on every container it manages,
/// holding the systemd service name (eg `web.service`).
pub const SYSTEMD_UNIT_LABEL: &str = "PODMAN_SYSTEMD_UNIT";

pub async fn version() -> anyhow::Result<String> {
  let output = run_standard_command(
    "podman version --format {{.Client.Version}}",
    CommandOptions::default().timeout(Duration::from_secs(10)),
  )
  .await;
  if output.success() {
    Ok(output.stdout.trim().to_string())
  } else {
    Err(anyhow!(
      "podman version failed | stdout: {} | stderr: {}",
      output.stdout.trim(),
      output.stderr.trim()
    ))
  }
}

// ============
//  CONTAINERS
// ============

#[derive(Deserialize)]
#[serde(untagged)]
enum Created {
  Unix(i64),
  Rfc3339(String),
}

/// Podman emits port keys in snake_case, unlike the rest of the item.
#[derive(Deserialize)]
struct PsPort {
  #[serde(default)]
  host_ip: String,
  #[serde(default)]
  container_port: u16,
  #[serde(default)]
  host_port: u16,
  #[serde(default)]
  protocol: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct PsItem {
  #[serde(default)]
  id: String,
  #[serde(default)]
  names: Vec<String>,
  #[serde(default)]
  image: String,
  #[serde(default, rename = "ImageID")]
  image_id: String,
  #[serde(default)]
  created: Option<Created>,
  #[serde(default)]
  state: String,
  #[serde(default)]
  status: String,
  #[serde(default)]
  labels: Option<HashMap<String, String>>,
  #[serde(default)]
  networks: Option<Vec<String>>,
  #[serde(default)]
  ports: Option<Vec<PsPort>>,
  #[serde(default)]
  mounts: Option<Vec<String>>,
  #[serde(default)]
  pod_name: Option<String>,
}

/// `podman ps -a --format json`
pub async fn list_containers()
-> anyhow::Result<Vec<ContainerListItem>> {
  let output = run_standard_command(
    "podman ps -a --no-trunc --format json",
    CommandOptions::default().timeout(Duration::from_secs(20)),
  )
  .await;
  if !output.success() {
    return Err(anyhow!(
      "podman ps failed | stdout: {} | stderr: {}",
      output.stdout.trim(),
      output.stderr.trim()
    ));
  }
  parse_ps_json(&output.stdout)
}

fn parse_ps_json(
  json: &str,
) -> anyhow::Result<Vec<ContainerListItem>> {
  let items: Vec<PsItem> = serde_json::from_str(json.trim())
    .context("Failed to parse podman ps json")?;
  let mut containers = items
    .into_iter()
    .map(|item| {
      let name = item.names.into_iter().next().unwrap_or_default();
      let created = match item.created {
        Some(Created::Unix(ts)) => Some(ts),
        Some(Created::Rfc3339(s)) => {
          chrono::DateTime::parse_from_rfc3339(&s)
            .ok()
            .map(|dt| dt.timestamp())
        }
        None => None,
      };
      let labels = item.labels.unwrap_or_default();
      let unit = labels.get(SYSTEMD_UNIT_LABEL).cloned();
      let ports = item
        .ports
        .unwrap_or_default()
        .into_iter()
        .map(|p| Port {
          ip: (!p.host_ip.is_empty()).then_some(p.host_ip),
          private_port: p.container_port,
          public_port: (p.host_port != 0).then_some(p.host_port),
          typ: match p.protocol.as_str() {
            "tcp" => PortTypeEnum::TCP,
            "udp" => PortTypeEnum::UDP,
            "sctp" => PortTypeEnum::SCTP,
            _ => PortTypeEnum::EMPTY,
          },
        })
        .collect();
      ContainerListItem {
        server_id: None,
        server_name: None,
        name,
        id: (!item.id.is_empty()).then_some(item.id),
        image: (!item.image.is_empty()).then_some(item.image),
        image_id: (!item.image_id.is_empty())
          .then_some(item.image_id),
        created,
        state: item
          .state
          .parse::<ContainerStateStatusEnum>()
          .unwrap_or_default(),
        status: (!item.status.is_empty()).then_some(item.status),
        unit,
        pod: item.pod_name.filter(|p| !p.is_empty()),
        networks: item.networks.unwrap_or_default(),
        ports,
        volumes: item.mounts.unwrap_or_default(),
      }
    })
    .collect::<Vec<_>>();
  containers.sort_by(|a, b| a.name.cmp(&b.name));
  Ok(containers)
}

pub async fn container_log(
  name: &str,
  tail: u64,
  timestamps: bool,
) -> Log {
  let timestamps = if timestamps { " --timestamps" } else { "" };
  run_komodo_standard_command(
    "Get Container Log",
    format!(
      "podman logs --tail {tail}{timestamps} -- {}",
      escape(name.into())
    ),
    CommandOptions::default().timeout(Duration::from_secs(10)),
  )
  .await
}

pub async fn container_log_search(
  name: &str,
  terms: &[String],
  combinator: SearchCombinator,
  invert: bool,
  timestamps: bool,
) -> Log {
  let timestamps = if timestamps { " --timestamps" } else { "" };
  let grep = format_log_grep(terms, combinator, invert);
  run_komodo_shell_command(
    "Search Container Log",
    format!(
      "podman logs --tail 5000{timestamps} -- {} 2>&1 | {grep}",
      escape(name.into())
    ),
    CommandOptions::default().timeout(Duration::from_secs(10)),
  )
  .await
}

// ========
//  IMAGES
// ========

/// Returns whether login was actually performed.
#[instrument("PodmanLogin", skip(registry_token))]
pub async fn login(
  domain: &str,
  account: &str,
  // For token override from core.
  registry_token: Option<&str>,
) -> anyhow::Result<bool> {
  if domain.is_empty() || account.is_empty() {
    return Ok(false);
  }

  let registry_token = match registry_token {
    Some(token) => token,
    None => crate::helpers::registry_token(domain, account)?,
  };

  // The token is written to the child's stdin rather than interpolated into
  // the command, so it never reaches the process arguments, where any user
  // on the host could read it out of `ps`. This runs without a shell, and
  // `--` keeps a domain beginning with `-` from being parsed as a flag.
  let log = run_standard_command(
    &format!(
      "podman login --username {} --password-stdin -- {}",
      escape(account.into()),
      escape(domain.into()),
    ),
    CommandOptions::default().stdin(registry_token),
  )
  .await;

  if log.success() {
    return Ok(true);
  }

  let mut e = anyhow!("End of trace");
  for line in
    log.stderr.split('\n').filter(|line| !line.is_empty()).rev()
  {
    e = e.context(line.to_string());
  }
  for line in
    log.stdout.split('\n').filter(|line| !line.is_empty()).rev()
  {
    e = e.context(line.to_string());
  }
  Err(e.context(format!("Registry {domain} login error")))
}

#[instrument("PodmanPull")]
pub async fn pull(image: &str) -> Log {
  run_komodo_standard_command(
    "Pull Image",
    format!("podman pull -- {}", escape(image.into())),
    CommandOptions::default(),
  )
  .await
}

// =========
//  SECRETS
// =========

/// Create or replace a podman secret. The value goes over stdin,
/// never through the process arguments.
#[instrument("PodmanSecretCreate", skip(value))]
pub async fn secret_create(
  name: &str,
  value: &str,
) -> anyhow::Result<()> {
  let output = run_standard_command(
    &format!(
      "podman secret create --replace -- {} -",
      escape(name.into())
    ),
    CommandOptions::default()
      .stdin(value)
      .timeout(Duration::from_secs(20)),
  )
  .await;
  if output.success() {
    Ok(())
  } else {
    Err(anyhow!(
      "podman secret create {name} failed | stdout: {} | stderr: {}",
      output.stdout.trim(),
      output.stderr.trim()
    ))
  }
}

#[instrument("PodmanSecretRm")]
pub async fn secret_rm(name: &str) -> Log {
  run_komodo_standard_command(
    "Remove Secret",
    format!("podman secret rm -- {}", escape(name.into())),
    CommandOptions::default().timeout(Duration::from_secs(20)),
  )
  .await
}

#[derive(Deserialize)]
struct SecretLsItem {
  #[serde(default, rename = "Name")]
  name: String,
}

pub async fn secret_names() -> anyhow::Result<Vec<String>> {
  let output = run_standard_command(
    "podman secret ls --format json",
    CommandOptions::default().timeout(Duration::from_secs(20)),
  )
  .await;
  if !output.success() {
    return Err(anyhow!(
      "podman secret ls failed | stdout: {} | stderr: {}",
      output.stdout.trim(),
      output.stderr.trim()
    ));
  }
  let items: Vec<SecretLsItem> =
    serde_json::from_str(output.stdout.trim())
      .context("Failed to parse podman secret ls json")?;
  Ok(items.into_iter().map(|i| i.name).collect())
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn parses_podman_ps_json() {
    let json = r#"[
      {
        "AutoRemove": false,
        "Command": ["nginx", "-g", "daemon off;"],
        "Created": "2026-09-23T10:00:00.123456789+10:00",
        "CreatedAt": "",
        "ExitCode": 0,
        "Exited": false,
        "Id": "abc123",
        "Image": "docker.io/library/nginx:latest",
        "ImageID": "img456",
        "IsInfra": false,
        "Labels": { "PODMAN_SYSTEMD_UNIT": "web.service" },
        "Mounts": ["/srv/www"],
        "Names": ["systemd-web"],
        "Networks": ["app"],
        "Pid": 100,
        "Pod": "",
        "PodName": "",
        "Ports": [{ "host_ip": "", "container_port": 80, "host_port": 8080, "range": 1, "protocol": "tcp" }],
        "State": "running",
        "Status": "Up 2 hours"
      },
      {
        "Created": 1758585600,
        "Id": "def",
        "Image": "docker.io/library/postgres:16",
        "ImageID": "img789",
        "Names": ["systemd-db"],
        "Pod": "podid",
        "PodName": "systemd-app",
        "State": "exited",
        "Status": "Exited (0) 5 minutes ago"
      }
    ]"#;
    let containers = parse_ps_json(json).unwrap();
    assert_eq!(containers.len(), 2);
    let db = &containers[0];
    assert_eq!(db.name, "systemd-db");
    assert_eq!(db.state, ContainerStateStatusEnum::Exited);
    assert_eq!(db.pod.as_deref(), Some("systemd-app"));
    assert_eq!(db.created, Some(1758585600));
    assert!(db.unit.is_none());
    let web = &containers[1];
    assert_eq!(web.name, "systemd-web");
    assert_eq!(web.unit.as_deref(), Some("web.service"));
    assert_eq!(web.state, ContainerStateStatusEnum::Running);
    assert_eq!(web.ports.len(), 1);
    assert_eq!(web.ports[0].public_port, Some(8080));
    assert_eq!(web.ports[0].private_port, 80);
    assert_eq!(web.ports[0].typ, PortTypeEnum::TCP);
    assert_eq!(web.networks, vec!["app".to_string()]);
    assert_eq!(web.created, Some(1790121600));
  }
}
