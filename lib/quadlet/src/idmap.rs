//! Fixed user namespaces for stack units.
//!
//! Podman's `UserNS=auto` takes the lowest free slice of the `containers`
//! subordinate range each time a container is created, and quadlet creates
//! the container on every unit start. A reboot or a redeploy can therefore
//! move a container's root to a different host uid: files it wrote into a
//! volume read as `nobody` afterwards, and podman before 5.6 keeps a
//! re-owned copy of every image layer per range it has used when the map
//! has a `uidmapping=` carve-out (containers/storage #2346).
//!
//! The agent hands out the ranges instead. Every `.pod`, and every
//! `.container` that is not a pod member, gets a 65536-id slice of the
//! `containers` range. The slice is recorded per host and never changes.
//!
//! - No `UserNS=`: a slice, written as `UIDMap=` / `GIDMap=` lines.
//! - `UserNS=auto[:size=N,uidmapping=C:H:S,gidmapping=C:H:S]`: the same,
//!   with the carve-outs kept and the map `N` ids wide (at most 65536).
//! - A `Pod=` member without `UserNS=`: `UserNS=` (empty), the pod's
//!   namespace. `UserNS=auto` on a member is an error: podman would run it
//!   in the host namespace.
//! - Any other `UserNS=` (host, keep-id, nomap, ns:, container:, empty), or
//!   an explicit `UIDMap=` / `GIDMap=` / `SubUIDMap=` / `SubGIDMap=`: kept.
//!
//! Slices are taken from the top of the range down. `auto` fills from the
//! bottom, so a container still running on an `auto` range from before the
//! agent allocated for it cannot overlap a slice.

use std::collections::BTreeMap;

use anyhow::anyhow;
use komodo_client::entities::stack::QuadletKind;

use crate::unit::{base_name, get_value, section_name};

/// Ids per slice. A full range: `nobody` (65534) maps, and it is what
/// `useradd` reserves per user in `/etc/subuid`.
pub const SLOT: u32 = 65536;

/// The subordinate id user podman's `auto` draws from, and the agent too.
pub const SUBID_USER: &str = "containers";

/// A `/etc/subuid` or `/etc/subgid` entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SubIdRange {
  pub start: u32,
  pub count: u32,
}

impl SubIdRange {
  /// Slices this range holds. The top one is left out: it ends at
  /// `u32::MAX`, which the kernel treats as "no id".
  fn slots(&self) -> u32 {
    (self.count / SLOT).saturating_sub(1)
  }

  /// First host id of slice `index`, counting down from the top.
  pub fn host_start(&self, index: u32) -> anyhow::Result<u32> {
    let slots = self.slots();
    if index >= slots {
      return Err(anyhow!(
        "subordinate id range {}:{} is used up: slice {index} of {slots}",
        self.start,
        self.count
      ));
    }
    let start =
      self.start as u64 + (slots - 1 - index) as u64 * SLOT as u64;
    u32::try_from(start).map_err(|_| {
      anyhow!(
        "subordinate id range {}:{} runs past u32",
        self.start,
        self.count
      )
    })
  }
}

/// The entry for `user` in a subuid / subgid file. The first one wins, as
/// in shadow-utils; a numeric first field is a uid, not a name, and is
/// skipped.
pub fn parse_subid(contents: &str, user: &str) -> Option<SubIdRange> {
  contents.lines().find_map(|line| {
    let mut parts = line.trim().split(':');
    let name = parts.next()?;
    if name != user {
      return None;
    }
    let start = parts.next()?.trim().parse().ok()?;
    let count = parts.next()?.trim().parse().ok()?;
    Some(SubIdRange { start, count })
  })
}

/// Slice index for `key`, allocating the lowest unused one on first sight.
pub fn slot_for(table: &mut BTreeMap<String, u32>, key: &str) -> u32 {
  if let Some(index) = table.get(key) {
    return *index;
  }
  let mut index = 0;
  while table.values().any(|v| *v == index) {
    index += 1;
  }
  table.insert(key.to_string(), index);
  index
}

/// One `container:host:size` triple.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Mapping {
  pub container: u32,
  pub host: u32,
  pub size: u32,
}

impl Mapping {
  fn parse(value: &str, what: &str) -> anyhow::Result<Mapping> {
    let parts = value.split(':').collect::<Vec<_>>();
    let [container, host, size] = parts[..] else {
      return Err(anyhow!(
        "{what}={value}: expected container:host:size"
      ));
    };
    let num = |s: &str| {
      s.trim()
        .parse::<u32>()
        .map_err(|_| anyhow!("{what}={value}: '{s}' is not a number"))
    };
    let mapping = Mapping {
      container: num(container)?,
      host: num(host)?,
      size: num(size)?,
    };
    if mapping.size == 0 {
      return Err(anyhow!("{what}={value}: size is 0"));
    }
    Ok(mapping)
  }

  fn render(&self) -> String {
    format!("{}:{}:{}", self.container, self.host, self.size)
  }
}

/// What a unit asks for, read from its `UserNS=` line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
  /// Leave the unit as it is.
  Keep,
  /// A pod member without `UserNS=`: write `UserNS=`.
  Inherit,
  /// A slice from the range, with the given width and carve-outs.
  Slice {
    size: u32,
    uids: Vec<Mapping>,
    gids: Vec<Mapping>,
  },
}

/// Read a unit's request. `Ok(None)` for a unit kind that has no user
/// namespace of its own.
pub fn request(
  path: &str,
  contents: &str,
) -> anyhow::Result<Option<Request>> {
  let kind = QuadletKind::from_file_name(base_name(path));
  let section = match kind {
    Some(k @ (QuadletKind::Container | QuadletKind::Pod)) => {
      section_name(k)
    }
    _ => return Ok(None),
  };
  let explicit = ["UIDMap", "GIDMap", "SubUIDMap", "SubGIDMap"]
    .iter()
    .any(|key| get_value(contents, section, key).is_some());
  let member = kind == Some(QuadletKind::Container)
    && get_value(contents, section, "Pod").is_some();
  let userns = get_value(contents, section, "UserNS");
  let is_auto =
    userns.is_some_and(|v| v == "auto" || v.starts_with("auto:"));

  if member {
    return match userns {
      None if !explicit => Ok(Some(Request::Inherit)),
      Some(_) if is_auto => Err(anyhow!(
        "{path}: a pod member cannot use UserNS=auto (podman would run it in the host user namespace); drop the key to share the pod's namespace"
      )),
      _ => Ok(Some(Request::Keep)),
    };
  }
  if explicit || (userns.is_some() && !is_auto) {
    return Ok(Some(Request::Keep));
  }

  let mut size = SLOT;
  let mut uids = Vec::new();
  let mut gids = Vec::new();
  if let Some(opts) = userns.and_then(|v| v.strip_prefix("auto:")) {
    for opt in
      opts.split(',').map(str::trim).filter(|o| !o.is_empty())
    {
      let (key, value) = opt.split_once('=').ok_or_else(|| {
        anyhow!("{path}: UserNS=auto option '{opt}' has no value")
      })?;
      match key.trim() {
        "size" => {
          size = value.trim().parse().map_err(|_| {
            anyhow!(
              "{path}: UserNS=auto size '{value}' is not a number"
            )
          })?;
          if size == 0 || size > SLOT {
            return Err(anyhow!(
              "{path}: UserNS=auto size must be 1 to {SLOT}, got {size}"
            ));
          }
        }
        "uidmapping" => {
          uids.push(Mapping::parse(value, "uidmapping")?)
        }
        "gidmapping" => {
          gids.push(Mapping::parse(value, "gidmapping")?)
        }
        other => {
          return Err(anyhow!(
            "{path}: unknown UserNS=auto option '{other}'"
          ));
        }
      }
    }
  }
  for (what, carve) in [("uidmapping", &uids), ("gidmapping", &gids)]
  {
    check_carve_outs(carve, size)
      .map_err(|e| anyhow!("{path}: {what}: {e}"))?;
  }
  Ok(Some(Request::Slice { size, uids, gids }))
}

fn check_carve_outs(
  carve: &[Mapping],
  size: u32,
) -> anyhow::Result<()> {
  let mut sorted = carve.to_vec();
  sorted.sort_by_key(|m| m.container);
  let mut end = 0u64;
  for m in sorted {
    let last = m.container as u64 + m.size as u64;
    if last > size as u64 {
      return Err(anyhow!(
        "{} maps container ids past the namespace size {size}",
        m.render()
      ));
    }
    if (m.container as u64) < end {
      return Err(anyhow!("{} overlaps another mapping", m.render()));
    }
    end = last;
  }
  Ok(())
}

/// The full map for a slice starting at `host_start`: the carve-outs as
/// given, every other container id in order onto consecutive host ids.
pub fn build_map(
  size: u32,
  host_start: u32,
  carve: &[Mapping],
) -> Vec<Mapping> {
  let mut sorted = carve.to_vec();
  sorted.sort_by_key(|m| m.container);
  let mut out = Vec::new();
  let mut next_container = 0u32;
  let mut next_host = host_start;
  // Each carve-out ends a run of plain ids; `size` ends the last run.
  let ends = sorted
    .iter()
    .map(|m| (m.container, Some(*m)))
    .chain(std::iter::once((size, None)));
  for (upto, carve) in ends {
    if upto > next_container {
      out.push(Mapping {
        container: next_container,
        host: next_host,
        size: upto - next_container,
      });
      next_host += upto - next_container;
      next_container = upto;
    }
    if let Some(m) = carve {
      out.push(m);
      next_container = m.container + m.size;
    }
  }
  out
}

/// Where a unit's slice went, for the deploy log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Assigned {
  pub file: String,
  pub uids: Vec<Mapping>,
  pub gids: Vec<Mapping>,
}

/// Rewrite the unit files in `write` for `stack`. `table` is the host's
/// slice table (`<stack>/<unit file>` -> slice index); new units are added
/// to it. Returns the slices written, in file order.
pub fn apply(
  stack: &str,
  write: &mut BTreeMap<String, String>,
  table: &mut BTreeMap<String, u32>,
  subuid: &SubIdRange,
  subgid: &SubIdRange,
) -> anyhow::Result<Vec<Assigned>> {
  let mut assigned = Vec::new();
  for (path, contents) in write.iter_mut() {
    let Some(request) = request(path, contents)? else {
      continue;
    };
    let section = match QuadletKind::from_file_name(base_name(path)) {
      Some(QuadletKind::Pod) => "Pod",
      _ => "Container",
    };
    match request {
      Request::Keep => {}
      Request::Inherit => {
        *contents =
          replace_userns(contents, section, &["UserNS=".to_string()]);
      }
      Request::Slice { size, uids, gids } => {
        let index = slot_for(table, &format!("{stack}/{path}"));
        let uids = build_map(size, subuid.host_start(index)?, &uids);
        let gids = build_map(size, subgid.host_start(index)?, &gids);
        let lines = uids
          .iter()
          .map(|m| format!("UIDMap={}", m.render()))
          .chain(
            gids.iter().map(|m| format!("GIDMap={}", m.render())),
          )
          .collect::<Vec<_>>();
        *contents = replace_userns(contents, section, &lines);
        assigned.push(Assigned {
          file: path.clone(),
          uids,
          gids,
        });
      }
    }
  }
  Ok(assigned)
}

/// Validate every unit's request without allocating. For Core, so a bad
/// `UserNS=auto` fails before anything reaches the host.
pub fn check(write: &BTreeMap<String, String>) -> Vec<String> {
  write
    .iter()
    .filter_map(|(path, contents)| {
      request(path, contents).err().map(|e| e.to_string())
    })
    .collect()
}

/// `contents` with the `UserNS=` line of `[section]` replaced by `lines`,
/// or with `lines` inserted after the section header when there is none.
/// A missing section is appended.
fn replace_userns(
  contents: &str,
  section: &str,
  lines: &[String],
) -> String {
  let header = format!("[{section}]");
  let mut out = String::with_capacity(contents.len() + 64);
  let mut in_section = false;
  let mut done = false;
  let mut header_at = None;
  for line in contents.split_inclusive('\n') {
    let trimmed = line.trim();
    if trimmed.starts_with('[') {
      in_section = trimmed == header;
      if in_section && header_at.is_none() {
        out.push_str(line);
        if !line.ends_with('\n') {
          out.push('\n');
        }
        header_at = Some(out.len());
        continue;
      }
    }
    let is_userns = in_section
      && !trimmed.starts_with('#')
      && !trimmed.starts_with(';')
      && trimmed
        .split_once('=')
        .is_some_and(|(k, _)| k.trim() == "UserNS");
    if is_userns && !done {
      let indent = &line[..line.len() - line.trim_start().len()];
      for l in lines {
        out.push_str(indent);
        out.push_str(l);
        out.push('\n');
      }
      done = true;
      continue;
    }
    if is_userns {
      // A repeated UserNS=: podman takes the last, we keep ours.
      continue;
    }
    out.push_str(line);
  }
  if done {
    return out;
  }
  let block =
    lines.iter().map(|l| format!("{l}\n")).collect::<String>();
  match header_at {
    Some(at) => {
      out.insert_str(at, &block);
      out
    }
    None => {
      if !out.ends_with('\n') && !out.is_empty() {
        out.push('\n');
      }
      out.push_str(&header);
      out.push('\n');
      out.push_str(&block);
      out
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  const RANGE: SubIdRange = SubIdRange {
    start: 2147483647,
    count: 2147483648,
  };

  fn m(container: u32, host: u32, size: u32) -> Mapping {
    Mapping {
      container,
      host,
      size,
    }
  }

  #[test]
  fn subid_parsing_and_slices() {
    let file =
      "alice:100000:65536\ncontainers:2147483647:2147483648\n";
    assert_eq!(parse_subid(file, "containers"), Some(RANGE));
    assert_eq!(parse_subid(file, "nobody"), None);
    assert_eq!(parse_subid("containers:bad:1\n", "containers"), None);

    // 32768 slices, the top one skipped: slice 0 ends one below it.
    assert_eq!(RANGE.slots(), 32767);
    assert_eq!(RANGE.host_start(0).unwrap(), 4294836223);
    assert_eq!(RANGE.host_start(1).unwrap(), 4294836223 - SLOT);
    assert_eq!(RANGE.host_start(32766).unwrap(), RANGE.start);
    assert!(RANGE.host_start(32767).is_err());
    assert!(
      SubIdRange {
        start: 100000,
        count: 65536
      }
      .host_start(0)
      .is_err()
    );
  }

  #[test]
  fn slots_are_sticky_and_fill_gaps() {
    let mut table = BTreeMap::new();
    assert_eq!(slot_for(&mut table, "a/x.container"), 0);
    assert_eq!(slot_for(&mut table, "a/y.pod"), 1);
    assert_eq!(slot_for(&mut table, "a/x.container"), 0);
    table.remove("a/x.container");
    assert_eq!(slot_for(&mut table, "b/z.container"), 0);
  }

  #[test]
  fn requests() {
    let req = |path: &str, body: &str| request(path, body).unwrap();
    assert_eq!(
      req("web.container", "[Container]\nImage=x\n"),
      Some(Request::Slice {
        size: SLOT,
        uids: vec![],
        gids: vec![]
      })
    );
    assert_eq!(
      req(
        "app.pod",
        "[Pod]\nUserNS=auto:size=4096,uidmapping=1000:1000:1,gidmapping=1000:1000:1\n"
      ),
      Some(Request::Slice {
        size: 4096,
        uids: vec![m(1000, 1000, 1)],
        gids: vec![m(1000, 1000, 1)],
      })
    );
    assert_eq!(
      req("web.container", "[Container]\nPod=app.pod\nImage=x\n"),
      Some(Request::Inherit)
    );
    for keep in [
      "[Container]\nPod=app.pod\nUserNS=\n",
      "[Container]\nUserNS=host\n",
      "[Container]\nUserNS=keep-id:uid=200\n",
      "[Container]\nUserNS=\n",
      "[Container]\nUIDMap=0:1:2\n",
      "[Container]\nSubUIDMap=utest\n",
    ] {
      assert_eq!(
        req("x.container", keep),
        Some(Request::Keep),
        "{keep}"
      );
    }
    assert_eq!(req("x.network", "[Network]\n"), None);
    assert_eq!(req("x.kube", "[Kube]\n"), None);

    for bad in [
      ("x.container", "[Container]\nPod=a.pod\nUserNS=auto\n"),
      ("x.container", "[Container]\nUserNS=auto:size=70000\n"),
      ("x.container", "[Container]\nUserNS=auto:size=0\n"),
      ("x.container", "[Container]\nUserNS=auto:nope=1\n"),
      ("x.container", "[Container]\nUserNS=auto:uidmapping=1:2\n"),
      (
        "x.container",
        "[Container]\nUserNS=auto:size=1000,uidmapping=1000:1000:1\n",
      ),
      (
        "x.container",
        "[Container]\nUserNS=auto:uidmapping=5:5:10,uidmapping=8:8:1\n",
      ),
    ] {
      assert!(request(bad.0, bad.1).is_err(), "{}", bad.1);
    }
  }

  #[test]
  fn map_shape_matches_podman_auto() {
    // podman 5.4 for auto:size=65536,uidmapping=1000:1000:1 starting at S
    // gave 1000:1000:1, 0:S:1000, 1001:S+1000:64535.
    let s = 2147484671;
    assert_eq!(
      build_map(SLOT, s, &[m(1000, 1000, 1)]),
      vec![m(0, s, 1000), m(1000, 1000, 1), m(1001, s + 1000, 64535)]
    );
    assert_eq!(build_map(SLOT, s, &[]), vec![m(0, s, SLOT)]);
    assert_eq!(
      build_map(10, s, &[m(0, 7, 2), m(9, 8, 1)]),
      vec![m(0, 7, 2), m(2, s, 7), m(9, 8, 1)]
    );
  }

  #[test]
  fn apply_rewrites_units_and_keeps_slices() {
    let mut write = BTreeMap::from([
      (
        "app.pod".to_string(),
        "[Pod]\nPodName=app\n# pinned\nUserNS=auto:uidmapping=1000:1000:1,gidmapping=1000:1000:1\nNetwork=app.network\n".to_string(),
      ),
      (
        "web.container".to_string(),
        "[Unit]\nDescription=web\n\n[Container]\nImage=x\nPod=app.pod\n".to_string(),
      ),
      (
        "solo.container".to_string(),
        "[Unit]\nDescription=solo\n".to_string(),
      ),
      (
        "agent.container".to_string(),
        "[Container]\nImage=x\nUserNS=host\n".to_string(),
      ),
      ("app.network".to_string(), "[Network]\n".to_string()),
    ]);
    let mut table = BTreeMap::new();
    let out =
      apply("my stack", &mut write, &mut table, &RANGE, &RANGE)
        .unwrap();

    let s0 = RANGE.host_start(0).unwrap();
    let s1 = RANGE.host_start(1).unwrap();
    assert_eq!(
      write["app.pod"],
      format!(
        "[Pod]\nPodName=app\n# pinned\nUIDMap=0:{s0}:1000\nUIDMap=1000:1000:1\nUIDMap=1001:{}:64535\nGIDMap=0:{s0}:1000\nGIDMap=1000:1000:1\nGIDMap=1001:{}:64535\nNetwork=app.network\n",
        s0 + 1000,
        s0 + 1000
      )
    );
    assert_eq!(
      write["web.container"],
      "[Unit]\nDescription=web\n\n[Container]\nUserNS=\nImage=x\nPod=app.pod\n"
    );
    assert_eq!(
      write["solo.container"],
      format!(
        "[Unit]\nDescription=solo\n[Container]\nUIDMap=0:{s1}:65536\nGIDMap=0:{s1}:65536\n"
      )
    );
    assert_eq!(
      write["agent.container"],
      "[Container]\nImage=x\nUserNS=host\n"
    );
    assert_eq!(write["app.network"], "[Network]\n");
    assert_eq!(
      out.iter().map(|a| a.file.as_str()).collect::<Vec<_>>(),
      vec!["app.pod", "solo.container"]
    );
    assert_eq!(table["my stack/app.pod"], 0);
    assert_eq!(table["my stack/solo.container"], 1);

    // Idempotent: the rewritten files are explicit maps now, so a second
    // pass keeps them, and the table does not grow.
    let before = write.clone();
    let again =
      apply("my stack", &mut write, &mut table, &RANGE, &RANGE)
        .unwrap();
    assert_eq!(write, before);
    assert!(again.is_empty());
    assert_eq!(table.len(), 2);

    // Another stack gets the next slice; a known unit keeps its own.
    let mut other = BTreeMap::from([(
      "solo.container".to_string(),
      "[Container]\nImage=y\n".to_string(),
    )]);
    apply("other", &mut other, &mut table, &RANGE, &RANGE).unwrap();
    assert_eq!(table["other/solo.container"], 2);
    let mut back = BTreeMap::from([(
      "solo.container".to_string(),
      "[Container]\nImage=z\n".to_string(),
    )]);
    apply("my stack", &mut back, &mut table, &RANGE, &RANGE).unwrap();
    assert!(
      back["solo.container"]
        .contains(&format!("UIDMap=0:{s1}:65536"))
    );
  }

  #[test]
  fn check_reports_without_allocating() {
    let write = BTreeMap::from([
      (
        "a.container".to_string(),
        "[Container]\nPod=p.pod\nUserNS=auto\n".to_string(),
      ),
      ("b.container".to_string(), "[Container]\n".to_string()),
    ]);
    let errors = check(&write);
    assert_eq!(errors.len(), 1);
    assert!(errors[0].starts_with("a.container: a pod member"));
  }

  #[test]
  fn replace_userns_handles_indent_repeats_and_other_sections() {
    let unit = "[Container]\n  UserNS=auto\nUserNS=auto:size=1\n[Service]\nUserNS=not-a-real-key\n";
    assert_eq!(
      replace_userns(
        unit,
        "Container",
        &["A=1".into(), "B=2".into()]
      ),
      "[Container]\n  A=1\n  B=2\n[Service]\nUserNS=not-a-real-key\n"
    );
    assert_eq!(
      replace_userns("[Container]", "Container", &["A=1".into()]),
      "[Container]\nA=1\n"
    );
  }
}
