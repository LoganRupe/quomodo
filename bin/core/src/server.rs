//! Serving the API and UI. Plain HTTP goes through `mogh_server`. With
//! `ssl_enabled`, Core serves TLS itself so it can reload the certificate
//! when the files change: a renewed certificate takes effect without a
//! restart, and connected agents keep their sessions.

use std::{
  net::SocketAddr,
  path::{Path, PathBuf},
  str::FromStr as _,
  sync::Arc,
  time::{Duration, SystemTime},
};

use anyhow::Context as _;
use axum::{
  Router,
  http::{HeaderName, HeaderValue, header},
  response::Response,
};
use mogh_server::axum_server::{
  self, Handle, tls_rustls::RustlsConfig,
};

use crate::config::{core_config, ssl_reload_interval};

pub async fn serve(
  app: Router,
  handle: Handle<SocketAddr>,
) -> anyhow::Result<()> {
  let config = core_config();
  if !config.ssl_enabled {
    return mogh_server::serve_app(app, config, handle).await;
  }

  let app = with_security_headers(app)?
    .into_make_service_with_connect_info::<SocketAddr>();
  let socket_addr = SocketAddr::from_str(&format!(
    "{}:{}",
    config.bind_ip, config.port
  ))
  .context("Failed to parse listen address")?;

  let cert = PathBuf::from(&config.ssl_cert_file);
  let key = PathBuf::from(&config.ssl_key_file);
  let tls = RustlsConfig::from_pem_file(&cert, &key)
    .await
    .context("Invalid ssl cert / key")?;
  spawn_reload_loop(tls.clone(), cert, key);

  info!("🔒 Server SSL Enabled");
  info!("Server starting on https://{socket_addr}");
  axum_server::bind_rustls(socket_addr, tls)
    .handle(handle)
    .serve(app)
    .await
    .context("Failed to start https server")
}

/// The same security headers `mogh_server::serve_app` sets.
fn with_security_headers(app: Router) -> anyhow::Result<Router> {
  let config = core_config();
  let mut headers = Vec::<(HeaderName, HeaderValue)>::new();
  for (name, value, field) in [
    (
      header::X_CONTENT_TYPE_OPTIONS,
      config.x_content_type_options.as_str(),
      "x_content_type_options",
    ),
    (
      header::X_FRAME_OPTIONS,
      config.x_frame_options.as_str(),
      "x_frame_options",
    ),
    (
      header::X_XSS_PROTECTION,
      config.x_xss_protection.as_str(),
      "x_xss_protection",
    ),
    (
      header::CONTENT_SECURITY_POLICY,
      config.content_security_policy.as_str(),
      "content_security_policy",
    ),
    (
      header::REFERRER_POLICY,
      config.referrer_policy.as_str(),
      "referrer_policy",
    ),
  ] {
    if value.is_empty() {
      continue;
    }
    let value = HeaderValue::from_str(value)
      .with_context(|| format!("Invalid {field} value"))?;
    headers.push((name, value));
  }
  let headers = Arc::new(headers);
  Ok(app.layer(axum::middleware::map_response(
    move |mut res: Response| {
      let headers = headers.clone();
      async move {
        for (name, value) in headers.iter() {
          res.headers_mut().insert(name.clone(), value.clone());
        }
        res
      }
    },
  )))
}

fn modified(
  cert: &Path,
  key: &Path,
) -> (Option<SystemTime>, Option<SystemTime>) {
  let mtime = |path: &Path| {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
  };
  (mtime(cert), mtime(key))
}

/// Reload the certificate when either file's mtime changes. A pair that
/// fails to load (a renewal half written, a key that does not match) keeps
/// the current certificate and is tried again on the next check.
fn spawn_reload_loop(tls: RustlsConfig, cert: PathBuf, key: PathBuf) {
  let every = Duration::from_millis(ssl_reload_interval());
  tokio::spawn(async move {
    let mut loaded = modified(&cert, &key);
    let mut interval = tokio::time::interval(every);
    interval.tick().await;
    loop {
      interval.tick().await;
      let current = modified(&cert, &key);
      if current == loaded {
        continue;
      }
      match tls.reload_from_pem_file(&cert, &key).await {
        Ok(()) => {
          info!("Reloaded TLS certificate from {}", cert.display());
          loaded = current;
        }
        Err(e) => warn!(
          "TLS certificate files changed but failed to load, still serving the previous certificate | {e:#}"
        ),
      }
    }
  });
}
