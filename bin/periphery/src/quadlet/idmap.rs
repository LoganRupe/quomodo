//! The host's user namespace slice table: `<stack>/<unit file>` -> slice
//! index, kept in `${root_directory}/idmap.json`. See `quadlet::idmap` for
//! the rules. A slice is never given back: a stack destroyed and deployed
//! again keeps the ownership of whatever volumes it left behind.

use std::{collections::BTreeMap, path::PathBuf};

use anyhow::{Context, anyhow};
use komodo_client::entities::update::Log;
use quadlet::{
  idmap::{self, SUBID_USER, SubIdRange},
  prepare::Prepared,
};
use tokio::sync::Mutex;

use crate::config::periphery_config;

/// Deploys of different stacks run at once; the table is one file.
static TABLE: Mutex<()> = Mutex::const_new(());

fn table_path() -> PathBuf {
  periphery_config().root_directory.join("idmap.json")
}

async fn subid_range(file: &str) -> anyhow::Result<SubIdRange> {
  let contents = tokio::fs::read_to_string(file)
    .await
    .with_context(|| format!("Failed to read {file}"))?;
  idmap::parse_subid(&contents, SUBID_USER).ok_or_else(|| {
    anyhow!(
      "{file} has no entry for '{SUBID_USER}'. Add '{SUBID_USER}:2147483647:2147483648' (the quomodo_agent role does)"
    )
  })
}

async fn load() -> anyhow::Result<BTreeMap<String, u32>> {
  let path = table_path();
  if !path.is_file() {
    return Ok(BTreeMap::new());
  }
  let contents = tokio::fs::read_to_string(&path)
    .await
    .with_context(|| format!("Failed to read {path:?}"))?;
  serde_json::from_str(&contents)
    .with_context(|| format!("Failed to parse {path:?}"))
}

async fn save(table: &BTreeMap<String, u32>) -> anyhow::Result<()> {
  let path = table_path();
  if let Some(parent) = path.parent() {
    tokio::fs::create_dir_all(parent)
      .await
      .with_context(|| format!("Failed to create {parent:?}"))?;
  }
  let tmp = path.with_extension("json.quomodo-tmp");
  let contents = serde_json::to_string_pretty(table)
    .context("Failed to serialize the id map table")?;
  tokio::fs::write(&tmp, contents)
    .await
    .with_context(|| format!("Failed to write {tmp:?}"))?;
  tokio::fs::rename(&tmp, &path)
    .await
    .with_context(|| format!("Failed to move {tmp:?} into place"))
}

/// Give every unit of `stack` its fixed user namespace, rewriting the
/// prepared files in place. New units are added to the table before
/// anything is written to the host, so a failed deploy still keeps its
/// slice.
pub async fn assign(
  stack: &str,
  prepared: &mut Prepared,
) -> anyhow::Result<Log> {
  let subuid = subid_range("/etc/subuid").await?;
  let subgid = subid_range("/etc/subgid").await?;

  let _guard = TABLE.lock().await;
  let mut table = load().await?;
  let before = table.len();
  let assigned = idmap::apply(
    stack,
    &mut prepared.write,
    &mut table,
    &subuid,
    &subgid,
  )?;
  if table.len() != before {
    save(&table).await?;
  }
  prepared.units = quadlet::parse_units(
    &prepared
      .write
      .iter()
      .map(|(path, contents)| komodo_client::entities::FileContents {
        path: path.clone(),
        contents: contents.clone(),
      })
      .collect::<Vec<_>>(),
  );

  let msg = if assigned.is_empty() {
    String::from(
      "No unit takes a slice: every user namespace is set by hand",
    )
  } else {
    assigned
      .iter()
      .map(|a| {
        let span = |maps: &[idmap::Mapping]| {
          // The plain run holding container id 0, as a host range.
          maps
            .iter()
            .find(|m| m.container == 0)
            .map(|m| {
              format!(
                "{}-{}",
                m.host,
                m.host as u64 + m.size as u64 - 1
              )
            })
            .unwrap_or_default()
        };
        format!(
          "{}: uid {} gid {}{}",
          a.file,
          span(&a.uids),
          span(&a.gids),
          if a.uids.len() > 1 || a.gids.len() > 1 {
            " (with carve-outs)"
          } else {
            ""
          }
        )
      })
      .collect::<Vec<_>>()
      .join("\n")
  };
  Ok(Log::simple("User Namespaces", msg))
}
