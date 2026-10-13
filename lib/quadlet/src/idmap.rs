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
//!
//! A rootless agent maps from its own user's subordinate ranges instead.
//! Rootless podman reads the host side of `UIDMap=` as an id in the user's
//! namespace, where 0 is the user and 1 to N are the user's subordinate ids
//! in `/etc/subuid` order. Slices there count up from 1, so a range widened
//! with `usermod --add-subuids` adds slices at the end and moves none.
//!
//! A carve-out may not map onto ids slices are taken from: it would share
//! them with another unit. Rootless, that leaves only the user itself.

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

/// How many ids a rootless user's namespace has past the user itself: the
/// sum of every entry whose first field is one of `owners` (the user name
/// and uid), as podman lays them out.
pub fn subid_count(contents: &str, owners: &[&str]) -> u64 {
  contents
    .lines()
    .filter_map(|line| {
      let mut parts = line.trim().split(':');
      let name = parts.next()?;
      if !owners.contains(&name) {
        return None;
      }
      let _start = parts.next()?.trim().parse::<u32>().ok()?;
      parts.next()?.trim().parse::<u64>().ok()
    })
    .sum()
}

/// Where the slices of one id kind (uid or gid) come from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pool {
  /// Who owns the range, for errors.
  owner: String,
  /// First host id of the lowest slice.
  base: u64,
  slots: u32,
  /// Slice 0 is the highest (rootful) or the lowest (rootless).
  top_down: bool,
  /// Host ids slices are taken from, end exclusive.
  span: (u64, u64),
  /// Host ids the namespace can map at all, end exclusive. None when any
  /// host id can be mapped (rootful).
  valid: Option<(u64, u64)>,
}

impl Pool {
  /// The `containers` range of a rootful host. The top slice is left out:
  /// it ends at `u32::MAX`, which the kernel treats as "no id".
  pub fn rootful(range: SubIdRange) -> Pool {
    Pool {
      owner: SUBID_USER.to_string(),
      base: range.start as u64,
      slots: (range.count / SLOT).saturating_sub(1),
      top_down: true,
      span: (
        range.start as u64,
        range.start as u64 + range.count as u64,
      ),
      valid: None,
    }
  }

  /// A rootless user's namespace, `count` ids past the user itself (see
  /// [subid_count]).
  pub fn rootless(user: &str, count: u64) -> Pool {
    // The namespace is u32 wide, the user takes id 0.
    let count = count.min(u32::MAX as u64);
    Pool {
      owner: user.to_string(),
      base: 1,
      slots: (count / SLOT as u64) as u32,
      top_down: false,
      span: (1, 1 + count),
      valid: Some((0, 1 + count)),
    }
  }

  pub fn slots(&self) -> u32 {
    self.slots
  }

  /// First host id of slice `index`. `needed` is how many slices the host
  /// holds with this one, for the error when the range is too small.
  pub fn host_start(
    &self,
    index: u32,
    needed: u32,
  ) -> anyhow::Result<u32> {
    if index >= self.slots {
      let hint = if self.top_down {
        String::new()
      } else {
        // Rootless: a range added after the current ones moves no slice.
        format!(
          ". As root, add a free range to '{0}' with 'usermod --add-subuids <start>-<end> --add-subgids <start>-<end> {0}'; 4194304 ids hold 64 slices",
          self.owner
        )
      };
      return Err(anyhow!(
        "the subordinate id range of '{}' holds {} slice(s) of {SLOT} ids, this host needs {}{hint}",
        self.owner,
        self.slots,
        needed.max(index + 1),
      ));
    }
    let step = if self.top_down {
      self.slots - 1 - index
    } else {
      index
    };
    let start = self.base + step as u64 * SLOT as u64;
    u32::try_from(start).map_err(|_| {
      anyhow!(
        "the subordinate id range of '{}' runs past u32",
        self.owner
      )
    })
  }

  /// Fail if a carve-out maps onto ids slices are taken from, or onto ids
  /// the namespace cannot map.
  fn check_carve_out(&self, m: &Mapping) -> anyhow::Result<()> {
    let (from, to) = (m.host as u64, m.host as u64 + m.size as u64);
    if from < self.span.1 && self.span.0 < to {
      return Err(anyhow!(
        "{} maps onto host ids {}-{}, which slices are taken from; it would share them with another unit",
        m.render(),
        self.span.0,
        self.span.1 - 1,
      ));
    }
    if let Some((lo, hi)) = self.valid
      && (from < lo || to > hi)
    {
      return Err(anyhow!(
        "{} maps onto host ids outside the namespace of '{}'",
        m.render(),
        self.owner
      ));
    }
    Ok(())
  }
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
  uid_pool: &Pool,
  gid_pool: &Pool,
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
        for (pool, carve) in [(uid_pool, &uids), (gid_pool, &gids)] {
          for m in carve {
            pool
              .check_carve_out(m)
              .map_err(|e| anyhow!("{path}: UserNS=auto: {e}"))?;
          }
        }
        let index = slot_for(table, &format!("{stack}/{path}"));
        let needed = table.values().max().map_or(0, |max| max + 1);
        let uids =
          build_map(size, uid_pool.host_start(index, needed)?, &uids);
        let gids =
          build_map(size, gid_pool.host_start(index, needed)?, &gids);
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

  fn pool() -> Pool {
    Pool::rootful(RANGE)
  }

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
    let pool = pool();
    assert_eq!(pool.slots(), 32767);
    assert_eq!(pool.host_start(0, 1).unwrap(), 4294836223);
    assert_eq!(pool.host_start(1, 2).unwrap(), 4294836223 - SLOT);
    assert_eq!(pool.host_start(32766, 32767).unwrap(), RANGE.start);
    assert!(pool.host_start(32767, 32768).is_err());
    assert!(
      Pool::rootful(SubIdRange {
        start: 100000,
        count: 65536
      })
      .host_start(0, 1)
      .is_err()
    );
  }

  #[test]
  fn rootless_pools_count_up_from_one() {
    // Entries by name and by uid add up, in file order.
    let file =
      "alice:100000:65536\nbob:165536:65536\n1000:231072:131072\n";
    assert_eq!(subid_count(file, &["alice", "1000"]), 196608);
    assert_eq!(subid_count(file, &["carol"]), 0);

    let pool = Pool::rootless("alice", 196608);
    assert_eq!(pool.slots(), 3);
    assert_eq!(pool.host_start(0, 1).unwrap(), 1);
    assert_eq!(pool.host_start(2, 3).unwrap(), 1 + 2 * SLOT);
    let err = pool.host_start(3, 5).unwrap_err().to_string();
    assert!(err.contains("'alice' holds 3 slice(s)"), "{err}");
    assert!(err.contains("needs 5"), "{err}");

    // Widening the range keeps every slice where it was.
    let wider = Pool::rootless("alice", 4194304);
    assert_eq!(wider.slots(), 64);
    assert_eq!(wider.host_start(2, 3).unwrap(), 1 + 2 * SLOT);

    // A default useradd range is one slice; none at all is none.
    assert_eq!(Pool::rootless("alice", 65536).slots(), 1);
    assert!(Pool::rootless("alice", 0).host_start(0, 1).is_err());
  }

  #[test]
  fn carve_outs_stay_out_of_the_slices() {
    let rootful = pool();
    assert!(rootful.check_carve_out(&m(1000, 1000, 1)).is_ok());
    assert!(
      rootful
        .check_carve_out(&m(1000, RANGE.start + 5, 1))
        .is_err()
    );
    let rootless = Pool::rootless("alice", 131072);
    assert!(rootless.check_carve_out(&m(1000, 0, 1)).is_ok());
    assert!(rootless.check_carve_out(&m(1000, 1000, 1)).is_err());
    assert!(rootless.check_carve_out(&m(1000, 0, 2)).is_err());
    assert!(rootless.check_carve_out(&m(1000, 131073, 1)).is_err());

    let mut write = BTreeMap::from([(
      "x.container".to_string(),
      "[Container]\nUserNS=auto:uidmapping=1000:1000:1\n".to_string(),
    )]);
    let mut table = BTreeMap::new();
    let err =
      apply("s", &mut write, &mut table, &rootless, &rootless)
        .unwrap_err()
        .to_string();
    assert!(
      err.starts_with("x.container: UserNS=auto: 1000:1000:1"),
      "{err}"
    );
    assert!(table.is_empty());

    let mut write = BTreeMap::from([(
      "x.container".to_string(),
      "[Container]\nUserNS=auto:uidmapping=1000:0:1,gidmapping=1000:0:1\n"
        .to_string(),
    )]);
    apply("s", &mut write, &mut table, &rootless, &rootless).unwrap();
    assert_eq!(
      write["x.container"],
      "[Container]\nUIDMap=0:1:1000\nUIDMap=1000:0:1\nUIDMap=1001:1001:64535\nGIDMap=0:1:1000\nGIDMap=1000:0:1\nGIDMap=1001:1001:64535\n"
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
      apply("my stack", &mut write, &mut table, &pool(), &pool())
        .unwrap();

    let s0 = pool().host_start(0, 1).unwrap();
    let s1 = pool().host_start(1, 2).unwrap();
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
      apply("my stack", &mut write, &mut table, &pool(), &pool())
        .unwrap();
    assert_eq!(write, before);
    assert!(again.is_empty());
    assert_eq!(table.len(), 2);

    // Another stack gets the next slice; a known unit keeps its own.
    let mut other = BTreeMap::from([(
      "solo.container".to_string(),
      "[Container]\nImage=y\n".to_string(),
    )]);
    apply("other", &mut other, &mut table, &pool(), &pool()).unwrap();
    assert_eq!(table["other/solo.container"], 2);
    let mut back = BTreeMap::from([(
      "solo.container".to_string(),
      "[Container]\nImage=z\n".to_string(),
    )]);
    apply("my stack", &mut back, &mut table, &pool(), &pool())
      .unwrap();
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
