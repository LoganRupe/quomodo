use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, anyhow};
use async_timing_util::{Timelength, wait_until_timelength};
use data_encoding::BASE64;
use komodo_client::entities::{
  ResourceTargetVariant, report::KomodoReport,
};
use mogh_pki::{PkiKind, RotatableKeyPair, SpkiPublicKey};

use crate::{config::core_config, state::db_client};

pub fn spawn_reporting_loop() {
  let config = core_config();

  if !config.reporting_enabled {
    return;
  }

  // An X25519 key, the same algorithm as the Core / agent keys.
  let keys = match RotatableKeyPair::from_private_key_spec(
    PkiKind::Mutual,
    &config.reporting_private_key,
  ) {
    Ok(keys) => keys,
    Err(e) => {
      warn!(
        "Failed to initialize reporting key pair. Reporting is disabled. {e:#}"
      );
      return;
    }
  };

  let client = reqwest::Client::default();

  tokio::spawn(async move {
    loop {
      wait_until_timelength(Timelength::OneDay, 1100).await;
      if let Err(e) = report(&client, &keys).await {
        warn!("Reporting failed. {e:#}");
      }
    }
  });
}

async fn report(
  client: &reqwest::Client,
  keys: &RotatableKeyPair,
) -> anyhow::Result<()> {
  let db = db_client();

  let keys = keys.load();
  let private_key_bytes = keys
    .private
    .as_raw_bytes(PkiKind::Mutual)
    .context("Invalid reporting private key.")?;

  let endpoint_public_key = client
    .get("https://mogh.tech/report/public_key")
    .send()
    .await
    .context("Failed to query for reporting endpoint public key for signature")?
    .error_for_status()
    .context("Failed response for reporting endpoint public key for signature")?
    .text()
    .await
    .context("Failed to get reporting endpoint public key for signature")?;

  let endpoint_public_key = SpkiPublicKey::maybe_pem_to_raw_bytes(
    PkiKind::Mutual,
    &endpoint_public_key,
  )
  .context("Invalid reporting endpoint public key.")?;

  let users = db
    .users
    .count_documents(Default::default())
    .await
    .context("Failed to query database for users")?;
  let servers = db
    .servers
    .count_documents(Default::default())
    .await
    .context("Failed to query database for servers")?;
  let stacks = db
    .stacks
    .count_documents(Default::default())
    .await
    .context("Failed to query database for stacks")?;

  let count = [
    (ResourceTargetVariant::Server, servers),
    (ResourceTargetVariant::Stack, stacks),
  ]
  .into_iter()
  .collect();

  let report = KomodoReport {
    public_key: keys.public().to_string(),
    version: String::from(env!("CARGO_PKG_VERSION")),
    users,
    count,
  };

  let serialized = serde_json::to_string(&report)
    .context("Failed to serialize report JSON")?;

  let timestamp =
    SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis() as i64;

  let prologue =
    format!("POST|/report/komodo|{serialized}|{timestamp}");

  let signature = one_way_signature(
    &private_key_bytes,
    &endpoint_public_key,
    prologue.as_bytes(),
  )
  .context("Failed to generate report signature")?;

  let res = client
    .post("https://mogh.tech/report/komodo")
    .header("x-api-signature", signature)
    .header("x-api-timestamp", timestamp)
    .header("content-type", "application/json")
    .body(serialized)
    .send()
    .await
    .context("Failed to post report")?;

  let status = res.status();

  if status.is_success() {
    return Ok(());
  }

  let text = res
    .text()
    .await
    .context("Failed report, and failed to get response body")?;

  Err(anyhow!("{status}: {text}"))
}

/// The first message of a Noise IK handshake with the endpoint, which
/// the endpoint reads to verify the report. mogh_pki dropped this
/// handshake in 3.0, so it is made here.
fn one_way_signature(
  private_key: &[u8],
  remote_public_key: &[u8],
  prologue: &[u8],
) -> anyhow::Result<String> {
  let mut handshake =
    snow::Builder::new("Noise_IK_25519_ChaChaPoly_BLAKE2s".parse()?)
      .local_private_key(private_key)
      .context("Invalid private key")?
      .remote_public_key(remote_public_key)
      .context("Invalid remote public key")?
      .prologue(prologue)
      .context("Invalid prologue")?
      .build_initiator()
      .context("Failed to build initiator")?;
  let mut buf = [0u8; 1024];
  let written = handshake.write_message(&[], &mut buf)?;
  Ok(BASE64.encode(&buf[..written]))
}
