use std::{
  net::IpAddr, path::PathBuf, str::FromStr as _, sync::OnceLock,
  time::Duration,
};

use anyhow::Context;
use command::{
  CommandOptions, KomodoCommandMode,
  run_komodo_command_with_sanitization,
};
use environment::write_env_file;
use komodo_client::entities::{
  EnvironmentVar, RepoExecutionArgs, RepoExecutionResponse,
  SearchCombinator, SystemCommand, all_logs_success,
};
use periphery_client::api::git::PeripheryRepoExecutionResponse;
use shell_escape::unix::escape;

use crate::config::periphery_config;

// ============
//  Formatting
// ============

pub fn format_log_grep(
  terms: &[String],
  combinator: SearchCombinator,
  invert: bool,
) -> String {
  let maybe_invert = if invert { " -v" } else { Default::default() };
  match combinator {
    SearchCombinator::Or => {
      format!(
        "grep{maybe_invert} -E {}",
        escape(terms.join("|").into())
      )
    }
    SearchCombinator::And => {
      format!(
        "grep{maybe_invert} -P {}",
        escape(format!("^(?=.*{})", terms.join(")(?=.*")).into())
      )
    }
  }
}

// =====
//  Git
// =====

/// Writes the repo environment and runs the on_clone / on_pull commands
/// as Core sent them.
#[instrument(
  "PostRepoExecution",
  skip_all,
  fields(
    path = res.path.display().to_string(),
    env_file_path
  )
)]
pub async fn handle_post_repo_execution(
  mut res: RepoExecutionResponse,
  environment: Vec<EnvironmentVar>,
  env_file_path: &str,
  on_clone: Option<SystemCommand>,
  on_pull: Option<SystemCommand>,
  replacers: Vec<(String, String)>,
) -> anyhow::Result<PeripheryRepoExecutionResponse> {
  let env_file_path = write_env_file(
    &environment,
    &res.path,
    env_file_path,
    &mut res.logs,
  )
  .await;

  let mut res = PeripheryRepoExecutionResponse { res, env_file_path };

  if let Some(on_clone) = on_clone
    && !on_clone.is_none()
  {
    let path = res
      .res
      .path
      .join(on_clone.path)
      .components()
      .collect::<PathBuf>();
    if let Some(log) = run_komodo_command_with_sanitization(
      "On Clone",
      on_clone.command,
      CommandOptions::default().path(path.as_path()),
      if on_clone.shell_mode {
        KomodoCommandMode::Shell
      } else {
        KomodoCommandMode::Multiline
      },
      &replacers,
    )
    .await
    {
      res.res.logs.push(log);
      if !all_logs_success(&res.res.logs) {
        return Ok(res);
      }
    }
  }

  if let Some(on_pull) = on_pull
    && !on_pull.is_none()
  {
    let path = res
      .res
      .path
      .join(on_pull.path)
      .components()
      .collect::<PathBuf>();
    if let Some(log) = run_komodo_command_with_sanitization(
      "On Pull",
      on_pull.command,
      CommandOptions::default().path(path.as_path()),
      if on_pull.shell_mode {
        KomodoCommandMode::Shell
      } else {
        KomodoCommandMode::Multiline
      },
      &replacers,
    )
    .await
    {
      res.res.logs.push(log);
    }
  }

  Ok(res)
}

// =======
//  Token
// =======

pub fn git_token_simple(
  domain: &str,
  account_username: &str,
) -> anyhow::Result<&'static str> {
  periphery_config()
    .git_providers
    .iter()
    .find(|provider| provider.domain == domain)
    .and_then(|provider| {
      provider.accounts.iter().find(|account| account.username == account_username).map(|account| account.token.as_str())
    })
    .with_context(|| format!("Did not find token in config for git account {account_username} | domain {domain}"))
}

pub fn git_token(
  core_token: Option<String>,
  args: &RepoExecutionArgs,
) -> anyhow::Result<Option<String>> {
  if core_token.is_some() {
    return Ok(core_token);
  }
  let Some(account) = &args.account else {
    return Ok(None);
  };
  let token = git_token_simple(&args.provider, account)?;
  Ok(Some(token.to_string()))
}

pub fn registry_token(
  domain: &str,
  account_username: &str,
) -> anyhow::Result<&'static str> {
  periphery_config()
    .image_registries
    .iter()
    .find(|registry| registry.domain == domain)
    .and_then(|registry| {
      registry.accounts.iter().find(|account| account.username == account_username).map(|account| account.token.as_str())
    })
    .with_context(|| format!("did not find token in config for image registry account {account_username} | domain {domain}"))
}

// ====================
//  Public IP over DNS
// ====================

type OpenDNSResolver = hickory_resolver::TokioResolver;

fn opendns_resolver() -> &'static OpenDNSResolver {
  static OPENDNS_RESOLVER: OnceLock<OpenDNSResolver> =
    OnceLock::new();
  OPENDNS_RESOLVER.get_or_init(|| {
    // OpenDNS resolver ipv4s.
    let name_servers = [
      IpAddr::from_str("208.67.220.220").unwrap(),
      IpAddr::from_str("208.67.222.222").unwrap(),
    ]
    .into_iter()
    .map(hickory_resolver::config::NameServerConfig::udp_and_tcp)
    .collect();

    hickory_resolver::Resolver::builder_with_config(
      hickory_resolver::config::ResolverConfig::from_parts(
        None,
        vec![],
        name_servers,
      ),
      hickory_resolver::net::runtime::TokioRuntimeProvider::default(),
    )
    .build()
    .expect("Failed to build OpenDNS resolver")
  })
}

/// Includes 1s timeout
pub async fn resolve_host_public_ip() -> anyhow::Result<String> {
  tokio::time::timeout(Duration::from_secs(1), async {
    opendns_resolver()
      .lookup_ip("myip.opendns.com.")
      .await
      .context(
        "Failed to query OpenDNS resolvers for host public IP",
      )?
      .iter()
      .map(|ip| ip.to_string())
      .next()
      .context("OpenDNS call for public IP didn't return anything")
  })
  .await
  .context("OpenDNS call for public IP timed out")
  .flatten()
}
