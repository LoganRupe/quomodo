//! Vaultwarden via the Bitwarden vault protocol.
//!
//! Vaultwarden does not implement Bitwarden Secrets Manager, so the only
//! client path is: log in with a user API key, derive the master key from
//! the master password, decrypt the user's symmetric key, sync the vault,
//! decrypt organization keys, and decrypt ciphers client-side.
//!
//! Crypto: PBKDF2-SHA256 or Argon2id for the master key, HKDF-Expand for the
//! stretched key, AES-256-CBC + HMAC-SHA256 for `2.` strings, RSA-OAEP-SHA1
//! for `4.` organization keys.

use std::{
  collections::HashMap,
  num::NonZeroU32,
  sync::Arc,
  time::{Duration, Instant},
};

use anyhow::{Context, anyhow};
use aws_lc_rs::{
  cipher::{
    AES_256, DecryptionContext, PaddedBlockDecryptingKey,
    UnboundCipherKey,
  },
  digest, hkdf, hmac,
  iv::FixedLength,
  pbkdf2,
  rsa::{
    OAEP_SHA1_MGF1SHA1, OaepPrivateDecryptingKey,
    PrivateDecryptingKey,
  },
};
use base64::{Engine, engine::general_purpose::STANDARD as B64};
use futures_util::{
  FutureExt,
  future::{BoxFuture, Shared},
};
use komodo_client::entities::config::core::VaultwardenConfig;
use reqwest::header::{HeaderMap, HeaderValue};
use serde::Deserialize;
use tokio::sync::Mutex;

use super::SecretProvider;

/// A session is kept for this long before logging in again.
const SESSION_TTL: Duration = Duration::from_secs(50 * 60);

/// A vault sync answers every resolve that starts within this long of the
/// sync request. A deploy pass resolves each stack once or twice within a
/// few seconds, so the whole pass shares one sync. An item edited in the
/// vault reaches any resolve that starts this long after the edit.
const SYNC_TTL: Duration = Duration::from_secs(30);

/// Sent as `Bitwarden-Client-Version` on every request. Vaultwarden logs an
/// error on `api/sync` without it, and leaves SSH key items out of the sync
/// for clients older than 2024.12.0, the only version it checks.
const CLIENT_VERSION: &str = "2024.12.0";

/// Bitwarden's device type for SDK clients, sent on login and as the
/// `Device-Type` header.
const DEVICE_TYPE: &str = "21";

pub struct Vaultwarden {
  client: Arc<Client>,
  sync_ttl: Duration,
  state: Mutex<SyncState>,
}

struct Client {
  config: VaultwardenConfig,
  http: reqwest::Client,
  session: Mutex<Option<Session>>,
}

/// A vault sync and the session it was fetched with.
struct Vault {
  session: Session,
  sync: SyncResponse,
  /// When the sync request was sent.
  fetched: Instant,
}

type VaultFuture =
  Shared<BoxFuture<'static, Result<Arc<Vault>, Arc<anyhow::Error>>>>;

#[derive(Default)]
struct SyncState {
  /// The last successful sync. A failed one is never kept.
  last: Option<Arc<Vault>>,
  /// The sync in progress. Resolves that arrive while it runs wait for it
  /// instead of starting their own, and all get its result.
  in_flight: Option<VaultFuture>,
}

struct Session {
  access_token: String,
  user_key: SymmetricKey,
  private_key_der: Vec<u8>,
  started: Instant,
}

/// 32 byte encryption key + 32 byte mac key.
#[derive(Clone)]
struct SymmetricKey {
  enc: [u8; 32],
  mac: [u8; 32],
}

impl SymmetricKey {
  fn from_bytes(bytes: &[u8]) -> anyhow::Result<Self> {
    if bytes.len() != 64 {
      return Err(anyhow!(
        "Expected 64 byte symmetric key, got {}",
        bytes.len()
      ));
    }
    let mut enc = [0u8; 32];
    let mut mac = [0u8; 32];
    enc.copy_from_slice(&bytes[..32]);
    mac.copy_from_slice(&bytes[32..]);
    Ok(SymmetricKey { enc, mac })
  }
}

impl Vaultwarden {
  pub fn new(config: VaultwardenConfig) -> Self {
    let mut headers = HeaderMap::new();
    headers.insert(
      "Bitwarden-Client-Version",
      HeaderValue::from_static(CLIENT_VERSION),
    );
    headers
      .insert("Device-Type", HeaderValue::from_static(DEVICE_TYPE));
    Vaultwarden {
      client: Arc::new(Client {
        config,
        http: reqwest::Client::builder()
          .timeout(Duration::from_secs(30))
          .default_headers(headers)
          .build()
          .expect("Failed to build http client"),
        session: Default::default(),
      }),
      sync_ttl: SYNC_TTL,
      state: Default::default(),
    }
  }

  /// The last sync if it is younger than `sync_ttl`, else a new one.
  /// Concurrent callers share one sync.
  async fn vault(&self) -> anyhow::Result<Arc<Vault>> {
    let fetch = {
      let mut state = self.state.lock().await;
      if let Some(vault) = &state.last
        && vault.fetched.elapsed() < self.sync_ttl
      {
        return Ok(vault.clone());
      }
      match &state.in_flight {
        Some(fetch) => fetch.clone(),
        None => {
          let client = self.client.clone();
          let fetch = async move {
            client.fetch().await.map(Arc::new).map_err(Arc::new)
          }
          .boxed()
          .shared();
          state.in_flight = Some(fetch.clone());
          fetch
        }
      }
    };
    let res = fetch.clone().await;
    let mut state = self.state.lock().await;
    if state.in_flight.as_ref().is_some_and(|f| f.ptr_eq(&fetch)) {
      state.in_flight = None;
      if let Ok(vault) = &res {
        state.last = Some(vault.clone());
      }
    }
    res.map_err(|e| anyhow!("{e:#}"))
  }
}

impl Client {
  async fn fetch(&self) -> anyhow::Result<Vault> {
    let session = self.session().await?;
    let fetched = Instant::now();
    let sync = self.sync(&session).await?;
    Ok(Vault {
      session,
      sync,
      fetched,
    })
  }

  fn url(&self, path: &str) -> String {
    format!("{}/{}", self.config.url.trim_end_matches('/'), path)
  }

  async fn session(&self) -> anyhow::Result<Session> {
    let mut lock = self.session.lock().await;
    if let Some(session) = lock.as_ref()
      && session.started.elapsed() < SESSION_TTL
    {
      return Ok(session.clone());
    }
    let session = self.login().await?;
    *lock = Some(session.clone());
    Ok(session)
  }

  #[instrument("VaultwardenLogin", skip_all)]
  async fn login(&self) -> anyhow::Result<Session> {
    super::validate_config(&self.config)?;
    let res = self
      .http
      .post(self.url("identity/connect/token"))
      .form(&[
        ("grant_type", "client_credentials"),
        ("scope", "api"),
        ("client_id", self.config.client_id.as_str()),
        ("client_secret", self.config.client_secret.as_str()),
        ("deviceType", DEVICE_TYPE),
        ("deviceIdentifier", "quomodo-core"),
        ("deviceName", "quomodo"),
      ])
      .send()
      .await
      .context("Failed to reach Vaultwarden identity endpoint")?;
    let status = res.status();
    let body = res.text().await.unwrap_or_default();
    if !status.is_success() {
      return Err(anyhow!(
        "Vaultwarden login failed ({status}): {}",
        redact_body(&body)
      ));
    }
    let token: TokenResponse = serde_json::from_str(&body)
      .context("Failed to parse Vaultwarden token response")?;

    let email = email_from_jwt(&token.access_token).context(
      "Failed to read email from Vaultwarden access token",
    )?;

    let master_key = derive_master_key(
      &self.config.master_password,
      &email,
      token.kdf,
      token.kdf_iterations,
      token.kdf_memory,
      token.kdf_parallelism,
    )?;
    let stretched = stretch_master_key(&master_key)?;

    let user_key_bytes = decrypt_symmetric(&stretched, &token.key)
      .context("Failed to decrypt user symmetric key. Is the master password right?")?;
    let user_key = SymmetricKey::from_bytes(&user_key_bytes)?;

    let private_key_der =
      decrypt_symmetric(&user_key, &token.private_key)
        .context("Failed to decrypt user private key")?;

    info!("Logged in to Vaultwarden as {email}");

    Ok(Session {
      access_token: token.access_token,
      user_key,
      private_key_der,
      started: Instant::now(),
    })
  }

  async fn sync(
    &self,
    session: &Session,
  ) -> anyhow::Result<SyncResponse> {
    let res = self
      .http
      .get(self.url("api/sync?excludeDomains=true"))
      .bearer_auth(&session.access_token)
      .send()
      .await
      .context("Failed to reach Vaultwarden sync endpoint")?;
    let status = res.status();
    let body = res.text().await.unwrap_or_default();
    if !status.is_success() {
      return Err(anyhow!(
        "Vaultwarden sync failed ({status}): {}",
        redact_body(&body)
      ));
    }
    serde_json::from_str(&body)
      .context("Failed to parse Vaultwarden sync response")
  }
}

impl Clone for Session {
  fn clone(&self) -> Self {
    Session {
      access_token: self.access_token.clone(),
      user_key: self.user_key.clone(),
      private_key_der: self.private_key_der.clone(),
      started: self.started,
    }
  }
}

impl SecretProvider for Vaultwarden {
  #[instrument("VaultwardenResolve", skip(self))]
  async fn resolve(
    &self,
    collection: &str,
    names: &[String],
  ) -> anyhow::Result<HashMap<String, String>> {
    let vault = self.vault().await?;
    let session = &vault.session;
    let sync = &vault.sync;

    // Organization keys, decrypted with the user's private key.
    let mut org_keys = HashMap::<String, SymmetricKey>::new();
    for org in &sync.profile.organizations {
      let key = decrypt_org_key(&session.private_key_der, &org.key)
        .with_context(|| {
        format!("Failed to decrypt key for organization {}", org.id)
      })?;
      org_keys.insert(org.id.clone(), key);
    }

    // Find the collection by decrypted name.
    let mut matches = Vec::new();
    for c in &sync.collections {
      let Some(key) = org_keys.get(&c.organization_id) else {
        continue;
      };
      let name = decrypt_string(key, &c.name).unwrap_or_default();
      if name == collection {
        matches.push(c);
      }
    }
    let target = match matches.as_slice() {
      [c] => *c,
      [] => {
        return Err(anyhow!(
          "No Vaultwarden collection named '{collection}' is visible to the service account"
        ));
      }
      _ => {
        return Err(anyhow!(
          "More than one Vaultwarden collection is named '{collection}'; names must be unique across organizations"
        ));
      }
    };
    let org_key = &org_keys[&target.organization_id];

    // Items in the collection, by decrypted name.
    let mut items = HashMap::<String, &Cipher>::new();
    for cipher in &sync.ciphers {
      if !cipher.collection_ids.iter().any(|id| id == &target.id) {
        continue;
      }
      if cipher.deleted_date.is_some() {
        continue;
      }
      let name =
        decrypt_string(org_key, &cipher.name).with_context(|| {
          format!("Failed to decrypt item {}", cipher.id)
        })?;
      items.insert(name, cipher);
    }

    let mut out = HashMap::new();
    let mut missing = Vec::new();
    for requested in names {
      let (item_name, field) = match requested.split_once('.') {
        Some((item, field)) => (item, Some(field)),
        None => (requested.as_str(), None),
      };
      let Some(cipher) = items.get(item_name) else {
        missing.push(requested.clone());
        continue;
      };
      let value = match field {
        None => cipher
          .login
          .as_ref()
          .and_then(|l| l.password.as_ref())
          .map(|p| decrypt_string(org_key, p))
          .transpose()?,
        Some(field) => {
          let mut found = None;
          for f in &cipher.fields {
            let name =
              decrypt_string(org_key, &f.name).unwrap_or_default();
            if name == field {
              found = Some(
                f.value
                  .as_ref()
                  .map(|v| decrypt_string(org_key, v))
                  .transpose()?
                  .unwrap_or_default(),
              );
              break;
            }
          }
          found
        }
      };
      match value {
        Some(value) => {
          out.insert(requested.clone(), value);
        }
        None => missing.push(requested.clone()),
      }
    }

    if !missing.is_empty() {
      return Err(anyhow!(
        "Not found in collection '{collection}': [[{}]]",
        missing.join("]], [[")
      ));
    }

    Ok(out)
  }
}

// =========
//  PROTOCOL
// =========

#[derive(Deserialize)]
struct TokenResponse {
  access_token: String,
  #[serde(rename = "Key", alias = "key")]
  key: String,
  #[serde(rename = "PrivateKey", alias = "privateKey")]
  private_key: String,
  #[serde(rename = "Kdf", alias = "kdf", default)]
  kdf: u32,
  #[serde(
    rename = "KdfIterations",
    alias = "kdfIterations",
    default = "default_iterations"
  )]
  kdf_iterations: u32,
  #[serde(rename = "KdfMemory", alias = "kdfMemory", default)]
  kdf_memory: Option<u32>,
  #[serde(
    rename = "KdfParallelism",
    alias = "kdfParallelism",
    default
  )]
  kdf_parallelism: Option<u32>,
}

fn default_iterations() -> u32 {
  600_000
}

#[derive(Deserialize)]
struct SyncResponse {
  profile: Profile,
  #[serde(default)]
  collections: Vec<Collection>,
  #[serde(default)]
  ciphers: Vec<Cipher>,
}

#[derive(Deserialize)]
struct Profile {
  #[serde(default)]
  organizations: Vec<Organization>,
}

#[derive(Deserialize)]
struct Organization {
  id: String,
  #[serde(default)]
  key: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Collection {
  id: String,
  organization_id: String,
  name: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Cipher {
  id: String,
  #[serde(default)]
  collection_ids: Vec<String>,
  #[serde(default)]
  name: String,
  #[serde(default)]
  login: Option<Login>,
  #[serde(default)]
  fields: Vec<Field>,
  #[serde(default)]
  deleted_date: Option<String>,
}

#[derive(Deserialize)]
struct Login {
  #[serde(default)]
  password: Option<String>,
}

#[derive(Deserialize)]
struct Field {
  #[serde(default)]
  name: String,
  #[serde(default)]
  value: Option<String>,
}

fn email_from_jwt(jwt: &str) -> anyhow::Result<String> {
  let payload =
    jwt.split('.').nth(1).context("Access token is not a JWT")?;
  let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
    .decode(payload)
    .context("JWT payload is not base64url")?;
  #[derive(Deserialize)]
  struct Claims {
    email: String,
  }
  let claims: Claims = serde_json::from_slice(&bytes)
    .context("JWT payload has no email claim")?;
  Ok(claims.email)
}

/// Strip anything that looks like a token or key from an error body.
fn redact_body(body: &str) -> String {
  let trimmed: String = body.chars().take(300).collect();
  if trimmed.contains("access_token") || trimmed.contains("\"Key\"") {
    String::from("<redacted>")
  } else {
    trimmed
  }
}

// ========
//  CRYPTO
// ========

fn derive_master_key(
  password: &str,
  email: &str,
  kdf: u32,
  iterations: u32,
  memory_mib: Option<u32>,
  parallelism: Option<u32>,
) -> anyhow::Result<[u8; 32]> {
  let email = email.trim().to_lowercase();
  let mut out = [0u8; 32];
  match kdf {
    // PBKDF2-SHA256
    0 => {
      let iterations = NonZeroU32::new(iterations)
        .context("KdfIterations must be greater than zero")?;
      pbkdf2::derive(
        pbkdf2::PBKDF2_HMAC_SHA256,
        iterations,
        email.as_bytes(),
        password.as_bytes(),
        &mut out,
      );
    }
    // Argon2id, salt = sha256(email)
    1 => {
      let salt = digest::digest(&digest::SHA256, email.as_bytes());
      let params = argon2::Params::new(
        memory_mib.unwrap_or(64) * 1024,
        iterations,
        parallelism.unwrap_or(4),
        Some(32),
      )
      .map_err(|e| anyhow!("Invalid Argon2 params: {e}"))?;
      argon2::Argon2::new(
        argon2::Algorithm::Argon2id,
        argon2::Version::V0x13,
        params,
      )
      .hash_password_into(
        password.as_bytes(),
        salt.as_ref(),
        &mut out,
      )
      .map_err(|e| anyhow!("Argon2 failed: {e}"))?;
    }
    other => return Err(anyhow!("Unsupported Kdf type {other}")),
  }
  Ok(out)
}

struct KeyLen(usize);

impl hkdf::KeyType for KeyLen {
  fn len(&self) -> usize {
    self.0
  }
}

/// HKDF-Expand the master key into the stretched (enc, mac) key.
fn stretch_master_key(
  master_key: &[u8; 32],
) -> anyhow::Result<SymmetricKey> {
  let prk = hkdf::Prk::new_less_safe(hkdf::HKDF_SHA256, master_key);
  let mut enc = [0u8; 32];
  let mut mac = [0u8; 32];
  prk
    .expand(&[b"enc"], KeyLen(32))
    .map_err(|_| anyhow!("HKDF expand failed"))?
    .fill(&mut enc)
    .map_err(|_| anyhow!("HKDF fill failed"))?;
  prk
    .expand(&[b"mac"], KeyLen(32))
    .map_err(|_| anyhow!("HKDF expand failed"))?
    .fill(&mut mac)
    .map_err(|_| anyhow!("HKDF fill failed"))?;
  Ok(SymmetricKey { enc, mac })
}

/// Decrypt a `2.<iv>|<data>|<mac>` string (AesCbc256_HmacSha256_B64).
fn decrypt_symmetric(
  key: &SymmetricKey,
  enc: &str,
) -> anyhow::Result<Vec<u8>> {
  let (typ, rest) = enc
    .split_once('.')
    .context("EncString has no type prefix")?;
  if typ != "2" {
    return Err(anyhow!(
      "Unsupported EncString type {typ}, expected 2 (AES-CBC + HMAC)"
    ));
  }
  let mut parts = rest.split('|');
  let iv = B64
    .decode(parts.next().context("EncString missing iv")?)
    .context("EncString iv is not base64")?;
  let mut data = B64
    .decode(parts.next().context("EncString missing data")?)
    .context("EncString data is not base64")?;
  let mac = B64
    .decode(parts.next().context("EncString missing mac")?)
    .context("EncString mac is not base64")?;

  // MAC over iv || data
  let mac_key = hmac::Key::new(hmac::HMAC_SHA256, &key.mac);
  let mut signed = Vec::with_capacity(iv.len() + data.len());
  signed.extend_from_slice(&iv);
  signed.extend_from_slice(&data);
  hmac::verify(&mac_key, &signed, &mac)
    .map_err(|_| anyhow!("EncString MAC check failed"))?;

  let iv: [u8; 16] = iv
    .try_into()
    .map_err(|_| anyhow!("EncString iv is not 16 bytes"))?;
  let cipher_key = UnboundCipherKey::new(&AES_256, &key.enc)
    .map_err(|_| anyhow!("Invalid AES key"))?;
  let decrypting = PaddedBlockDecryptingKey::cbc_pkcs7(cipher_key)
    .map_err(|_| anyhow!("Failed to init AES-CBC"))?;
  let plain = decrypting
    .decrypt(
      &mut data,
      DecryptionContext::Iv128(FixedLength::from(iv)),
    )
    .map_err(|_| anyhow!("AES-CBC decrypt failed"))?;
  Ok(plain.to_vec())
}

fn decrypt_string(
  key: &SymmetricKey,
  enc: &str,
) -> anyhow::Result<String> {
  let bytes = decrypt_symmetric(key, enc)?;
  String::from_utf8(bytes).context("Decrypted value is not UTF-8")
}

/// Decrypt a `4.<data>` (RSA-OAEP-SHA1) organization key with the user's private key.
fn decrypt_org_key(
  private_key_der: &[u8],
  enc: &str,
) -> anyhow::Result<SymmetricKey> {
  let (typ, data) = enc
    .split_once('.')
    .context("Org key EncString has no type prefix")?;
  if typ != "4" {
    return Err(anyhow!(
      "Unsupported org key EncString type {typ}, expected 4 (RSA-OAEP-SHA1)"
    ));
  }
  let data = B64
    .decode(data.split('|').next().unwrap_or(data))
    .context("Org key is not base64")?;
  let private = PrivateDecryptingKey::from_pkcs8(private_key_der)
    .map_err(|_| anyhow!("User private key is not valid PKCS8"))?;
  let oaep = OaepPrivateDecryptingKey::new(private)
    .map_err(|_| anyhow!("Failed to init RSA-OAEP"))?;
  let mut out = vec![0u8; oaep.key_size_bytes()];
  let plain = oaep
    .decrypt(&OAEP_SHA1_MGF1SHA1, &data, &mut out, None)
    .map_err(|_| anyhow!("RSA-OAEP decrypt of org key failed"))?;
  SymmetricKey::from_bytes(plain)
}

#[cfg(test)]
mod tests {
  use std::sync::atomic::{AtomicUsize, Ordering};

  use super::*;
  use aws_lc_rs::{
    cipher::{EncryptionContext, PaddedBlockEncryptingKey},
    encoding::{AsDer, Pkcs8V1Der},
    rsa::{KeySize, OaepPublicEncryptingKey},
  };
  use axum::http::StatusCode;

  fn encrypt_symmetric(key: &SymmetricKey, plain: &[u8]) -> String {
    let iv = [7u8; 16];
    let cipher_key =
      UnboundCipherKey::new(&AES_256, &key.enc).unwrap();
    let encrypting =
      PaddedBlockEncryptingKey::cbc_pkcs7(cipher_key).unwrap();
    let mut data = plain.to_vec();
    encrypting
      .less_safe_encrypt(
        &mut data,
        EncryptionContext::Iv128(FixedLength::from(iv)),
      )
      .unwrap();
    let mac_key = hmac::Key::new(hmac::HMAC_SHA256, &key.mac);
    let mut signed = iv.to_vec();
    signed.extend_from_slice(&data);
    let mac = hmac::sign(&mac_key, &signed);
    format!(
      "2.{}|{}|{}",
      B64.encode(iv),
      B64.encode(&data),
      B64.encode(mac.as_ref())
    )
  }

  #[test]
  fn symmetric_round_trip_and_mac_check() {
    let master = derive_master_key(
      "hunter2",
      "Logan@Example.com",
      0,
      1000,
      None,
      None,
    )
    .unwrap();
    let key = stretch_master_key(&master).unwrap();
    let enc = encrypt_symmetric(&key, b"the quick brown fox");
    assert_eq!(
      decrypt_symmetric(&key, &enc).unwrap(),
      b"the quick brown fox"
    );
    // Tamper with the data: MAC must fail.
    let mut parts =
      enc.splitn(3, '|').map(str::to_string).collect::<Vec<_>>();
    parts[1] = B64.encode([0u8; 32]);
    assert!(decrypt_symmetric(&key, &parts.join("|")).is_err());
    // Wrong password: different key, MAC fails.
    let other = stretch_master_key(
      &derive_master_key(
        "hunter3",
        "logan@example.com",
        0,
        1000,
        None,
        None,
      )
      .unwrap(),
    )
    .unwrap();
    assert!(decrypt_symmetric(&other, &enc).is_err());
  }

  #[test]
  fn pbkdf2_matches_known_vector() {
    // PBKDF2-HMAC-SHA256("password", "salt", c = 4096, dkLen = 32)
    let key =
      derive_master_key("password", "salt", 0, 4096, None, None)
        .unwrap();
    assert_eq!(
      hex::encode(key),
      "c5e478d59288c841aa530db6845c4c8d962893a001ce4e11a4963873aa98134a"
    );
    // Email is trimmed and lower-cased before use as the salt.
    let key2 =
      derive_master_key("password", " SALT ", 0, 4096, None, None)
        .unwrap();
    assert_eq!(key, key2);
  }

  #[test]
  fn argon2_derives_32_bytes_deterministically() {
    let a = derive_master_key("pw", "a@b.c", 1, 2, Some(16), Some(1))
      .unwrap();
    let b = derive_master_key("pw", "a@b.c", 1, 2, Some(16), Some(1))
      .unwrap();
    assert_eq!(a, b);
    assert_ne!(
      a,
      derive_master_key("pw2", "a@b.c", 1, 2, Some(16), Some(1))
        .unwrap()
    );
  }

  /// What the mock vault saw.
  #[derive(Default)]
  struct Seen {
    logins: AtomicUsize,
    syncs: AtomicUsize,
    /// Requests without the expected client headers.
    bad_headers: AtomicUsize,
    /// Answer this many syncs with a 500.
    fail_syncs: AtomicUsize,
  }

  fn check_headers(seen: &Seen, headers: &axum::http::HeaderMap) {
    let version = headers
      .get("Bitwarden-Client-Version")
      .and_then(|v| v.to_str().ok());
    let device =
      headers.get("Device-Type").and_then(|v| v.to_str().ok());
    if version != Some(CLIENT_VERSION) || device != Some(DEVICE_TYPE)
    {
      seen.bad_headers.fetch_add(1, Ordering::SeqCst);
    }
  }

  /// Serve a vault holding one organization with one collection
  /// `infra` and one item `DB_PASSWORD` whose password is `s3cret`.
  async fn mock_vault(
    email: &str,
    password: &str,
  ) -> (String, Arc<Seen>) {
    let master =
      derive_master_key(password, email, 0, 1000, None, None)
        .unwrap();
    let stretched = stretch_master_key(&master).unwrap();
    let user_key = SymmetricKey {
      enc: [1; 32],
      mac: [2; 32],
    };
    let mut user_key_bytes = user_key.enc.to_vec();
    user_key_bytes.extend_from_slice(&user_key.mac);
    let rsa =
      PrivateDecryptingKey::generate(KeySize::Rsa2048).unwrap();
    let rsa_der = AsDer::<Pkcs8V1Der>::as_der(&rsa).unwrap();
    let org_key = SymmetricKey {
      enc: [3; 32],
      mac: [4; 32],
    };
    let mut org_key_bytes = org_key.enc.to_vec();
    org_key_bytes.extend_from_slice(&org_key.mac);
    let oaep =
      OaepPublicEncryptingKey::new(rsa.public_key()).unwrap();
    let mut org_key_enc = vec![0u8; oaep.ciphertext_size()];
    let org_key_enc = oaep
      .encrypt(
        &OAEP_SHA1_MGF1SHA1,
        &org_key_bytes,
        &mut org_key_enc,
        None,
      )
      .unwrap();

    let claims = base64::engine::general_purpose::URL_SAFE_NO_PAD
      .encode(serde_json::json!({ "email": email }).to_string());
    let token = serde_json::json!({
      "access_token": format!("eyJhbGciOiJSUzI1NiJ9.{claims}.sig"),
      "Key": encrypt_symmetric(&stretched, &user_key_bytes),
      "PrivateKey": encrypt_symmetric(&user_key, rsa_der.as_ref()),
      "Kdf": 0,
      "KdfIterations": 1000,
    })
    .to_string();
    let sync = serde_json::json!({
      "profile": { "organizations": [
        { "id": "org", "key": format!("4.{}", B64.encode(&org_key_enc)) },
      ]},
      "collections": [
        { "id": "col", "organizationId": "org", "name": encrypt_symmetric(&org_key, b"infra") },
      ],
      "ciphers": [{
        "id": "item",
        "collectionIds": ["col"],
        "name": encrypt_symmetric(&org_key, b"DB_PASSWORD"),
        "login": { "password": encrypt_symmetric(&org_key, b"s3cret") },
      }],
    })
    .to_string();

    let seen = Arc::new(Seen::default());
    let app = axum::Router::new()
      .route(
        "/identity/connect/token",
        axum::routing::post({
          let seen = seen.clone();
          move |headers: axum::http::HeaderMap| async move {
            check_headers(&seen, &headers);
            seen.logins.fetch_add(1, Ordering::SeqCst);
            token
          }
        }),
      )
      .route(
        "/api/sync",
        axum::routing::get({
          let seen = seen.clone();
          move |headers: axum::http::HeaderMap| async move {
            check_headers(&seen, &headers);
            seen.syncs.fetch_add(1, Ordering::SeqCst);
            // Slow enough that concurrent resolves overlap.
            tokio::time::sleep(Duration::from_millis(200)).await;
            let fail = seen
              .fail_syncs
              .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| {
                n.checked_sub(1)
              })
              .is_ok();
            if fail {
              (StatusCode::INTERNAL_SERVER_ERROR, String::new())
            } else {
              (StatusCode::OK, sync)
            }
          }
        }),
      );
    let listener =
      tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await });
    (url, seen)
  }

  async fn vaultwarden() -> (Vaultwarden, Arc<Seen>) {
    let (url, seen) = mock_vault("svc@example.com", "hunter2").await;
    let vw = Vaultwarden::new(VaultwardenConfig {
      url,
      client_id: String::from("user.test"),
      client_secret: String::from("secret"),
      master_password: String::from("hunter2"),
    });
    (vw, seen)
  }

  async fn resolve_many(
    vw: &Vaultwarden,
    n: usize,
  ) -> Vec<anyhow::Result<HashMap<String, String>>> {
    let names = [String::from("DB_PASSWORD")];
    futures_util::future::join_all(
      (0..n).map(|_| vw.resolve("infra", &names)),
    )
    .await
  }

  #[tokio::test]
  async fn concurrent_resolves_share_one_sync() {
    let (vw, seen) = vaultwarden().await;
    for res in resolve_many(&vw, 20).await {
      assert_eq!(res.unwrap()["DB_PASSWORD"], "s3cret");
    }
    assert_eq!(seen.logins.load(Ordering::SeqCst), 1);
    assert_eq!(seen.syncs.load(Ordering::SeqCst), 1);
    // A resolve within the TTL reuses the sync.
    resolve_many(&vw, 1).await.pop().unwrap().unwrap();
    assert_eq!(seen.syncs.load(Ordering::SeqCst), 1);
    assert_eq!(seen.bad_headers.load(Ordering::SeqCst), 0);
  }

  #[tokio::test]
  async fn expired_sync_is_fetched_again() {
    let (mut vw, seen) = vaultwarden().await;
    vw.sync_ttl = Duration::ZERO;
    resolve_many(&vw, 1).await.pop().unwrap().unwrap();
    resolve_many(&vw, 1).await.pop().unwrap().unwrap();
    assert_eq!(seen.syncs.load(Ordering::SeqCst), 2);
    assert_eq!(seen.logins.load(Ordering::SeqCst), 1);
    assert_eq!(seen.bad_headers.load(Ordering::SeqCst), 0);
  }

  #[tokio::test]
  async fn failed_sync_is_shared_but_not_kept() {
    let (vw, seen) = vaultwarden().await;
    seen.fail_syncs.store(1, Ordering::SeqCst);
    for res in resolve_many(&vw, 5).await {
      let err = format!("{:#}", res.unwrap_err());
      assert!(err.contains("Vaultwarden sync failed"), "{err}");
    }
    assert_eq!(seen.syncs.load(Ordering::SeqCst), 1);
    // The failure was not cached: the next resolve syncs again.
    let res = resolve_many(&vw, 1).await.pop().unwrap().unwrap();
    assert_eq!(res["DB_PASSWORD"], "s3cret");
    assert_eq!(seen.syncs.load(Ordering::SeqCst), 2);
    assert_eq!(seen.bad_headers.load(Ordering::SeqCst), 0);
  }

  #[test]
  fn reads_email_from_jwt() {
    let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD
      .encode(r#"{"sub":"x","email":"svc@example.com"}"#);
    let jwt = format!("eyJhbGciOiJSUzI1NiJ9.{payload}.sig");
    assert_eq!(email_from_jwt(&jwt).unwrap(), "svc@example.com");
  }
}
