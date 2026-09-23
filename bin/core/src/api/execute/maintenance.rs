use std::{fmt::Write as _, sync::OnceLock};

use anyhow::{Context, anyhow};
use command::{CommandOptions, run_komodo_standard_command};
use database::{bson::Document, mungos::find::find_collect};
use formatting::{bold, format_serror};
use futures_util::{StreamExt, stream::FuturesOrdered};
use komodo_client::api::execute::{
  BackupCoreDatabase, ClearRepoCache, RotateAllServerKeys,
  RotateCoreKeys,
};
use komodo_client::entities::server::ServerState;
use mogh_error::AddStatusCodeError;
use mogh_resolver::Resolve;
use periphery_client::api;
use reqwest::StatusCode;
use tokio::sync::Mutex;

use crate::{
  api::execute::ExecuteArgs,
  config::{core_config, core_keys},
  helpers::{periphery_client, update::update_update},
  resource::rotate_server_keys,
  state::{db_client, server_status_cache},
};

/// Makes sure the method can only be called once at a time
fn clear_repo_cache_lock() -> &'static Mutex<()> {
  static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
  LOCK.get_or_init(Default::default)
}

impl Resolve<ExecuteArgs> for ClearRepoCache {
  #[instrument(
    "ClearRepoCache",
    skip_all,
    fields(
      task_id = task_id.to_string(),
      operator = user.id,
      update_id = update.id
    )
  )]
  async fn resolve(
    self,
    ExecuteArgs {
      user,
      update,
      task_id,
    }: &ExecuteArgs,
  ) -> Result<Self::Response, Self::Error> {
    if !user.admin {
      return Err(
        anyhow!("This method is admin only.")
          .status_code(StatusCode::FORBIDDEN),
      );
    }

    let _lock = clear_repo_cache_lock()
      .try_lock()
      .context("Clear already in progress...")?;

    let mut update = update.clone();

    let mut contents =
      tokio::fs::read_dir(&core_config().repo_directory)
        .await
        .context("Failed to read repo cache directory")?;

    loop {
      let path = match contents
        .next_entry()
        .await
        .context("Failed to read contents at path")
      {
        Ok(Some(contents)) => contents.path(),
        Ok(None) => break,
        Err(e) => {
          update.push_error_log(
            "Read Directory",
            format_serror(&e.into()),
          );
          continue;
        }
      };
      if path.is_dir() {
        match tokio::fs::remove_dir_all(&path)
          .await
          .context("Failed to clear contents at path")
        {
          Ok(_) => {}
          Err(e) => {
            update.push_error_log(
              "Clear Directory",
              format_serror(&e.into()),
            );
          }
        };
      }
    }

    update.finalize();
    update_update(update.clone()).await?;

    Ok(update)
  }
}

//

/// Makes sure the method can only be called once at a time
fn backup_database_lock() -> &'static Mutex<()> {
  static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
  LOCK.get_or_init(Default::default)
}

impl Resolve<ExecuteArgs> for BackupCoreDatabase {
  #[instrument(
    "BackupCoreDatabase",
    skip_all,
    fields(
      task_id = task_id.to_string(),
      operator = user.id,
      update_id = update.id,
    )
  )]
  async fn resolve(
    self,
    ExecuteArgs {
      user,
      update,
      task_id,
    }: &ExecuteArgs,
  ) -> Result<Self::Response, Self::Error> {
    if !user.admin {
      return Err(
        anyhow!("This method is admin only.")
          .status_code(StatusCode::FORBIDDEN),
      );
    }

    let _lock = backup_database_lock()
      .try_lock()
      .context("Backup already in progress...")?;

    let mut update = update.clone();

    update_update(update.clone()).await?;

    let res = run_komodo_standard_command(
      "Backup Core Database",
      "km database backup --yes",
      CommandOptions::default(),
    )
    .await;

    update.logs.push(res);
    update.finalize();

    update_update(update.clone()).await?;

    Ok(update)
  }
}

//

//

/// Makes sure the method can only be called once at a time
fn global_rotate_lock() -> &'static Mutex<()> {
  static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
  LOCK.get_or_init(Default::default)
}

impl Resolve<ExecuteArgs> for RotateAllServerKeys {
  #[instrument(
    "RotateAllServerKeys",
    skip_all,
    fields(
      task_id = task_id.to_string(),
      operator = user.id,
      update_id = update.id,
    )
  )]
  async fn resolve(
    self,
    ExecuteArgs {
      user,
      update,
      task_id,
    }: &ExecuteArgs,
  ) -> Result<Self::Response, Self::Error> {
    if !user.admin {
      return Err(
        anyhow!("This method is admin only.")
          .status_code(StatusCode::FORBIDDEN),
      );
    }

    let _lock = global_rotate_lock()
      .try_lock()
      .context("Key rotation already in progress...")?;

    let mut update = update.clone();

    update_update(update.clone()).await?;

    let mut servers = db_client()
      .servers
      .find(Document::new())
      .await
      .context("Failed to query servers from database")?;

    let server_status_cache = server_status_cache();

    let mut log = String::new();

    while let Some(server) = servers.next().await {
      let server = match server {
        Ok(server) => server,
        Err(e) => {
          warn!("Failed to parse Server | {e:#}");
          continue;
        }
      };
      if !server.config.auto_rotate_keys {
        let _ = write!(
          &mut log,
          "\nSkipping {}: Key Rotation Disabled ⚙️",
          bold(&server.name)
        );
        continue;
      }
      let Some(status) = server_status_cache.get(&server.id).await
      else {
        let _ = write!(
          &mut log,
          "\nSkipping {}: No Status ⚠️",
          bold(&server.name)
        );
        continue;
      };
      match status.state {
        ServerState::Disabled => {
          let _ = write!(
            &mut log,
            "\nSkipping {}: Server Disabled ⚙️",
            bold(&server.name)
          );
          continue;
        }
        ServerState::NotOk => {
          let _ = write!(
            &mut log,
            "\nSkipping {}: Server Not Ok ⚠️",
            bold(&server.name)
          );
          continue;
        }
        _ => {}
      }
      match rotate_server_keys(&server).await {
        Ok(_) => {
          let _ = write!(
            &mut log,
            "\nRotated keys for {} ✅",
            bold(&server.name)
          );
        }
        Err(e) => {
          update.push_error_log(
            "Key Rotation Failure",
            format_serror(
              &e.context(format!(
                "Failed to rotate {} keys",
                bold(&server.name)
              ))
              .into(),
            ),
          );
        }
      }
    }

    update.push_simple_log("Rotate Server Keys", log);
    update.finalize();
    update_update(update.clone()).await?;

    Ok(update)
  }
}

impl Resolve<ExecuteArgs> for RotateCoreKeys {
  #[instrument(
    "RotateCoreKeys",
    skip_all,
    fields(
      task_id = task_id.to_string(),
      operator = user.id,
      update_id = update.id,
      force = self.force,
    )
  )]
  async fn resolve(
    self,
    ExecuteArgs {
      user,
      update,
      task_id,
    }: &ExecuteArgs,
  ) -> Result<Self::Response, Self::Error> {
    if !user.admin {
      return Err(
        anyhow!("This method is admin only.")
          .status_code(StatusCode::FORBIDDEN),
      );
    }

    let _lock = global_rotate_lock()
      .try_lock()
      .context("Key rotation already in progress...")?;

    let mut update = update.clone();

    update_update(update.clone()).await?;

    let core_keys = core_keys();

    if !core_keys.rotatable() {
      return Err(anyhow!("Core `private_key` must be pointing to file, for example 'file:/config/keys/core.key'").into());
    };

    let server_status_cache = server_status_cache();
    let servers =
      find_collect(&db_client().servers, Document::new(), None)
        .await
        .context("Failed to query servers from database")?
        .into_iter()
        .map(|server| async move {
          let state = server_status_cache
            .get(&server.id)
            .await
            .map(|s| s.state)
            .unwrap_or(ServerState::NotOk);
          (server, state)
        })
        .collect::<FuturesOrdered<_>>()
        .collect::<Vec<_>>()
        .await;

    if !self.force
      && let Some((server, _)) = servers
        .iter()
        .find(|(_, state)| matches!(state, ServerState::NotOk))
    {
      return Err(
        anyhow!("Server {} is NotOk, stopping key rotation. Pass `force: true` to continue anyways.", server.name).into(),
      );
    }

    let public_key = core_keys
      .rotate(mogh_pki::PkiKind::Mutual)
      .await?
      .into_inner();

    info!("New Public Key: {public_key}");

    let mut log = format!("New Public Key: {public_key}\n");

    for (server, state) in servers {
      match state {
        ServerState::Disabled => {
          let _ = write!(
            &mut log,
            "\nSkipping {}: Server Disabled ⚙️",
            bold(&server.name)
          );
          continue;
        }
        ServerState::NotOk => {
          // Shouldn't be reached unless 'force: true'
          let _ = write!(
            &mut log,
            "\nSkipping {}: Server Not Ok ⚠️",
            bold(&server.name)
          );
          continue;
        }
        _ => {}
      }
      let periphery = periphery_client(&server).await?;
      let res = periphery
        .request(api::keys::RotateCorePublicKey {
          public_key: public_key.clone(),
        })
        .await;
      match res {
        Ok(_) => {
          let _ = write!(
            &mut log,
            "\nRotated key for {} ✅",
            bold(&server.name)
          );
        }
        Err(e) => {
          update.push_error_log(
            "Key Rotation Failure",
            format_serror(
              &e.context(format!(
                "Failed to rotate for {}. The new Core public key will have to be added manually.",
                bold(&server.name)
              ))
              .into(),
            ),
          );
        }
      }
    }

    update.push_simple_log("Rotate Core Keys", log);
    update.finalize();
    update_update(update.clone()).await?;

    Ok(update)
  }
}
