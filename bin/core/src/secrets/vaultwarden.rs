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
use komodo_client::entities::config::core::VaultwardenConfig;
use serde::Deserialize;
use tokio::sync::Mutex;

use super::SecretProvider;

/// A session is kept for this long before logging in again.
const SESSION_TTL: Duration = Duration::from_secs(50 * 60);

pub struct Vaultwarden {
  config: VaultwardenConfig,
  http: reqwest::Client,
  session: Arc<Mutex<Option<Session>>>,
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
    Vaultwarden {
      config,
      http: reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .expect("Failed to build http client"),
      session: Default::default(),
    }
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
        ("deviceType", "21"),
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
    let session = self.session().await?;
    let sync = self.sync(&session).await?;

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
  use super::*;
  use aws_lc_rs::cipher::{
    EncryptionContext, PaddedBlockEncryptingKey,
  };

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

  #[test]
  fn reads_email_from_jwt() {
    let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD
      .encode(r#"{"sub":"x","email":"svc@example.com"}"#);
    let jwt = format!("eyJhbGciOiJSUzI1NiJ9.{payload}.sig");
    assert_eq!(email_from_jwt(&jwt).unwrap(), "svc@example.com");
  }
}
