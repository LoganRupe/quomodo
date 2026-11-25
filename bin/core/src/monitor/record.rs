use std::collections::HashMap;

use komodo_client::entities::stats::{
  StackUsage, SystemStatsRecord, TotalDiskUsage, sum_disk_usage,
};

use crate::{
  helpers::query::running_usage,
  state::{db_client, server_status_cache, stack_status_cache},
};

pub async fn record_server_stats(ts: i64) {
  let status = server_status_cache().get_values().await;
  // Each server's stacks with a measured usage, as last observed
  // on that server.
  let mut stacks = HashMap::<String, Vec<StackUsage>>::new();
  for stack in stack_status_cache().get_values().await {
    let Some(usage) = stack.curr.usage else {
      continue;
    };
    stacks
      .entry(stack.curr.server_id.clone())
      .or_default()
      .push(StackUsage {
        id: stack.curr.id.clone(),
        cpu_cores: usage.cpu_cores,
        mem_bytes: usage.mem_bytes,
      });
  }
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
        stacks: stacks.remove(&status.id).unwrap_or_default(),
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
