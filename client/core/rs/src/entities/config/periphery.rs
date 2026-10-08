//! # Configuring the Quomodo Agent
//!
//! The agent configuration is passed in three ways:
//! 1. Command line args ([CliArgs])
//! 2. Environment Variables ([Env])
//! 3. Configuration File ([PeripheryConfig])
//!
//! The final configuration is built by combining parameters
//! passed through the different methods. The priority of the args is
//! strictly hierarchical, meaning params passed with [CliArgs] have top priority,
//! followed by those passed in the environment, followed by those passed in
//! the configuration file.
//!
//! The agent only dials out to Core. There is no inbound listener.

use clap::Parser;
use serde::Deserialize;
use std::path::PathBuf;

use crate::{
  deserializers::{
    ForgivingVec, option_string_list_deserializer,
    string_list_deserializer,
  },
  entities::{
    Timelength,
    logger::{LogConfig, LogLevel, StdioLogMode},
  },
};

use super::{
  GitProvider, ImageRegistry, ProviderAccount, empty_or_redacted,
};

/// # Agent Command Line Arguments.
///
/// Example command:
/// ```sh
/// quomodo-agent \
///   --config-path /etc/quomodo/config.toml \
///   --log-level info
/// ```
#[derive(Parser)]
#[command(name = "quomodo-agent", author, about, version)]
pub struct CliArgs {
  /// Additional utilities.
  #[command(subcommand)]
  pub command: Option<Command>,

  /// Sets the path of a config file or directory to use.
  /// Can use multiple times
  #[arg(long, short = 'c')]
  pub config_path: Option<Vec<PathBuf>>,

  /// Sets the keywords to match directory config file names on.
  /// Supports wildcard syntax.
  /// Can use multiple times to match multiple patterns independently.
  #[arg(long, short = 'm')]
  pub config_keyword: Option<Vec<String>>,

  /// Merges nested configs, eg. providers.
  /// Will override the equivalent env configuration.
  /// Default: true
  #[arg(long)]
  pub merge_nested_config: Option<bool>,

  /// Extends config arrays, eg. core_addresses.
  /// Will override the equivalent env configuration.
  /// Default: true
  #[arg(long)]
  pub extend_config_arrays: Option<bool>,

  /// Configure the logging level: error, warn, info, debug, trace.
  /// Default: info
  /// If passed, will override any other log_level set.
  #[arg(long)]
  pub log_level: Option<tracing::Level>,
}

#[cfg(feature = "cli")]
#[derive(Debug, Clone, clap::Subcommand)]
pub enum Command {
  /// Private-Public key utilities. (alias: `k`)
  #[clap(alias = "k")]
  Key {
    #[command(subcommand)]
    command: mogh_pki::cli::KeyCommand,
  },
  /// Print the agent's public key, generating the configured private key
  /// file first if it does not exist yet. Use this to register the Server on Core.
  #[clap(name = "public-key", alias = "pk")]
  PublicKey,
}

/// # Agent Environment Variables
///
/// The variables should be passed in the traditional `UPPER_SNAKE_CASE` format,
/// although the lower case format can still be parsed. If equivalent paramater is passed
/// in [CliArgs], the value passed to the environment will be ignored in favor of the cli arg.
#[derive(Deserialize)]
pub struct Env {
  /// Specify the config paths (files or folders) used to build up the
  /// final [PeripheryConfig].
  /// If not provided, will use Default config.
  ///
  /// Note. This is overridden if the equivalent arg is passed in [CliArgs].
  #[serde(
    default,
    alias = "periphery_config_path",
    alias = "quomodo_agent_config_path",
    alias = "quomodo_agent_config_paths"
  )]
  pub periphery_config_paths: Vec<PathBuf>,
  /// If specifying folders, use this to narrow down which
  /// files will be matched to parse into the final [PeripheryConfig].
  /// Only files inside the folders which have names containing a keywords
  /// provided to `config_keywords` will be included.
  /// Keywords support wildcard matching syntax.
  ///
  /// Note. This is overridden if the equivalent arg is passed in [CliArgs].
  #[serde(
    default = "super::default_config_keywords",
    alias = "periphery_config_keyword"
  )]
  pub periphery_config_keywords: Vec<String>,

  /// Will merge nested config object (eg. providers) across multiple
  /// config files. Default: `true`
  ///
  /// Note. This is overridden if the equivalent arg is passed in [CliArgs].
  #[serde(default = "super::default_merge_nested_config")]
  pub periphery_merge_nested_config: bool,

  /// Will extend config arrays (eg. `core_addresses`) across multiple config files.
  /// Default: `true`
  ///
  /// Note. This is overridden if the equivalent arg is passed in [CliArgs].
  #[serde(default = "super::default_extend_config_arrays")]
  pub periphery_extend_config_arrays: bool,

  /// Override `private_key`
  pub periphery_private_key: Option<String>,
  /// Override `private_key` from file
  pub periphery_private_key_file: Option<PathBuf>,
  /// Override `onboarding_key`
  pub periphery_onboarding_key: Option<String>,
  /// Override `onboarding_key` from file
  pub periphery_onboarding_key_file: Option<PathBuf>,
  /// Override `core_public_keys`
  #[serde(alias = "periphery_core_public_key")]
  pub periphery_core_public_keys: Option<Vec<String>>,
  /// Override `core_addresses`
  #[serde(alias = "periphery_core_address")]
  pub periphery_core_addresses: Option<Vec<String>>,
  /// Override `core_tls_ca_file`
  pub periphery_core_tls_ca_file: Option<PathBuf>,
  /// Override `connect_as`
  pub periphery_connect_as: Option<String>,
  /// Override `config_directory`
  pub periphery_config_directory: Option<PathBuf>,
  /// Override `root_directory`
  pub periphery_root_directory: Option<PathBuf>,
  /// Override `repo_dir`
  pub periphery_repo_dir: Option<PathBuf>,
  /// Override `quadlet_dir`
  pub periphery_quadlet_dir: Option<PathBuf>,
  /// Override `stats_polling_rate`
  pub periphery_stats_polling_rate: Option<Timelength>,

  /// Override `logging.level`
  pub periphery_logging_level: Option<LogLevel>,
  /// Override `logging.stdio`
  pub periphery_logging_stdio: Option<StdioLogMode>,
  /// Override `logging.pretty`
  pub periphery_logging_pretty: Option<bool>,
  /// Override `logging.location`
  pub periphery_logging_location: Option<bool>,
  /// Override `logging.ansi`
  pub periphery_logging_ansi: Option<bool>,
  /// Override `logging.timestamps`
  pub periphery_logging_timestamps: Option<bool>,
  /// Override `logging.otlp_endpoint`
  pub periphery_logging_otlp_endpoint: Option<String>,
  /// Override `logging.opentelemetry_service_name`
  pub periphery_logging_opentelemetry_service_name: Option<String>,
  /// Override `logging.opentelemetry_scope_name`
  pub periphery_logging_opentelemetry_scope_name: Option<String>,
  /// Override `pretty_startup_config`
  pub periphery_pretty_startup_config: Option<bool>,

  /// Override `include_disk_mounts`
  pub periphery_include_disk_mounts: Option<ForgivingVec<PathBuf>>,
  /// Override `exclude_disk_mounts`
  pub periphery_exclude_disk_mounts: Option<ForgivingVec<PathBuf>>,
}

/// # Agent Configuration File
///
/// Refer to the example file at `config/agent.config.toml` for a full example.
#[derive(Debug, Clone, Deserialize)]
pub struct PeripheryConfig {
  /// The private key used with noise handshake.
  ///
  /// Supports openssl generated pem file, `openssl genpkey -algorithm X25519 -out private.key`.
  /// To load from file, use `private_key = "file:/path/to/private.key"`
  ///
  /// If a file is specified and does not exist, will try to generate one at the path
  /// and use it going forward.
  ///
  /// Default: ${config_directory}/keys/agent.key
  #[serde(skip_serializing_if = "Option::is_none")]
  pub private_key: Option<String>,

  /// Provide an onboarding key to use with the new Server
  /// creation flow.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub onboarding_key: Option<String>,

  /// Optionally pin a specific Core public key for additional trust.
  ///
  /// Supports openssl generated pem file, `openssl pkey -in private.key -pubout -out public.key`.
  /// To load from file, include `file:/path/to/public.key` in the list.
  ///
  /// If not provided, defaults to ["file:${config_directory}/keys/core.pub"]
  #[serde(
    default,
    alias = "core_public_key",
    deserialize_with = "option_string_list_deserializer",
    skip_serializing_if = "Option::is_none"
  )]
  pub core_public_keys: Option<Vec<String>>,

  // =======================
  // = OUTBOUND CONNECTION =
  // =======================
  /// Address of Core to connect to. The agent only dials out.
  #[serde(
    default,
    alias = "core_address",
    deserialize_with = "string_list_deserializer"
  )]
  pub core_addresses: Vec<String>,

  /// A PEM file of CA certificates to verify Core's TLS certificate
  /// against, instead of the system roots. For a Core on a private CA.
  /// There is no way to skip verification.
  #[serde(default)]
  pub core_tls_ca_file: Option<PathBuf>,

  /// Server name / id to connect as
  #[serde(default)]
  pub connect_as: String,

  // ==================
  // = OTHER SETTINGS =
  // ==================
  /// The directory holding the agent config and keys.
  ///
  /// keys: ${config_directory}/keys
  ///
  /// Default: `/etc/quomodo`
  #[serde(default = "default_config_directory")]
  pub config_directory: PathBuf,

  /// The directory the agent uses as the default root for its working state.
  ///
  /// repo: ${root_directory}/repos
  /// stack state: ${root_directory}/stacks
  ///
  /// Default: `/var/lib/quomodo`
  #[serde(default = "default_root_directory")]
  pub root_directory: PathBuf,

  /// The system directory where managed repos will be cloned.
  /// If not provided, will default to `${root_directory}/repos`.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub repo_dir: Option<PathBuf>,

  /// The systemd quadlet directory stacks are installed into,
  /// one subdirectory per stack.
  /// Default: `/etc/containers/systemd`
  #[serde(default = "default_quadlet_dir")]
  pub quadlet_dir: PathBuf,

  /// The rate at which the system stats will be polled to update the cache.
  /// Options: https://docs.rs/komodo_client/latest/komodo_client/entities/enum.Timelength.html
  /// Default: `5-sec`
  #[serde(default = "default_stats_polling_rate")]
  pub stats_polling_rate: Timelength,

  /// Logging configuration
  #[serde(default)]
  pub logging: LogConfig,

  /// Pretty-log (multi-line) the startup config
  /// for easier human readability.
  #[serde(default)]
  pub pretty_startup_config: bool,

  /// If non-empty, only includes specific mount paths in the disk report.
  #[serde(default)]
  pub include_disk_mounts: ForgivingVec<PathBuf>,

  /// Exclude specific mount paths in the disk report.
  #[serde(default)]
  pub exclude_disk_mounts: ForgivingVec<PathBuf>,

  /// Configure git credentials used to clone private repos.
  /// Supports any git provider.
  #[serde(default, alias = "git_provider")]
  pub git_providers: ForgivingVec<GitProvider>,

  /// Configure image registry credentials used to pull images.
  #[serde(default, alias = "image_registry")]
  pub image_registries: ForgivingVec<ImageRegistry>,
}

fn default_config_directory() -> PathBuf {
  "/etc/quomodo".parse().unwrap()
}

fn default_root_directory() -> PathBuf {
  "/var/lib/quomodo".parse().unwrap()
}

fn default_quadlet_dir() -> PathBuf {
  "/etc/containers/systemd".parse().unwrap()
}

fn default_stats_polling_rate() -> Timelength {
  Timelength::FiveSeconds
}

impl Default for PeripheryConfig {
  fn default() -> Self {
    Self {
      private_key: None,
      onboarding_key: None,
      core_public_keys: None,
      core_addresses: Default::default(),
      core_tls_ca_file: None,
      connect_as: Default::default(),
      config_directory: default_config_directory(),
      root_directory: default_root_directory(),
      repo_dir: None,
      quadlet_dir: default_quadlet_dir(),
      stats_polling_rate: default_stats_polling_rate(),
      logging: Default::default(),
      pretty_startup_config: Default::default(),
      include_disk_mounts: Default::default(),
      exclude_disk_mounts: Default::default(),
      git_providers: Default::default(),
      image_registries: Default::default(),
    }
  }
}

impl PeripheryConfig {
  pub fn sanitized(&self) -> PeripheryConfig {
    PeripheryConfig {
      private_key: self.private_key.as_ref().map(|private_key| {
        if private_key.starts_with("file:") {
          private_key.clone()
        } else {
          empty_or_redacted(private_key)
        }
      }),
      onboarding_key: self
        .onboarding_key
        .as_ref()
        .map(|key| empty_or_redacted(key)),
      core_public_keys: self.core_public_keys.clone(),
      core_addresses: self.core_addresses.clone(),
      core_tls_ca_file: self.core_tls_ca_file.clone(),
      connect_as: self.connect_as.clone(),
      config_directory: self.config_directory.clone(),
      root_directory: self.root_directory.clone(),
      repo_dir: self.repo_dir.clone(),
      quadlet_dir: self.quadlet_dir.clone(),
      stats_polling_rate: self.stats_polling_rate,
      logging: self.logging.clone(),
      pretty_startup_config: self.pretty_startup_config,
      include_disk_mounts: self.include_disk_mounts.clone(),
      exclude_disk_mounts: self.exclude_disk_mounts.clone(),
      git_providers: self
        .git_providers
        .iter()
        .map(|provider| GitProvider {
          domain: provider.domain.clone(),
          https: provider.https,
          accounts: provider
            .accounts
            .iter()
            .map(|account| ProviderAccount {
              username: account.username.clone(),
              token: empty_or_redacted(&account.token),
            })
            .collect(),
        })
        .collect(),
      image_registries: self
        .image_registries
        .iter()
        .map(|provider| ImageRegistry {
          domain: provider.domain.clone(),
          organizations: provider.organizations.clone(),
          accounts: provider
            .accounts
            .iter()
            .map(|account| ProviderAccount {
              username: account.username.clone(),
              token: empty_or_redacted(&account.token),
            })
            .collect(),
        })
        .collect(),
    }
  }

  /// Explicitly set public key spec, or the default
  /// `$config_directory/keys/core.pub`.
  /// If it doesn't exist, will be auto written on first connection with Core.
  pub fn core_public_keys_spec(&self) -> Vec<String> {
    if let Some(public_keys) = self.core_public_keys.clone() {
      return public_keys;
    };
    vec![format!("file:{}", self.core_public_key_path().display())]
  }

  pub fn private_key_spec(&self) -> String {
    self.private_key.clone().unwrap_or_else(|| {
      format!(
        "file:{}",
        self.config_directory.join("keys/agent.key").display()
      )
    })
  }

  pub fn core_public_key_path(&self) -> PathBuf {
    self.config_directory.join("keys/core.pub")
  }

  pub fn repo_dir(&self) -> PathBuf {
    if let Some(dir) = &self.repo_dir {
      dir.to_owned()
    } else {
      self.root_directory.join("repos")
    }
  }

  /// Per-stack working state (last deployed hashes).
  pub fn stack_state_dir(&self) -> PathBuf {
    self.root_directory.join("stacks")
  }
}
