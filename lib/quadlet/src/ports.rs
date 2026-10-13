//! Host ports a stack publishes, for the rootless agent: an unprivileged
//! user cannot bind below `net.ipv4.ip_unprivileged_port_start`.

use std::collections::BTreeMap;

use komodo_client::entities::stack::QuadletKind;

use crate::unit::{base_name, get_values, section_name};

/// The lowest host port of a `PublishPort=` value, or None when podman
/// picks the host port. Forms: `[[ip:][host]:]container[/proto]`, with a
/// bracketed IPv6 address and `start-end` ranges.
pub fn host_port(value: &str) -> Option<u16> {
  let value = value.trim();
  // Drop a bracketed IPv6 address so its colons don't count.
  let value = match value.strip_prefix('[') {
    Some(rest) => rest.split_once(']')?.1.trim_start_matches(':'),
    None => value,
  };
  let host = match value.split(':').collect::<Vec<_>>()[..] {
    [host, _container] | [_, host, _container] => host,
    _ => return None,
  };
  let host = host.trim();
  if host.is_empty() {
    return None;
  }
  host.split('-').next()?.trim().parse().ok()
}

/// One line per published host port below `floor`, as `file: PublishPort=v`.
pub fn low_host_ports(
  write: &BTreeMap<String, String>,
  floor: u16,
) -> Vec<String> {
  let mut out = Vec::new();
  for (path, contents) in write {
    let section = match QuadletKind::from_file_name(base_name(path)) {
      Some(k @ (QuadletKind::Container | QuadletKind::Pod)) => {
        section_name(k)
      }
      _ => continue,
    };
    for value in get_values(contents, section, "PublishPort") {
      if host_port(value).is_some_and(|port| port < floor) {
        out.push(format!("{path}: PublishPort={value}"));
      }
    }
  }
  out
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn host_ports() {
    for (value, port) in [
      ("80", None),
      ("8080:80", Some(8080)),
      ("80:80/tcp", Some(80)),
      ("127.0.0.1:443:443", Some(443)),
      ("127.0.0.1::443", None),
      ("[::1]:53:53/udp", Some(53)),
      ("[::1]::53", None),
      ("1000-1010:1000-1010", Some(1000)),
      ("0.0.0.0:22-23:22-23", Some(22)),
    ] {
      assert_eq!(host_port(value), port, "{value}");
    }
  }

  #[test]
  fn low_ports_per_unit() {
    let write = BTreeMap::from([
      (
        "app.pod".to_string(),
        "[Pod]\nPublishPort=80:8080\nPublishPort=8443:443\n"
          .to_string(),
      ),
      (
        "dns.container".to_string(),
        "[Container]\nPublishPort=[::]:53:53/udp\n".to_string(),
      ),
      (
        "x.network".to_string(),
        "[Network]\nPublishPort=1:1\n".to_string(),
      ),
    ]);
    assert_eq!(
      low_host_ports(&write, 1024),
      vec![
        "app.pod: PublishPort=80:8080",
        "dns.container: PublishPort=[::]:53:53/udp"
      ]
    );
    assert!(low_host_ports(&write, 53).is_empty());
  }
}
