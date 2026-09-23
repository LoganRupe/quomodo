//! Parsing of quadlet unit files.
//!
//! Quadlet files are systemd-style INI: `[Section]` headers and `Key=Value`
//! lines, `#` / `;` comments, keys may repeat.

use komodo_client::entities::stack::{QuadletKind, QuadletUnit};

/// A unit file parsed far enough for deploy planning.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ParsedUnit {
  pub unit: QuadletUnit,
  /// Other unit files this one references (`Pod=`, `Network=x.network`, ...).
  pub deps: Vec<String>,
  /// Podman secret names referenced with `Secret=`.
  pub secrets: Vec<String>,
}

/// The quadlet section holding the object keys for this kind.
pub fn section_name(kind: QuadletKind) -> &'static str {
  match kind {
    QuadletKind::Container => "Container",
    QuadletKind::Pod => "Pod",
    QuadletKind::Network => "Network",
    QuadletKind::Volume => "Volume",
    QuadletKind::Build => "Build",
    QuadletKind::Image => "Image",
    QuadletKind::Kube => "Kube",
  }
}

/// The file name without directories.
pub fn base_name(path: &str) -> &str {
  path.rsplit('/').next().unwrap_or(path)
}

pub fn stem(file_name: &str) -> &str {
  file_name
    .rsplit_once('.')
    .map(|(s, _)| s)
    .unwrap_or(file_name)
}

/// Every `Key=` value in `[Section]`, in file order.
pub fn get_values<'a>(
  contents: &'a str,
  section: &str,
  key: &str,
) -> Vec<&'a str> {
  let mut in_section = false;
  let mut out = Vec::new();
  for line in contents.lines() {
    let line = line.trim();
    if line.is_empty()
      || line.starts_with('#')
      || line.starts_with(';')
    {
      continue;
    }
    if let Some(header) = line.strip_prefix('[') {
      in_section = header.trim_end_matches(']').trim() == section;
      continue;
    }
    if !in_section {
      continue;
    }
    if let Some((k, v)) = line.split_once('=')
      && k.trim() == key
    {
      out.push(v.trim());
    }
  }
  out
}

pub fn get_value<'a>(
  contents: &'a str,
  section: &str,
  key: &str,
) -> Option<&'a str> {
  get_values(contents, section, key).into_iter().next()
}

/// Parse a unit file. Returns None if the file name is not a quadlet unit.
pub fn parse_unit(path: &str, contents: &str) -> Option<ParsedUnit> {
  let file_name = base_name(path);
  let kind = QuadletKind::from_file_name(file_name)?;
  let stem = stem(file_name);
  let section = section_name(kind);
  let default_name = format!("systemd-{stem}");

  let mut deps = Vec::new();
  let mut image = String::new();
  let mut pod = None;

  let name = match kind {
    QuadletKind::Container => {
      image = get_value(contents, section, "Image")
        .unwrap_or_default()
        .to_string();
      if image.ends_with(".image") || image.ends_with(".build") {
        deps.push(image.clone());
      }
      if let Some(p) = get_value(contents, section, "Pod") {
        pod = Some(p.to_string());
        deps.push(p.to_string());
      }
      get_value(contents, section, "ContainerName")
        .map(str::to_string)
        .unwrap_or(default_name)
    }
    QuadletKind::Pod => get_value(contents, section, "PodName")
      .map(str::to_string)
      .unwrap_or(default_name),
    QuadletKind::Network => {
      get_value(contents, section, "NetworkName")
        .map(str::to_string)
        .unwrap_or(default_name)
    }
    QuadletKind::Volume => get_value(contents, section, "VolumeName")
      .map(str::to_string)
      .unwrap_or(default_name),
    QuadletKind::Image => {
      image = get_value(contents, section, "Image")
        .unwrap_or_default()
        .to_string();
      image.clone()
    }
    QuadletKind::Build => get_value(contents, section, "ImageTag")
      .map(str::to_string)
      .unwrap_or_else(|| format!("localhost/{stem}")),
    QuadletKind::Kube => stem.to_string(),
  };

  // Network= / Volume= references to other quadlet units.
  for value in get_values(contents, section, "Network") {
    let target = value.split(':').next().unwrap_or(value);
    if target.ends_with(".network") {
      deps.push(target.to_string());
    }
  }
  for value in get_values(contents, section, "Volume") {
    let target = value.split(':').next().unwrap_or(value);
    if target.ends_with(".volume") {
      deps.push(target.to_string());
    }
  }

  let secrets = get_values(contents, section, "Secret")
    .into_iter()
    .map(|value| {
      value.split(',').next().unwrap_or(value).trim().to_string()
    })
    .filter(|s| !s.is_empty())
    .collect();

  Some(ParsedUnit {
    unit: QuadletUnit {
      file: path.to_string(),
      kind,
      service: kind.service_name(stem),
      name,
      image,
      pod,
    },
    deps,
    secrets,
  })
}

/// Every `.container` gets `UserNS=auto` so root inside the container
/// is an unprivileged host uid. An explicit `UserNS=` is left alone.
pub fn with_userns_auto(contents: &str) -> String {
  if get_value(contents, "Container", "UserNS").is_some() {
    return contents.to_string();
  }
  let mut out = String::with_capacity(contents.len() + 16);
  let mut injected = false;
  for line in contents.split_inclusive('\n') {
    out.push_str(line);
    if !injected && line.trim() == "[Container]" {
      if !line.ends_with('\n') {
        out.push('\n');
      }
      out.push_str("UserNS=auto\n");
      injected = true;
    }
  }
  if !injected {
    if !out.ends_with('\n') && !out.is_empty() {
      out.push('\n');
    }
    out.push_str("[Container]\nUserNS=auto\n");
  }
  out
}

#[cfg(test)]
mod tests {
  use super::*;

  const WEB: &str = "[Unit]\nDescription=web\n\n[Container]\nImage=docker.io/library/nginx:latest\nContainerName=web\nPod=app.pod\nNetwork=app.network\nVolume=data.volume:/data\nVolume=/etc/localtime:/etc/localtime:ro\nSecret=DB_PASSWORD,type=env,target=DB_PASSWORD\nSecret=app-config,type=mount,target=/etc/app.yml\n\n[Install]\nWantedBy=multi-user.target\n";

  #[test]
  fn parses_container_unit() {
    let parsed = parse_unit("web.container", WEB).unwrap();
    assert_eq!(parsed.unit.kind, QuadletKind::Container);
    assert_eq!(parsed.unit.service, "web.service");
    assert_eq!(parsed.unit.name, "web");
    assert_eq!(parsed.unit.image, "docker.io/library/nginx:latest");
    assert_eq!(parsed.unit.pod.as_deref(), Some("app.pod"));
    assert_eq!(
      parsed.deps,
      vec!["app.pod", "app.network", "data.volume"]
    );
    assert_eq!(parsed.secrets, vec!["DB_PASSWORD", "app-config"]);
  }

  #[test]
  fn default_names_and_services() {
    let pod = parse_unit("app.pod", "[Pod]\n").unwrap();
    assert_eq!(pod.unit.service, "app-pod.service");
    assert_eq!(pod.unit.name, "systemd-app");
    let net = parse_unit(
      "sub/app.network",
      "[Network]\nSubnet=10.1.0.0/24\n",
    )
    .unwrap();
    assert_eq!(net.unit.service, "app-network.service");
    assert_eq!(net.unit.file, "sub/app.network");
    assert!(parse_unit("config.yml", "a: 1").is_none());
    let c =
      parse_unit("db.container", "[Container]\nImage=db.build\n")
        .unwrap();
    assert_eq!(c.unit.name, "systemd-db");
    assert_eq!(c.deps, vec!["db.build"]);
  }

  #[test]
  fn injects_userns_auto_once() {
    let out = with_userns_auto(WEB);
    assert!(out.contains("[Container]\nUserNS=auto\nImage="));
    assert_eq!(with_userns_auto(&out), out);
    let explicit = "[Container]\nUserNS=keep-id\nImage=x\n";
    assert_eq!(with_userns_auto(explicit), explicit);
    assert_eq!(
      with_userns_auto("[Unit]\nDescription=x\n"),
      "[Unit]\nDescription=x\n[Container]\nUserNS=auto\n"
    );
  }
}
