use std::{path::Path, sync::Arc, time::Duration};

use anyhow::{Context, anyhow};
use axum::http::{self, HeaderValue};
use bytes::Bytes;
use encoding::CastBytes as _;
use futures_util::{
  SinkExt, Stream, StreamExt, TryStreamExt,
  stream::{SplitSink, SplitStream},
};
use mogh_error::AddStatusCodeError;
use periphery_client::transport::EncodedTransportMessage;
use rustls::{
  ClientConfig, RootCertStore,
  pki_types::{CertificateDer, pem::PemObject},
};
use tokio::net::TcpStream;
use tokio_tungstenite::{
  Connector, MaybeTlsStream, WebSocketStream,
  tungstenite::{
    self, client::IntoClientRequest as _,
    handshake::client::Response, protocol::CloseFrame,
  },
};
use tokio_util::sync::CancellationToken;

use crate::timeout::MaybeWithTimeout;

use super::{
  Websocket, WebsocketMessage, WebsocketReceiver, WebsocketSender,
};

pub type InnerWebsocket = WebSocketStream<MaybeTlsStream<TcpStream>>;

pub struct TungsteniteWebsocket(pub InnerWebsocket);

impl Websocket for TungsteniteWebsocket {
  fn split(self) -> (impl WebsocketSender, impl WebsocketReceiver) {
    let (tx, rx) = self.0.split();
    (
      TungsteniteWebsocketSender(tx),
      TungsteniteWebsocketReceiver::new(rx),
    )
  }

  fn recv_inner(
    &mut self,
  ) -> MaybeWithTimeout<
    impl Future<Output = anyhow::Result<WebsocketMessage>>,
  > {
    MaybeWithTimeout::new(try_next(&mut self.0))
  }

  async fn send(&mut self, bytes: Bytes) -> anyhow::Result<()> {
    self
      .0
      .send(tungstenite::Message::Binary(bytes))
      .await
      .context("Failed to send message over websocket")
  }

  async fn close(&mut self) -> anyhow::Result<()> {
    self
      .0
      .close(None)
      .await
      .context("Failed to send websocket close frame")
  }
}

pub type InnerWebsocketSender = SplitSink<
  WebSocketStream<MaybeTlsStream<TcpStream>>,
  tungstenite::Message,
>;

pub struct TungsteniteWebsocketSender(pub InnerWebsocketSender);

impl WebsocketSender for TungsteniteWebsocketSender {
  async fn ping(&mut self) -> anyhow::Result<()> {
    self
      .0
      .send(tungstenite::Message::Ping(Bytes::new()))
      .await
      .context("Failed to send ping over websocket")
  }

  async fn send(&mut self, bytes: Bytes) -> anyhow::Result<()> {
    self
      .0
      .send(tungstenite::Message::Binary(bytes))
      .await
      .context("Failed to send message over websocket")
  }

  async fn close(&mut self) -> anyhow::Result<()> {
    self
      .0
      .send(tungstenite::Message::Close(None))
      .await
      .context("Failed to send websocket close frame")
  }
}

async fn try_next<S>(
  stream: &mut S,
) -> anyhow::Result<WebsocketMessage>
where
  S: Stream<Item = Result<tungstenite::Message, tungstenite::Error>>
    + Unpin,
{
  loop {
    match stream.try_next().await? {
      Some(tungstenite::Message::Binary(bytes)) => {
        return Ok(WebsocketMessage::Message(
          EncodedTransportMessage::from_vec(bytes.into()),
        ));
      }
      Some(tungstenite::Message::Text(text)) => {
        let bytes: Bytes = text.into();
        return Ok(WebsocketMessage::Message(
          EncodedTransportMessage::from_vec(bytes.into()),
        ));
      }
      Some(tungstenite::Message::Ping(_)) => {
        return Ok(WebsocketMessage::Ping);
      }
      Some(tungstenite::Message::Close(_)) => {
        return Ok(WebsocketMessage::Close);
      }
      None => return Ok(WebsocketMessage::Closed),
      // Ignored messages
      Some(tungstenite::Message::Pong(_))
      | Some(tungstenite::Message::Frame(_)) => continue,
    }
  }
}

pub type InnerWebsocketReceiver =
  SplitStream<WebSocketStream<MaybeTlsStream<TcpStream>>>;

pub struct TungsteniteWebsocketReceiver {
  receiver: InnerWebsocketReceiver,
  cancel: Option<CancellationToken>,
}

impl TungsteniteWebsocketReceiver {
  pub fn new(receiver: InnerWebsocketReceiver) -> Self {
    Self {
      receiver,
      cancel: None,
    }
  }
}

impl WebsocketReceiver for TungsteniteWebsocketReceiver {
  type CloseFrame = CloseFrame;

  fn set_cancel(&mut self, cancel: CancellationToken) {
    self.cancel = Some(cancel);
  }

  async fn recv(&mut self) -> anyhow::Result<WebsocketMessage> {
    let fut = try_next(&mut self.receiver);
    if let Some(cancel) = &self.cancel {
      tokio::select! {
        res = fut => res,
        _ = cancel.cancelled() => Err(anyhow!("Cancelled before receive"))
      }
    } else {
      fut.await
    }
  }
}

/// How long one connection attempt (TCP, TLS and the websocket upgrade)
/// may take. Without it a SYN sent before the route to Core exists, as at
/// boot when the agent starts beside its VPN, waits out the kernel's SYN
/// retries (about two minutes) before the caller gets to retry.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

impl TungsteniteWebsocket {
  /// Connect to a `wss://` url, verifying the server certificate.
  /// With `ca_file`, only the CA certificates in that PEM file are
  /// trusted; without it, the system roots. Any other scheme is refused.
  pub async fn connect_wss(
    url: &str,
    ca_file: Option<&Path>,
  ) -> mogh_error::Result<(Self, HeaderValue)> {
    if !url.starts_with("wss://") {
      return Err(
        anyhow!(
          "Refusing to connect to {url}: only wss:// is allowed"
        )
        .into(),
      );
    }
    let connector = match ca_file {
      Some(path) => {
        Some(Connector::Rustls(Arc::new(ca_client_config(path)?)))
      }
      None => None,
    };
    let res = tokio::time::timeout(
      CONNECT_TIMEOUT,
      tokio_tungstenite::connect_async_tls_with_config(
        make_request(url)?,
        None,
        false,
        connector,
      ),
    )
    .await
    .map_err(|_| {
      anyhow!(
        "Timed out after {}s connecting to websocket | url: {url}",
        CONNECT_TIMEOUT.as_secs()
      )
    })?;
    Self::handle_connection_result(url, res)
  }

  fn handle_connection_result(
    url: &str,
    res: Result<
      (WebSocketStream<MaybeTlsStream<TcpStream>>, Response),
      tungstenite::Error,
    >,
  ) -> mogh_error::Result<(Self, HeaderValue)> {
    let (ws, mut response) = res
      .map_err(|e| {
        let status = if let tungstenite::Error::Http(response) = &e {
          response.status()
        } else {
          return anyhow::Error::from(e).into();
        };
        e.status_code(status)
      })
      .map_err(|mut e| {
        e.error = e.error.context({
          format!("Failed to connect to websocket | url: {url}")
        });
        e
      })?;

    let accept = response
      .headers_mut()
      .remove("sec-websocket-accept")
      .context("Headers do not contain Sec-Websocket-Accept")?;

    Ok((Self(ws), accept))
  }
}

fn make_request(url: &str) -> mogh_error::Result<http::Request<()>> {
  let mut request =
    url.into_client_request().context("Invalid websocket URL")?;
  request.headers_mut().insert(
    "user-agent",
    HeaderValue::from_static(concat!(
      "komodo/",
      env!("CARGO_PKG_VERSION")
    )),
  );
  Ok(request)
}

/// A client config trusting only the CA certificates in a PEM file.
fn ca_client_config(path: &Path) -> anyhow::Result<ClientConfig> {
  let mut roots = RootCertStore::empty();
  let certs =
    CertificateDer::pem_file_iter(path).with_context(|| {
      format!("Failed to read Core CA file {}", path.display())
    })?;
  for cert in certs {
    let cert = cert.with_context(|| {
      format!(
        "Invalid certificate in Core CA file {}",
        path.display()
      )
    })?;
    roots.add(cert).with_context(|| {
      format!(
        "Unusable certificate in Core CA file {}",
        path.display()
      )
    })?;
  }
  if roots.is_empty() {
    return Err(anyhow!(
      "Core CA file {} holds no certificates",
      path.display()
    ));
  }
  Ok(
    ClientConfig::builder()
      .with_root_certificates(roots)
      .with_no_client_auth(),
  )
}
