use anyhow::Context;
use futures_util::{StreamExt, stream::FuturesUnordered};
use komodo_client::entities::config::periphery::Command;
use tracing::Instrument;

use crate::{
  config::{periphery_args, periphery_config},
  state::{core_public_keys, periphery_keys},
};

#[macro_use]
extern crate tracing;

// Vendored openssl is linked for the whole graph; keep the dependency live.
use openssl_sys as _;

mod api;
mod config;
mod connection;
mod helpers;
mod podman;
mod quadlet;
mod state;
mod stats;
mod systemd;

async fn app() -> anyhow::Result<()> {
  dotenvy::dotenv().ok();
  let config = config::periphery_config();
  mogh_logger::init(&config.logging)?;

  // The agent only talks to Core over wss://. Refuse to start on any other
  // address rather than retry it forever.
  for address in &config.core_addresses {
    transport::wss_address(address).context(
      "Invalid 'core_addresses' (PERIPHERY_CORE_ADDRESSES)",
    )?;
  }

  let startup_span = info_span!("AgentStartup");

  let mut handles = async {
    info!("Quomodo Agent version: v{}", env!("CARGO_PKG_VERSION"));

    if config.pretty_startup_config {
      info!("{:#?}", config.sanitized());
    } else {
      info!("{:?}", config.sanitized());
    }

    // Init + log public key. Will crash if invalid private key here.
    // Initialised outside the macro so a disabled log line cannot skip it.
    let public_key = periphery_keys().load().public.clone();
    info!("Public Key: {public_key}");

    // Init core public keys. Will crash if invalid core public keys here.
    core_public_keys();

    rustls::crypto::aws_lc_rs::default_provider()
      .install_default()
      .expect("Failed to install default crypto provider");

    match podman::version().await {
      Ok(version) => info!("podman {version}"),
      Err(e) => warn!(
        "podman is not reachable, stack operations will fail | {e:#}"
      ),
    }

    stats::spawn_polling_thread();

    let handles = FuturesUnordered::new();

    if config.core_addresses.is_empty() {
      error!(
        "No 'core_addresses' configured (PERIPHERY_CORE_ADDRESSES). The agent only dials out to Core, so there is nothing to do."
      );
    } else if config.connect_as.is_empty() {
      error!(
        "'core_addresses' are defined, but 'connect_as' (PERIPHERY_CONNECT_AS) is missing."
      );
    } else {
      for address in &config.core_addresses {
        match connection::client::handler(address).await {
          Ok(handle) => handles.push(handle),
          Err(e) => {
            error!("Failed to start outbound connection to {address} | {e:#}");
          }
        }
      }
    }

    handles
  }.instrument(startup_span).await;

  // Watch the threads
  while let Some(res) = handles.next().await {
    match res {
      Ok(Err(e)) => {
        error!("CONNECTION ERROR: {e:#}");
      }
      Err(e) => {
        error!("SPAWN ERROR: {e:#}");
      }
      Ok(Ok(())) => {}
    }
  }

  Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
  // Handle `quomodo-agent key gen` and `quomodo-agent key compute <private-key>`
  match &periphery_args().command {
    Some(Command::Key { command }) => {
      return mogh_pki::cli::handle(
        command,
        mogh_pki::PkiKind::Mutual,
      )
      .await;
    }
    Some(Command::PublicKey) => {
      // Same loader the agent uses at startup: reads the configured
      // private key file, or generates and writes it when missing.
      let keys = mogh_pki::RotatableKeyPair::from_private_key_spec(
        mogh_pki::PkiKind::Mutual,
        &periphery_config().private_key_spec(),
      )
      .context("Failed to load or generate the agent private key")?;
      println!("{}", keys.load().public());
      return Ok(());
    }
    None => {}
  }

  let mut term_signal = tokio::signal::unix::signal(
    tokio::signal::unix::SignalKind::terminate(),
  )?;
  tokio::select! {
    res = tokio::spawn(app()) => res?,
    _ = term_signal.recv() => {
      info!("Received SIGTERM, exiting");
      Ok(())
    },
  }
}
