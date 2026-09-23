//! Thin wrappers over systemctl / journalctl for quadlet units.

use std::time::Duration;

use command::{
  CommandOptions, run_komodo_standard_command, run_standard_command,
};
use komodo_client::entities::update::Log;
use shell_escape::unix::escape;

pub async fn daemon_reload() -> Log {
  run_komodo_standard_command(
    "Reload systemd",
    "systemctl daemon-reload",
    CommandOptions::default().timeout(Duration::from_secs(60)),
  )
  .await
}

/// `systemctl <action> <units...>`. No-op with an empty list.
pub async fn systemctl(stage: &str, action: &str, units: &[String]) -> Log {
  if units.is_empty() {
    return Log::simple(stage, String::from("No units to act on"));
  }
  let units = units
    .iter()
    .map(|u| escape(u.into()))
    .collect::<Vec<_>>()
    .join(" ");
  run_komodo_standard_command(
    stage,
    format!("systemctl {action} -- {units}"),
    CommandOptions::default().timeout(Duration::from_secs(600)),
  )
  .await
}

pub async fn is_active(unit: &str) -> bool {
  run_standard_command(
    &format!("systemctl is-active --quiet -- {}", escape(unit.into())),
    CommandOptions::default().timeout(Duration::from_secs(10)),
  )
  .await
  .success()
}

pub async fn journal_tail(unit: &str, lines: u32) -> String {
  let output = run_standard_command(
    &format!(
      "journalctl --no-pager -n {lines} -u {}",
      escape(unit.into())
    ),
    CommandOptions::default().timeout(Duration::from_secs(10)),
  )
  .await;
  if output.success() {
    output.stdout
  } else {
    format!("{}\n{}", output.stdout, output.stderr)
  }
}

/// Wait up to `timeout` for every unit to report active.
/// Returns the units which never became active.
pub async fn wait_active(
  units: &[String],
  timeout: Duration,
) -> Vec<String> {
  let start = std::time::Instant::now();
  let mut pending = units.to_vec();
  loop {
    let mut still = Vec::new();
    for unit in pending {
      if !is_active(&unit).await {
        still.push(unit);
      }
    }
    pending = still;
    if pending.is_empty() || start.elapsed() > timeout {
      return pending;
    }
    tokio::time::sleep(Duration::from_secs(2)).await;
  }
}
