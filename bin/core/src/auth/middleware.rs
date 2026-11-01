use std::{
  collections::HashSet,
  net::IpAddr,
  sync::LazyLock,
  time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context as _, anyhow};
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode};
use komodo_client::entities::user::User;
use mogh_auth_server::{AuthImpl as _, RequestAuthentication};
use mogh_error::{AddStatusCode as _, StatusCode};
use serde::Deserialize;

use crate::{
  auth::{JWT_PROVIDER, JWT_SECRET, KomodoAuthImpl},
  helpers::query::get_user,
};

pub async fn extract_user_from_auth(
  auth: RequestAuthentication,
  ip: IpAddr,
  require_user_enabled: bool,
) -> mogh_error::Result<User> {
  let user_id = KomodoAuthImpl
    .get_user_id_from_request_authentication(auth, ip)
    .await?;
  if require_user_enabled {
    check_enabled(&user_id).await
  } else {
    get_user(&user_id).await
  }
  .status_code(StatusCode::UNAUTHORIZED)
}

pub async fn auth_jwt_check_enabled(
  jwt: &str,
) -> mogh_error::Result<User> {
  let user_id = jwt_user_id(jwt)?;
  check_enabled(&user_id)
    .await
    .status_code(StatusCode::UNAUTHORIZED)
}

pub async fn auth_api_key_check_enabled(
  key: String,
  secret: String,
  ip: IpAddr,
) -> mogh_error::Result<User> {
  extract_user_from_auth(
    RequestAuthentication::ApiKey { key, secret },
    ip,
    true,
  )
  .await
}

/// The user id of a session token, issued now or before 0.5.
pub fn jwt_user_id(jwt: &str) -> mogh_error::Result<String> {
  JWT_PROVIDER
    .decode_sub(jwt)
    .or_else(|e| legacy_jwt_user_id(jwt).map_err(|_| e))
    .status_code(StatusCode::UNAUTHORIZED)
}

/// The claims of a session token issued before 0.5, by
/// mogh_auth_server 1.x. The times are unix milliseconds.
#[derive(Deserialize)]
struct LegacyJwtClaims {
  sub: String,
  exp: u128,
  iss: Option<serde_json::Value>,
  aud: Option<serde_json::Value>,
}

/// Session tokens from before 0.5 carry only `sub`, `iat` and `exp`
/// (in milliseconds), signed with the same `jwt_secret`. They are
/// accepted until they expire, so upgrading Core doesn't log
/// everyone out. Tokens issued since always carry `iss` and `aud`,
/// and are refused here.
fn legacy_jwt_user_id(jwt: &str) -> anyhow::Result<String> {
  static VALIDATION: LazyLock<Validation> = LazyLock::new(|| {
    let mut validation = Validation::new(Algorithm::HS256);
    // exp is in milliseconds, checked below.
    validation.validate_exp = false;
    validation.validate_aud = false;
    validation.required_spec_claims = HashSet::new();
    validation
  });
  static DECODING_KEY: LazyLock<DecodingKey> =
    LazyLock::new(|| DecodingKey::from_secret(JWT_SECRET.as_bytes()));

  let claims =
    decode::<LegacyJwtClaims>(jwt, &DECODING_KEY, &VALIDATION)
      .context("Invalid user credentials")?
      .claims;

  // The same 10 second clock skew tolerance as before.
  let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis();
  if claims.iss.is_some()
    || claims.aud.is_some()
    // An exp in seconds is not from a pre 0.5 token.
    || claims.exp < 1_000_000_000_000
    || claims.exp <= now.saturating_sub(10_000)
  {
    return Err(anyhow!("Invalid user credentials"));
  }
  Ok(claims.sub)
}

async fn check_enabled(user_id: &str) -> anyhow::Result<User> {
  let user = get_user(user_id).await?;
  if user.enabled {
    Ok(user)
  } else {
    Err(anyhow!("Invalid user credentials"))
  }
}

#[cfg(test)]
mod tests {
  use jsonwebtoken::{EncodingKey, Header, encode};
  use serde_json::json;

  use super::*;

  fn now_ms() -> u128 {
    SystemTime::now()
      .duration_since(UNIX_EPOCH)
      .unwrap()
      .as_millis()
  }

  fn sign(claims: serde_json::Value) -> String {
    encode(
      &Header::default(),
      &claims,
      &EncodingKey::from_secret(JWT_SECRET.as_bytes()),
    )
    .unwrap()
  }

  #[test]
  fn accepts_unexpired_legacy_token() {
    let now = now_ms();
    let jwt = sign(
      json!({ "sub": "user-1", "iat": now, "exp": now + 60_000 }),
    );
    assert_eq!(legacy_jwt_user_id(&jwt).unwrap(), "user-1");
    assert_eq!(jwt_user_id(&jwt).unwrap(), "user-1");
  }

  #[test]
  fn refuses_expired_legacy_token() {
    let now = now_ms();
    let jwt = sign(
      json!({ "sub": "user-1", "iat": now - 120_000, "exp": now - 60_000 }),
    );
    assert!(legacy_jwt_user_id(&jwt).is_err());
  }

  #[test]
  fn refuses_tokens_with_iss_or_aud() {
    let now = now_ms();
    for claims in [
      json!({ "sub": "u", "iat": now, "exp": now + 60_000, "iss": "x" }),
      json!({ "sub": "u", "iat": now, "exp": now + 60_000, "aud": "x" }),
    ] {
      assert!(legacy_jwt_user_id(&sign(claims)).is_err());
    }
  }

  #[test]
  fn refuses_exp_in_seconds() {
    let now = now_ms() / 1000;
    let jwt =
      sign(json!({ "sub": "u", "iat": now, "exp": now + 60 }));
    assert!(legacy_jwt_user_id(&jwt).is_err());
  }

  #[test]
  fn refuses_other_secret() {
    let now = now_ms();
    let jwt = encode(
      &Header::default(),
      &json!({ "sub": "u", "iat": now, "exp": now + 60_000 }),
      &EncodingKey::from_secret(b"some other secret"),
    )
    .unwrap();
    assert!(legacy_jwt_user_id(&jwt).is_err());
  }

  #[test]
  fn current_tokens_still_decode() {
    let jwt = JWT_PROVIDER.encode_sub("user-2").unwrap().jwt;
    assert_eq!(jwt_user_id(&jwt).unwrap(), "user-2");
    assert!(legacy_jwt_user_id(&jwt).is_err());
  }
}
