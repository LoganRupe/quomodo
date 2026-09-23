use std::{sync::Arc, time::Duration};

use anyhow::{Context, anyhow};
use encoding::Decode as _;
use mogh_resolver::HasResponse;
use serde::{Serialize, de::DeserializeOwned};
use serde_json::json;
use transport::channel::channel;
use uuid::Uuid;

use crate::{
  connection::{
    PeripheryConnectionArgs, ResponseChannels,
  },
  state::periphery_connections,
};


#[derive(Debug)]
pub struct PeripheryClient {
  /// Usually the server id
  pub id: String,
  pub responses: Arc<ResponseChannels>,
}

impl PeripheryClient {
  /// The agent always dials in to Core, so the connection must already exist.
  pub async fn new(
    args: PeripheryConnectionArgs<'_>,
  ) -> anyhow::Result<PeripheryClient> {
    let connections = periphery_connections();

    let id = args.id.to_string();

    let Some(connection) = connections.get(&id).await else {
      return Err(anyhow!("Server {id} is not connected"));
    };

    // Ensure the connection args are unchanged.
    if args.matches(&connection.args) {
      return Ok(PeripheryClient {
        id,
        responses: connection.responses.clone(),
      });
    }

    // The args have changed (eg the expected public key).
    // Remove this connection, wait and see if the agent reconnects.
    connections.remove(&id).await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    let connection = connections
      .get(&id)
      .await
      .with_context(|| format!("Server {id} is not connected"))?;
    Ok(PeripheryClient {
      id,
      responses: connection.responses.clone(),
    })
  }

  pub async fn request<T>(
    &self,
    request: T,
  ) -> anyhow::Result<T::Response>
  where
    T: std::fmt::Debug + Serialize + HasResponse,
    T::Response: DeserializeOwned,
  {
    let connection =
      periphery_connections().get(&self.id).await.with_context(
        || format!("No connection found for server {}", self.id),
      )?;

    // Polls connected 3 times before bailing
    connection.bail_if_not_connected().await?;

    let channel_id = Uuid::new_v4();
    let (response_sender, mut response_receiever) = channel();
    self.responses.insert(channel_id, response_sender).await;

    if let Err(e) = connection
      .sender
      .send_request(
        channel_id,
        &json!({
          "type": T::req_type(),
          "params": request
        }),
      )
      .await
      .context("Failed to send request over channel")
    {
      self.responses.remove(&channel_id).await;
      return Err(e);
    }

    let res = async {
      // Poll for the associated response
      loop {
        let message = response_receiever
          .recv()
          // Periphery request handler sends pings every 4s
          // *on this channel specifically* so Core knows
          // request is being processed. Hardcoded 11s
          // allows for missed 5s ping due to network reconnect.
          .with_timeout(Duration::from_secs(10))
          .await?;

        let Some(message) = message.decode()? else {
          // Just a ping from periphery request handler
          continue;
        };

        return message.decode();
      }
    }
    .await;

    self.responses.remove(&channel_id).await;

    res
  }
}
