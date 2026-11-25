use std::time::Duration;

use anyhow::Context;
use async_timing_util::{
  ONE_DAY_MS, ONE_HOUR_MS, Timelength, unix_timestamp_ms,
  wait_until_timelength,
};
use database::mungos::{find::find_collect, mongodb::bson::doc};
use futures_util::{StreamExt, stream::FuturesUnordered};
use periphery_client::api::PruneImages;

use crate::{config::core_config, state::db_client};

use super::periphery_client;

pub fn spawn_prune_loop() {
  tokio::spawn(async move {
    loop {
      wait_until_timelength(Timelength::OneDay, 5000).await;
      let (images_res, stats_res, alerts_res) =
        tokio::join!(prune_images(), prune_stats(), prune_alerts());
      if let Err(e) = images_res {
        error!("error in pruning images | {e:#}");
      }
      if let Err(e) = stats_res {
        error!("error in pruning stats | {e:#}");
      }
      if let Err(e) = alerts_res {
        error!("error in pruning alerts | {e:#}");
      }
    }
  });
}

async fn prune_images() -> anyhow::Result<()> {
  let mut futures = find_collect(
    &db_client().servers,
    doc! { "config.enabled": true, "config.auto_prune": true },
    None,
  )
  .await
  .context("failed to get servers from db")?
  .into_iter()
  .map(|server| async move {
    tokio::time::sleep(prune_offset(&server.id)).await;
    (
      async {
        periphery_client(&server)
          .await?
          .request(PruneImages {})
          .await
      }
      .await,
      server,
    )
  })
  .collect::<FuturesUnordered<_>>();

  while let Some((res, server)) = futures.next().await {
    if let Err(e) = res {
      warn!(
        "Failed to prune images on Server {} ({}) | {e:#}",
        server.name, server.id
      )
    }
  }

  Ok(())
}

/// Where in the hour after midnight UTC a server prunes its images.
/// Removing a large image holds podman's storage lock long enough for
/// the agent's `podman ps` to time out, so spreading servers out keeps
/// them from all losing monitoring in the same minute. Derived from the
/// id so a server prunes at the same time every day.
fn prune_offset(server_id: &str) -> Duration {
  let hash = server_id.bytes().fold(0u64, |hash, byte| {
    hash.wrapping_mul(31).wrapping_add(byte as u64)
  });
  Duration::from_millis(hash % ONE_HOUR_MS as u64)
}

async fn prune_stats() -> anyhow::Result<()> {
  if core_config().keep_stats_for_days == 0 {
    return Ok(());
  }
  let delete_before_ts = (unix_timestamp_ms()
    - core_config().keep_stats_for_days as u128 * ONE_DAY_MS)
    as i64;
  let res = db_client()
    .stats
    .delete_many(doc! {
      "ts": { "$lt": delete_before_ts }
    })
    .await?;
  if res.deleted_count > 0 {
    info!("deleted {} stats from db", res.deleted_count);
  }
  Ok(())
}

async fn prune_alerts() -> anyhow::Result<()> {
  if core_config().keep_alerts_for_days == 0 {
    return Ok(());
  }
  let delete_before_ts = (unix_timestamp_ms()
    - core_config().keep_alerts_for_days as u128 * ONE_DAY_MS)
    as i64;
  let res = db_client()
    .alerts
    .delete_many(doc! {
      "ts": { "$lt": delete_before_ts }
    })
    .await?;
  if res.deleted_count > 0 {
    info!("deleted {} alerts from db", res.deleted_count);
  }
  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn prune_offset_is_stable_and_spread_within_the_hour() {
    let ids =
      ["6650a1f0c1d2e3f4a5b6c7d8", "6650a1f0c1d2e3f4a5b6c7d9"];
    let offsets = ids.map(prune_offset);
    assert_eq!(offsets, ids.map(prune_offset));
    assert_ne!(offsets[0], offsets[1]);
    assert!(
      offsets
        .iter()
        .all(|o| *o < Duration::from_millis(ONE_HOUR_MS as u64))
    );
  }
}
