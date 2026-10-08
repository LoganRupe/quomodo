use anyhow::anyhow;

pub mod auth;
pub mod channel;
pub mod timeout;
pub mod websocket;

/// The `wss://` address to dial Core on. The agent only connects over TLS:
/// the key handshake authenticates both ends but does not encrypt the
/// session, and Core sends resolved secrets over it.
///   - `wss://server.domain` => `wss://server.domain`
///   - `https://server.domain` => `wss://server.domain`
///   - `server.domain`, `11.11.11.11:9120` => `wss://...`
///   - `ws://...`, `http://...`, any other scheme => error
pub fn wss_address(address: &str) -> anyhow::Result<String> {
  let address = address.trim();
  if address.starts_with("wss://") {
    return Ok(address.to_string());
  }
  if let Some(rest) = address.strip_prefix("https://") {
    return Ok(format!("wss://{rest}"));
  }
  if address.starts_with("ws://") || address.starts_with("http://") {
    return Err(anyhow!(
      "'{address}' is not encrypted. The agent only connects to Core over wss://: serve Core with TLS (ssl_enabled) or put a TLS reverse proxy in front of it"
    ));
  }
  if address.contains("://") {
    return Err(anyhow!(
      "'{address}' has an unsupported scheme. Use wss://"
    ));
  }
  if address.is_empty() {
    return Err(anyhow!("Core address is empty"));
  }
  Ok(format!("wss://{address}"))
}

#[cfg(test)]
mod tests {
  use super::wss_address;

  #[test]
  fn only_wss_is_accepted() {
    for (input, expected) in [
      ("wss://core.example.com", "wss://core.example.com"),
      (
        "https://core.example.com:9120",
        "wss://core.example.com:9120",
      ),
      ("core.example.com", "wss://core.example.com"),
      ("10.0.0.5:9120", "wss://10.0.0.5:9120"),
      (" wss://core ", "wss://core"),
    ] {
      assert_eq!(wss_address(input).unwrap(), expected, "{input}");
    }
    for input in [
      "ws://core.example.com",
      "http://10.0.0.5:9120",
      "ftp://core",
      "",
    ] {
      assert!(wss_address(input).is_err(), "{input} was accepted");
    }
  }
}
