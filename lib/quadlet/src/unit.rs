//! Parsing of quadlet unit files.
//!
//! Quadlet files are systemd-style INI: `[Section]` headers and `Key=Value`
//! lines, `#` / `;` comments, keys may repeat.

use std::collections::BTreeMap;

use komodo_client::entities::stack::{QuadletKind, QuadletUnit};

use crate::ignore::IgnoreRules;

/// A unit file parsed far enough for deploy planning.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ParsedUnit {
  pub unit: QuadletUnit,
  /// Other unit files this one references (`Pod=`, `Network=x.network`, ...).
  pub deps: Vec<String>,
  /// Podman secret names referenced with `Secret=`.
  pub secrets: Vec<String>,
  /// Files or directories inside the stack directory this unit reads at
  /// start: relative `Volume=` bind-mount sources, and for a `.build` its
  /// `File=`, `IgnoreFile=` and build context. Relative to the stack
  /// directory; an empty string is the whole stack directory.
  pub mounts: Vec<String>,
  /// For a `.build`: the inputs inside the stack directory.
  pub build: Option<BuildInputs>,
}

impl ParsedUnit {
  /// Whether a change to the stack file at `path` affects this unit.
  /// `files` is the stack's text files by path, for a `.build`'s ignore
  /// file.
  pub fn reads(
    &self,
    path: &str,
    files: &BTreeMap<String, String>,
  ) -> bool {
    if !self.mounts.iter().any(|m| covers(m, path)) {
      return false;
    }
    match &self.build {
      Some(build) => build.reads(path, files),
      None => true,
    }
  }
}

/// A `.build`'s inputs, relative to the stack directory. None where the
/// value is absent or points outside the stack directory.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BuildInputs {
  /// The build context directory; empty for the whole stack directory.
  pub context: Option<String>,
  /// `File=`
  pub file: Option<String>,
  /// `IgnoreFile=`. Without it podman reads `.containerignore`, else
  /// `.dockerignore`, from the context root.
  pub ignore_file: Option<String>,
}

impl BuildInputs {
  fn mounts(&self) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(context) = &self.context {
      out.push(context.clone());
    }
    for path in [&self.file, &self.ignore_file].into_iter().flatten()
    {
      if !out.iter().any(|c| covers(c, path)) {
        out.push(path.clone());
      }
    }
    out
  }

  /// The ignore file podman reads, if it exists in `files`.
  pub fn active_ignore_file(
    &self,
    files: &BTreeMap<String, String>,
  ) -> Option<String> {
    if let Some(file) = &self.ignore_file {
      return Some(file.clone());
    }
    let context = self.context.as_deref()?;
    [".containerignore", ".dockerignore"]
      .into_iter()
      .filter_map(|name| join(context, name))
      .find(|path| files.contains_key(path))
  }

  /// Whether the build sees the stack file at `path`. The Containerfile
  /// and the ignore file always count, as podman reads both even when a
  /// pattern excludes them.
  pub fn reads(
    &self,
    path: &str,
    files: &BTreeMap<String, String>,
  ) -> bool {
    let ignore_file = self.active_ignore_file(files);
    if self.file.as_deref() == Some(path)
      || ignore_file.as_deref() == Some(path)
    {
      return true;
    }
    let Some(context) = self.context.as_deref() else {
      return false;
    };
    let Some(rel) = relative_to(context, path) else {
      return false;
    };
    if self.file.is_none()
      && matches!(rel, "Containerfile" | "Dockerfile")
    {
      return true;
    }
    match ignore_file.and_then(|f| files.get(&f)) {
      Some(rules) => !IgnoreRules::parse(rules).excludes(rel),
      None => true,
    }
  }
}

/// `path` relative to `dir`, if it is under it.
fn relative_to<'a>(dir: &str, path: &'a str) -> Option<&'a str> {
  if dir.is_empty() {
    return Some(path);
  }
  path.strip_prefix(dir)?.strip_prefix('/')
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

/// The directory holding a stack file, relative to the stack directory.
/// Empty for a file at the top level.
pub fn dir_name(path: &str) -> &str {
  path.rsplit_once('/').map(|(d, _)| d).unwrap_or("")
}

/// Whether a changed stack file at `path` falls under `mount`
/// (see `ParsedUnit::mounts`).
pub fn covers(mount: &str, path: &str) -> bool {
  mount.is_empty()
    || path == mount
    || path.strip_prefix(mount).is_some_and(|r| r.starts_with('/'))
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
  let unit_dir = dir_name(path);
  let stem = stem(file_name);
  let section = section_name(kind);
  let default_name = format!("systemd-{stem}");

  let mut deps = Vec::new();
  let mut mounts = Vec::new();
  let mut build = None;
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
    QuadletKind::Build => {
      let inputs = build_inputs(contents, unit_dir);
      mounts.extend(inputs.mounts());
      build = Some(inputs);
      get_value(contents, section, "ImageTag")
        .map(str::to_string)
        .unwrap_or_else(|| format!("localhost/{stem}"))
    }
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
    } else if let Some(rel) = relative_mount(target)
      && let Some(rel) = join(unit_dir, &rel)
    {
      mounts.push(rel);
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
    mounts,
    build,
  })
}

/// A `Volume=` source that lives inside the stack directory:
/// quadlet resolves relative sources against the unit's directory.
/// Absolute paths, systemd specifiers and named volumes are not.
fn relative_mount(source: &str) -> Option<String> {
  let source = source.trim();
  if source.is_empty()
    || source.starts_with('/')
    || source.starts_with('%')
    || source.starts_with('$')
  {
    return None;
  }
  let rel = source.strip_prefix("./").unwrap_or(source);
  // A bare word with no slash or dot is a named volume, not a path.
  if !source.starts_with("./")
    && !rel.contains('/')
    && !rel.contains('.')
  {
    return None;
  }
  Some(rel.trim_end_matches('/').to_string())
}

/// What a `.build` reads from the stack directory, as quadlet resolves it
/// (checked against `podman-system-generator --dryrun`, podman 5.7):
/// - `SetWorkingDirectory=unit`: context is the unit's directory.
/// - `SetWorkingDirectory=file`: context is the directory of `File=`,
///   itself relative to the unit's directory.
/// - `SetWorkingDirectory=<relative path>`: context is that path under
///   the unit's directory.
/// - A relative `File=` counts too, in case it sits outside the context.
/// - A relative `IgnoreFile=` resolves against the service's working
///   directory: the directory of `File=` for `SetWorkingDirectory=file`,
///   the unit's directory otherwise. Without `SetWorkingDirectory=` there
///   is none, so it is outside the stack directory.
///
/// Absolute paths, URLs and specifiers are outside the stack directory.
fn build_inputs(contents: &str, unit_dir: &str) -> BuildInputs {
  let local = |dir: &str, value: &str| {
    relative_path(value).and_then(|rel| join(dir, rel))
  };
  let file = get_value(contents, "Build", "File")
    .and_then(|f| local(unit_dir, f));
  let working_dir =
    get_value(contents, "Build", "SetWorkingDirectory");
  let context = match working_dir {
    Some("unit") => Some(unit_dir.to_string()),
    Some("file") => file.as_deref().map(|f| dir_name(f).to_string()),
    Some(other) => local(unit_dir, other),
    None => None,
  };
  let ignore_file = match working_dir {
    Some("file") => file.as_deref().map(dir_name),
    Some(_) => Some(unit_dir),
    None => None,
  }
  .and_then(|dir| {
    get_value(contents, "Build", "IgnoreFile")
      .and_then(|f| local(dir, f))
  });
  BuildInputs {
    context,
    file,
    ignore_file,
  }
}

/// A path value quadlet resolves against the unit's directory.
fn relative_path(value: &str) -> Option<&str> {
  let value = value.trim();
  if value.is_empty()
    || value.starts_with('/')
    || value.starts_with('%')
    || value.starts_with('$')
    || value.contains("://")
  {
    return None;
  }
  Some(value)
}

/// `rel` under `dir`, both relative to the stack directory, with `.` and
/// `..` resolved. None if it climbs out of the stack directory.
fn join(dir: &str, rel: &str) -> Option<String> {
  let mut parts = Vec::new();
  for part in dir.split('/').chain(rel.split('/')) {
    match part {
      "" | "." => {}
      ".." => {
        parts.pop()?;
      }
      part => parts.push(part),
    }
  }
  Some(parts.join("/"))
}

/// One line per unit of a stack whose systemd service another installed
/// quadlet file already generates. Quadlet names every service after its
/// file name alone, so `web.container` in two stacks on one host is one
/// `web.service`, and `Network=app.network` could mean either stack's.
/// `installed` is every quadlet file outside the stack's own directory, as
/// (where it is, file name).
pub fn name_collisions<'a>(
  units: &[ParsedUnit],
  installed: impl IntoIterator<Item = (&'a str, &'a str)>,
) -> Vec<String> {
  let mut taken = BTreeMap::new();
  for (owner, file) in installed {
    let file = base_name(file);
    if let Some(kind) = QuadletKind::from_file_name(file) {
      taken
        .entry(kind.service_name(stem(file)))
        .or_insert((owner, file));
    }
  }
  units
    .iter()
    .filter_map(|u| {
      let (owner, file) = taken.get(&u.unit.service)?;
      Some(format!(
        "{}: {} is already {file}'s in {owner}",
        u.unit.file, u.unit.service
      ))
    })
    .collect()
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
    assert!(parsed.mounts.is_empty());
  }

  #[test]
  fn records_relative_bind_mounts() {
    let unit = "[Container]\nImage=x\nVolume=./Caddyfile:/etc/caddy/Caddyfile:ro\nVolume=config/:/config\nVolume=named:/data\nVolume=/abs/path:/abs\nVolume=%h/x:/x\n";
    let parsed = parse_unit("caddy.container", unit).unwrap();
    assert_eq!(parsed.mounts, vec!["Caddyfile", "config"]);
  }

  #[test]
  fn bind_mounts_resolve_against_the_unit_directory() {
    let unit = "[Container]\nImage=x\nVolume=./conf:/c\nVolume=../shared/a.yml:/a\nVolume=../../out:/o\n";
    let parsed = parse_unit("web/web.container", unit).unwrap();
    assert_eq!(parsed.mounts, vec!["web/conf", "shared/a.yml"]);
  }

  #[test]
  fn build_context_counts_as_mounts() {
    let inputs = |path: &str, body: &str| {
      parse_unit(
        path,
        &format!("[Build]\nImageTag=localhost/a\n{body}"),
      )
      .unwrap()
      .mounts
    };
    assert_eq!(
      inputs(
        "app.build",
        "SetWorkingDirectory=unit\nFile=Dockerfile\n"
      ),
      vec![""]
    );
    assert_eq!(
      inputs("sub/app.build", "SetWorkingDirectory=unit\n"),
      vec!["sub"]
    );
    assert_eq!(
      inputs(
        "app.build",
        "File=docker/Containerfile\nSetWorkingDirectory=file\n"
      ),
      vec!["docker"]
    );
    assert_eq!(
      inputs(
        "app.build",
        "SetWorkingDirectory=src\nFile=Dockerfile\n"
      ),
      vec!["src", "Dockerfile"]
    );
    assert_eq!(
      inputs(
        "app.build",
        "SetWorkingDirectory=https://git.example/x.git\nFile=/abs/Dockerfile\n"
      ),
      Vec::<String>::new()
    );
    assert_eq!(
      inputs(
        "app.build",
        "SetWorkingDirectory=unit\nIgnoreFile=../outside\n"
      ),
      vec![""]
    );
    assert_eq!(
      inputs(
        "sub/app.build",
        "SetWorkingDirectory=ctx\nIgnoreFile=.ign\n"
      ),
      vec!["sub/ctx", "sub/.ign"]
    );
    assert!(covers("", "anything/at/all"));
    assert!(covers("src", "src/main.go"));
    assert!(!covers("src", "srcs/main.go"));
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
  fn build_reads_respects_the_ignore_file() {
    let files = |list: &[(&str, &str)]| {
      list
        .iter()
        .map(|(p, c)| (p.to_string(), c.to_string()))
        .collect::<BTreeMap<_, _>>()
    };
    let unit = parse_unit(
      "app.build",
      "[Build]\nSetWorkingDirectory=app\nFile=app/Containerfile\n",
    )
    .unwrap();
    let docker = files(&[(
      "app/.dockerignore",
      "*.md\nContainerfile\n.dockerignore\n",
    )]);
    assert!(unit.reads("app/main.go", &docker));
    assert!(!unit.reads("app/README.md", &docker));
    // podman reads these even when a pattern excludes them.
    assert!(unit.reads("app/Containerfile", &docker));
    assert!(unit.reads("app/.dockerignore", &docker));
    assert!(!unit.reads("other/main.go", &docker));

    // .containerignore wins over .dockerignore.
    let both = files(&[
      ("app/.dockerignore", "*.md\n"),
      ("app/.containerignore", "*.go\n"),
    ]);
    assert!(unit.reads("app/README.md", &both));
    assert!(!unit.reads("app/main.go", &both));
    // Neither file: everything in the context counts.
    assert!(unit.reads("app/README.md", &files(&[])));

    // IgnoreFile= overrides both, and may sit outside the context.
    let custom = parse_unit(
      "app.build",
      "[Build]\nSetWorkingDirectory=app\nIgnoreFile=build.ignore\n",
    )
    .unwrap();
    let with_custom = files(&[
      ("build.ignore", "tests\n"),
      ("app/.containerignore", "*\n"),
    ]);
    assert!(custom.reads("app/src/lib.rs", &with_custom));
    assert!(!custom.reads("app/tests/a.rs", &with_custom));
    assert!(custom.reads("build.ignore", &with_custom));
    assert!(custom.reads("app/Containerfile", &with_custom));
  }

  #[test]
  fn name_collisions_across_stacks() {
    let units = [
      parse_unit("web.container", "[Container]\nImage=x\n").unwrap(),
      parse_unit("net/app.network", "[Network]\n").unwrap(),
      parse_unit("db.container", "[Container]\nImage=x\n").unwrap(),
    ];
    let installed = [
      ("stack 'other'", "web.container"),
      ("stack 'other'", "deep/app.network"),
      ("stack 'kube'", "db.kube"),
      ("stack 'other'", "README.md"),
      ("stack 'other'", "db.pod"),
    ];
    assert_eq!(
      name_collisions(&units, installed),
      vec![
        "web.container: web.service is already web.container's in stack 'other'",
        "net/app.network: app-network.service is already app.network's in stack 'other'",
        "db.container: db.service is already db.kube's in stack 'kube'",
      ]
    );
    assert!(name_collisions(&units, [("x", "web.pod")]).is_empty());
  }
}
