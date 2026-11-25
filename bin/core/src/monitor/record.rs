use std::{collections::HashMap, str::FromStr};

use database::mungos::{
  find::find_collect,
  mongodb::bson::{doc, oid::ObjectId},
};
use komodo_client::entities::stats::{
  SystemStatsRecord, TotalDiskUsage, sum_disk_usage,
};

use crate::{
  helpers::query::running_usage,
  state::{db_client, server_status_cache},
};

pub async fn record_server_stats(ts: i64) {
  let status = server_status_cache().get_values().await;
  // A stack that moved during this cycle can be in two servers' last
  // polls. Record it only under the server it is assigned to now, read
  // from the database: the resource cache can lag a create or a move.
  let stack_ids = status
    .iter()
    .flat_map(|s| s.stack_usage.iter())
    .filter_map(|stack| ObjectId::from_str(&stack.id).ok())
    .collect::<Vec<_>>();
  let assignments = if stack_ids.is_empty() {
    HashMap::new()
  } else {
    match find_collect(
      &db_client().stacks,
      doc! { "_id": { "$in": stack_ids } },
      None,
    )
    .await
    {
      Ok(stacks) => stacks
        .into_iter()
        .map(|stack| (stack.id, stack.config.server_id))
        .collect::<HashMap<_, _>>(),
      Err(e) => {
        error!("Failed to read stack assignments for stats | {e:#}");
        HashMap::new()
      }
    }
  };
  let assigned = |stack_id: &str, server_id: &str| {
    assignments
      .get(stack_id)
      .is_some_and(|assigned| assigned == server_id)
  };
  let records = status
    .into_iter()
    .filter_map(|status| {
      let stats = status.system_stats.as_ref()?;

      let TotalDiskUsage {
        used_gb: disk_used_gb,
        total_gb: disk_total_gb,
      } = sum_disk_usage(&stats.disks);

      Some(SystemStatsRecord {
        ts,
        sid: status.id.clone(),
        cpu_perc: stats.cpu_perc,
        load_average: stats.load_average,
        mem_total_gb: stats.mem_total_gb,
        mem_used_gb: stats.mem_used_gb,
        mem_buff_cache_gb: stats.mem_buff_cache_gb,
        mem_zfs_arc_gb: stats.mem_zfs_arc_gb,
        swap_total_gb: stats.swap_total_gb,
        swap_used_gb: stats.swap_used_gb,
        disk_total_gb,
        disk_used_gb,
        disks: stats.disks.clone(),
        network_ingress_bytes: stats.network_ingress_bytes,
        network_egress_bytes: stats.network_egress_bytes,
        containers: status
          .containers
          .as_deref()
          .and_then(running_usage),
        stacks: status
          .stack_usage
          .iter()
          .filter(|stack| assigned(&stack.id, &status.id))
          .cloned()
          .collect(),
      })
    })
    .collect::<Vec<_>>();
  if !records.is_empty() {
    let res = db_client().stats.insert_many(records).await;
    if let Err(e) = res {
      error!("failed to record server stats | {e:#}");
    }
  }
}
