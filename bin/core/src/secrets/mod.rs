//! Secret store providers.
//!
//! Secrets are resolved by Core at deploy time and sent to the agent over the
//! key-authenticated websocket. They are never written to the database.

use std::{
  collections::{BTreeMap, HashMap},
  sync::OnceLock,
};

use anyhow::{Context, anyhow};
use komodo_client::entities::{
  FileContents,
  config::core::{SecretsProvider, VaultwardenConfig},
  stack::Stack,
};
use quadlet::prepare::referenced_names;

use crate::config::core_config;

pub mod vaultwarden;

/// A store of named secrets, grouped by collection.
pub trait SecretProvider: Send + Sync {
  /// Resolve the given names from the collection.
  /// `NAME` reads the item's password. `NAME.field` reads a custom field.
  /// Every requested name must resolve, or the call fails.
  fn resolve(
    &self,
    collection: &str,
    names: &[String],
  ) -> impl std::future::Future<
    Output = anyhow::Result<HashMap<String, String>>,
  > + Send;
}

enum Provider {
  None,
  Vaultwarden(vaultwarden::Vaultwarden),
}

fn provider() -> &'static Provider {
  static PROVIDER: OnceLock<Provider> = OnceLock::new();
  PROVIDER.get_or_init(|| {
    let config = &core_config().secrets;
    match config.provider {
      SecretsProvider::None => Provider::None,
      SecretsProvider::Vaultwarden => Provider::Vaultwarden(
        vaultwarden::Vaultwarden::new(config.vaultwarden.clone()),
      ),
    }
  })
}

async fn resolve_from_provider(
  collection: &str,
  names: &[String],
) -> anyhow::Result<HashMap<String, String>> {
  match provider() {
    Provider::None => Err(anyhow!(
      "No secrets provider configured (secrets.provider). Cannot resolve [[{}]]",
      names.join("]], [[")
    )),
    Provider::Vaultwarden(vw) => vw.resolve(collection, names).await,
  }
}

#[derive(Debug, Default)]
pub struct ResolvedSecrets {
  /// Secret name -> value
  pub secrets: HashMap<String, String>,
  /// Podman secret name (`<stack>_<NAME>`) -> sha256 of value,
  /// matching what the agent hashes into `contents_hash`.
  pub hashes: BTreeMap<String, String>,
}

/// Find every `[[NAME]]` in the stack files which is not a Variable,
/// and resolve it from the stack's secrets collection.
pub async fn resolve_stack_secrets(
  stack: &Stack,
  files: &[FileContents],
  variables: &HashMap<String, String>,
) -> anyhow::Result<ResolvedSecrets> {
  let mut names = files
    .iter()
    .flat_map(|f| referenced_names(&f.contents))
    .filter(|name| !variables.contains_key(name))
    .collect::<Vec<_>>();
  names.sort();
  names.dedup();

  if names.is_empty() {
    return Ok(ResolvedSecrets::default());
  }

  if stack.config.secrets_collection.is_empty() {
    return Err(anyhow!(
      "Stack references secret/s [[{}]] but has no 'secrets_collection' configured",
      names.join("]], [[")
    ));
  }

  let secrets =
    resolve_from_provider(&stack.config.secrets_collection, &names)
      .await
      .with_context(|| {
        format!(
          "Failed to resolve secrets from collection '{}'",
          stack.config.secrets_collection
        )
      })?;

  // Run the same preparation the agent runs, so the secret hashes
  // (including config files mounted with `Secret=file:`) match exactly,
  // and so a rule violation fails here instead of on the host.
  let prepared =
    quadlet::prepare::prepare(stack, files, variables, &secrets)
      .context("Stack files failed secret validation")?;

  Ok(ResolvedSecrets {
    secrets,
    hashes: prepared.secret_hashes,
  })
}

/// Only used at startup to fail fast on a broken provider config.
pub fn validate_config(
  config: &VaultwardenConfig,
) -> anyhow::Result<()> {
  if config.url.is_empty() {
    return Err(anyhow!("secrets.vaultwarden.url is required"));
  }
  if config.client_id.is_empty() || config.client_secret.is_empty() {
    return Err(anyhow!(
      "secrets.vaultwarden.client_id and client_secret are required"
    ));
  }
  if config.master_password.is_empty() {
    return Err(anyhow!(
      "secrets.vaultwarden.master_password is required to decrypt the vault"
    ));
  }
  Ok(())
}
