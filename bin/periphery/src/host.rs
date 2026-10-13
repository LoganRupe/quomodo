//! Who the agent runs as. As root it deploys rootful quadlets; as any other
//! user it deploys that user's rootless quadlets with `systemctl --user`.
//! Probed once at startup.

use std::{ffi::CStr, path::PathBuf, sync::OnceLock};

use anyhow::{Context, anyhow};
use komodo_client::entities::update::Log;
use quadlet::prepare::Prepared;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scope {
  Root,
  User { name: String, uid: u32 },
}

impl Scope {
  pub fn is_rootless(&self) -> bool {
    matches!(self, Scope::User { .. })
  }

  pub fn user(&self) -> &str {
    match self {
      Scope::Root => "root",
      Scope::User { name, .. } => name,
    }
  }
}

pub fn scope() -> &'static Scope {
  static SCOPE: OnceLock<Scope> = OnceLock::new();
  SCOPE.get_or_init(|| {
    // SAFETY: geteuid cannot fail and touches no memory.
    let uid = unsafe { libc::geteuid() };
    if uid == 0 {
      return Scope::Root;
    }
    let name = user_name(uid).unwrap_or_else(|| uid.to_string());
    Scope::User { name, uid }
  })
}

/// The passwd name of `uid`, through NSS so directory users resolve too.
fn user_name(uid: u32) -> Option<String> {
  let mut buf = vec![0 as libc::c_char; 16384];
  // SAFETY: an all-zero passwd is a valid out parameter.
  let mut pwd: libc::passwd = unsafe { std::mem::zeroed() };
  let mut result = std::ptr::null_mut();
  // SAFETY: every pointer is valid for the call and buf outlives pwd's
  // borrowed strings, which are copied before buf drops.
  let rc = unsafe {
    libc::getpwuid_r(
      uid,
      &mut pwd,
      buf.as_mut_ptr(),
      buf.len(),
      &mut result,
    )
  };
  if rc != 0 || result.is_null() || pwd.pw_name.is_null() {
    return None;
  }
  // SAFETY: getpwuid_r succeeded, pw_name is a NUL-terminated string in buf.
  let name = unsafe { CStr::from_ptr(pwd.pw_name) };
  Some(name.to_string_lossy().into_owned())
}

fn linger_file(user: &str) -> PathBuf {
  PathBuf::from("/var/lib/systemd/linger").join(user)
}

/// Rootless only: linger, without which the user's systemd and every
/// stack on it stop when the user's last session ends.
pub fn check_linger() -> anyhow::Result<()> {
  let Scope::User { name, .. } = scope() else {
    return Ok(());
  };
  if linger_file(name).exists() {
    return Ok(());
  }
  Err(anyhow!(
    "lingering is off for '{name}', so its stacks stop when its last session ends. Run 'loginctl enable-linger {name}' as root"
  ))
}

/// Checks a deploy must pass before anything is written to the host or a
/// user namespace slice is taken. A no-op as root.
pub fn preflight(prepared: &Prepared) -> anyhow::Result<()> {
  if !scope().is_rootless() {
    return Ok(());
  }
  check_linger()?;
  let floor = std::fs::read_to_string(
    "/proc/sys/net/ipv4/ip_unprivileged_port_start",
  )
  .context("Failed to read net.ipv4.ip_unprivileged_port_start")?
  .trim()
  .parse::<u16>()
  .context("Failed to parse net.ipv4.ip_unprivileged_port_start")?;
  let low = quadlet::ports::low_host_ports(&prepared.write, floor);
  if low.is_empty() {
    return Ok(());
  }
  Err(anyhow!(
    "a rootless agent cannot publish host ports below {floor}:\n{}\nUse a higher host port, or as root set 'net.ipv4.ip_unprivileged_port_start' low enough in /etc/sysctl.d",
    low.join("\n")
  ))
}

/// Rootless only: point `[Install]` at the user manager's `default.target`
/// instead of system targets it does not have, so units start at boot.
/// Run before `idmap::assign`, which re-parses the units.
pub fn user_targets(prepared: &mut Prepared) -> Option<Log> {
  if !scope().is_rootless() {
    return None;
  }
  let changed =
    quadlet::install::to_user_targets(&mut prepared.write);
  (!changed.is_empty())
    .then(|| Log::simple("User Targets", changed.join("\n")))
}
