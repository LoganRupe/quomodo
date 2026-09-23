use std::{
  path::PathBuf,
  sync::{Arc, OnceLock},
};

use anyhow::Context;
use arc_swap::ArcSwap;
use mogh_cache::CloneCache;
use mogh_pki::{PkiKind, RotatableKeyPair, SpkiPublicKey};
use periphery_client::transport::EncodedTransportMessage;
use tokio::sync::{OnceCell, RwLock};
use transport::channel::BufferedChannel;

use crate::{
  config::periphery_config, helpers::resolve_host_public_ip,
  stats::StatsClient,
};

/// Should call in startup to ensure the agent errors without valid private key.
pub fn periphery_keys() -> &'static RotatableKeyPair {
  static PERIPHERY_KEYS: OnceLock<RotatableKeyPair> = OnceLock::new();
  PERIPHERY_KEYS.get_or_init(|| {
    RotatableKeyPair::from_private_key_spec(
      PkiKind::Mutual,
      &periphery_config().private_key_spec(),
    )
    .unwrap()
  })
}

pub fn core_public_keys() -> &'static CorePublicKeys {
  static CORE_PUBLIC_KEYS: OnceLock<CorePublicKeys> = OnceLock::new();
  CORE_PUBLIC_KEYS.get_or_init(CorePublicKeys::default)
}

pub struct CorePublicKeys {
  keys: ArcSwap<Vec<SpkiPublicKey>>,
  /// If any keys fail to read, store the paths here.
  /// The agent writes the Core pub key to these files as it connects.
  to_write: ArcSwap<Vec<PathBuf>>,
}

impl Default for CorePublicKeys {
  fn default() -> Self {
    let keys = CorePublicKeys {
      keys: Default::default(),
      to_write: Default::default(),
    };
    keys.refresh();
    keys
  }
}

impl CorePublicKeys {
  pub fn load(&self) -> arc_swap::Guard<Arc<Vec<SpkiPublicKey>>> {
    self.keys.load()
  }

  pub async fn is_valid(&self, public_key: &str) -> bool {
    // Maybe init the Core public key file if it doesn't exist.
    self.maybe_write(public_key).await;
    let keys = self.keys.load();
    keys.is_empty() || keys.iter().any(|pk| pk.as_str() == public_key)
  }

  async fn maybe_write(&self, public_key: &str) {
    let to_write = self.to_write.load();
    let path = match to_write.as_slice() {
      // Do nothing if empty
      [] => return,
      [path, _rest @ ..] => path,
    };
    let public_key = match SpkiPublicKey::from_maybe_pem(public_key) {
      Ok(public_key) => public_key,
      Err(e) => {
        error!("Invalid incoming public key | {e:#}");
        return;
      }
    };
    // Check equality at path again before trying to rewrite.
    match SpkiPublicKey::from_file(path) {
      Ok(existing) if existing == public_key => {
        self.refresh();
        return;
      }
      _ => {}
    }
    if let Some(parent) = path.parent() {
      let _ = tokio::fs::create_dir_all(parent).await;
    }
    if let Err(e) = public_key.write_pem_async(path).await {
      warn!("Failed to pin incoming public key | {e:#}");
    }
    self.refresh();
  }

  pub fn refresh(&self) {
    let config = periphery_config();
    let mut to_write = Vec::new();
    let core_public_keys = config
      .core_public_keys_spec()
      .iter()
      .flat_map(|public_key| {
        if let Some(path) = public_key.strip_prefix("file:") {
          match SpkiPublicKey::from_file(path) {
            Ok(public_key) => Some(public_key),
            Err(e) => {
              // It will be written when the Core public key is received.
              warn!("{e:#}");
              to_write.push(path.into());
              None
            }
          }
        } else {
          SpkiPublicKey::from_maybe_pem(public_key)
            .context("Invalid hardcoded public key. If this is supposed to point to file, add 'file:' prefix.")
            .inspect_err(|e| {
              error!("{e:#}");
              std::process::exit(1)
            })
            .ok()
        }
      })
      .collect::<Vec<_>>();
    self.keys.store(Arc::new(core_public_keys));
    self.to_write.store(Arc::new(to_write));
  }
}

/// Core Address / Host -> Channel
pub type CoreConnection = BufferedChannel<EncodedTransportMessage>;
pub type CoreConnections = CloneCache<String, Arc<CoreConnection>>;

pub fn core_connections() -> &'static CoreConnections {
  static CORE_CONNECTIONS: OnceLock<CoreConnections> =
    OnceLock::new();
  CORE_CONNECTIONS.get_or_init(Default::default)
}

pub fn stats_client() -> &'static RwLock<StatsClient> {
  static STATS_CLIENT: OnceLock<RwLock<StatsClient>> =
    OnceLock::new();
  STATS_CLIENT.get_or_init(|| RwLock::new(StatsClient::default()))
}

pub async fn host_public_ip() -> Option<&'static String> {
  static PUBLIC_IPS: OnceCell<Option<String>> = OnceCell::const_new();
  PUBLIC_IPS
    .get_or_init(|| async {
      resolve_host_public_ip()
        .await
        .inspect_err(|e| {
          warn!("Failed to resolve host public ip | {e:#}")
        })
        .ok()
    })
    .await
    .as_ref()
}
