use bson::{Document, doc};
use derive_builder::Builder;
use derive_default_builder::DefaultBuilder;
use partial_derive2::Partial;
use serde::{Deserialize, Serialize};
use strum::Display;
use typeshare::typeshare;

use crate::deserializers::{
  option_string_list_deserializer, string_list_deserializer,
};

use super::{
  FileContents, SystemCommand,
  docker::container::{ContainerListItem, ContainerStateStatusEnum},
  resource::{Resource, ResourceListItem, ResourceQuery},
};

#[cfg(feature = "utoipa")]
#[derive(utoipa::ToSchema)]
#[schema(as = Stack)]
pub struct StackSchema(
  #[schema(inline)] pub Resource<StackConfig, StackInfo>,
);

/// A Stack is a directory of podman quadlet unit files
/// (`.container`, `.pod`, `.network`, `.volume`, `.build`, `.image`, `.kube`)
/// plus any config files they reference, sourced from a git repo.
#[typeshare]
pub type Stack = Resource<StackConfig, StackInfo>;

#[typeshare]
pub type StackListItem = ResourceListItem<StackListItemInfo>;

#[typeshare]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct StackListItemInfo {
  /// The server that stack is deployed on.
  pub server_id: String,
  /// The name of the server that stack is deployed on.
  #[serde(default)]
  pub server_name: String,
  /// Linked repo, if one is attached.
  pub linked_repo: String,
  /// The name of the linked repo, if one is attached.
  #[serde(default)]
  pub linked_repo_name: String,
  /// The git provider domain
  pub git_provider: String,
  /// The configured repo
  pub repo: String,
  /// The configured branch
  pub branch: String,
  /// Full link to the repo.
  pub repo_link: String,
  /// The stack state
  pub state: StackState,
  /// A string conveying the status of the stack.
  pub status: Option<String>,
  /// The quadlet units that are part of the stack.
  /// If deployed, will be `deployed_units`.
  /// Otherwise, its `latest_units`
  pub units: Vec<QuadletUnit>,
  /// If the stack directory has no quadlet units, or could not be read.
  /// If true, this is an unhealthy state, and deploying will fail.
  pub missing_units: bool,
  /// Deployed short commit hash, or null.
  pub deployed_hash: Option<String>,
  /// Latest short commit hash, or null.
  pub latest_hash: Option<String>,
}

#[typeshare]
#[derive(
  Debug,
  Clone,
  Copy,
  Default,
  PartialEq,
  Eq,
  PartialOrd,
  Ord,
  Serialize,
  Deserialize,
  Display,
)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum StackState {
  /// The stack is currently re/deploying
  Deploying,
  /// All containers are running.
  Running,
  /// All containers are paused
  Paused,
  /// All contianers are stopped
  Stopped,
  /// All containers are created
  Created,
  /// All containers are restarting
  Restarting,
  /// All containers are dead
  Dead,
  /// All containers are removing
  Removing,
  /// The containers are in a mix of states
  Unhealthy,
  /// The stack is not deployed
  Down,
  /// Server not reachable for status
  #[default]
  Unknown,
}

#[typeshare]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct StackInfo {
  /// Deployed short commit hash, or null.
  pub deployed_hash: Option<String>,
  /// Deployed commit message, or null.
  pub deployed_message: Option<String>,
  /// Hash over the deployed file contents and the resolved secret values.
  /// Used by `DeployStackIfChanged` to decide whether a redeploy is needed.
  /// Secret values themselves are never stored, only their hash contributes.
  pub deployed_contents_hash: Option<String>,
  /// The deployed file contents (with secret references unresolved).
  /// This is updated whenever the stack is successfully deployed.
  pub deployed_contents: Option<Vec<FileContents>>,
  /// The deployed quadlet units.
  /// This is updated whenever the stack is successfully deployed.
  pub deployed_units: Option<Vec<QuadletUnit>>,
  /// The latest quadlet units, parsed from the latest remote contents.
  /// This is updated whenever the stack cache refreshes.
  #[serde(default)]
  pub latest_units: Vec<QuadletUnit>,
  /// The latest file contents in the stack directory (secret references unresolved).
  /// This is updated whenever the stack cache refreshes.
  pub remote_contents: Option<Vec<FileContents>>,
  /// If there was an error in getting the remote contents, it will be here.
  pub remote_errors: Option<Vec<FileContents>>,
  /// Latest commit hash, or null
  pub latest_hash: Option<String>,
  /// Latest commit message, or null
  pub latest_message: Option<String>,
}

#[typeshare(serialized_as = "Partial<StackConfig>")]
pub type _PartialStackConfig = PartialStackConfig;

/// The quadlet stack configuration.
#[typeshare]
#[derive(Debug, Clone, Serialize, Deserialize, Builder, Partial)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[partial_derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(
  feature = "schemars",
  partial_derive(schemars::JsonSchema)
)]
#[diff_derive(Debug, Clone, Default, Serialize, Deserialize)]
#[partial(skip_serializing_none, from, diff)]
pub struct StackConfig {
  /// The Server to deploy the Stack on.
  #[serde(default, alias = "server")]
  #[partial_attr(serde(alias = "server"))]
  #[cfg_attr(
    feature = "schemars",
    partial_attr(schemars(rename = "server"))
  )]
  #[builder(default)]
  pub server_id: String,

  /// Configure quick links that are displayed in the resource header
  #[serde(default, deserialize_with = "string_list_deserializer")]
  #[partial_attr(serde(
    default,
    deserialize_with = "option_string_list_deserializer"
  ))]
  #[builder(default)]
  pub links: Vec<String>,

  /// Whether to `podman pull` the images of every container unit
  /// before restarting units. Ensures latest images are deployed
  /// while minimizing downtime.
  #[serde(default = "default_auto_pull")]
  #[builder(default = "default_auto_pull()")]
  #[partial_default(default_auto_pull())]
  pub auto_pull: bool,

  /// Whether to skip variable / secret interpolation into the stack files.
  #[serde(default)]
  #[builder(default)]
  pub skip_secret_interp: bool,

  /// The secret store collection this stack reads secrets from.
  /// Required if any file in the stack references a secret.
  /// A stack that references a secret without naming a collection fails at sync.
  #[serde(default)]
  #[builder(default)]
  pub secrets_collection: String,

  /// Choose a Repo (Resource) to source the quadlet files.
  #[serde(default)]
  #[builder(default)]
  pub linked_repo: String,

  /// The git provider domain. Default: github.com
  #[serde(default = "default_git_provider")]
  #[builder(default = "default_git_provider()")]
  #[partial_default(default_git_provider())]
  pub git_provider: String,

  /// Whether to use https to clone the repo (versus http). Default: true
  #[serde(default = "default_git_https")]
  #[builder(default = "default_git_https()")]
  #[partial_default(default_git_https())]
  pub git_https: bool,

  /// The git account used to access private repos.
  /// Passing empty string can only clone public repos.
  #[serde(default)]
  #[builder(default)]
  pub git_account: String,

  /// The repo used as the source of the stack.
  /// {namespace}/{repo_name}
  #[serde(default)]
  #[builder(default)]
  pub repo: String,

  /// The branch of the repo.
  #[serde(default = "default_branch")]
  #[builder(default = "default_branch()")]
  #[partial_default(default_branch())]
  pub branch: String,

  /// Optionally set a specific commit hash.
  #[serde(default)]
  #[builder(default)]
  pub commit: String,

  /// Optionally set a specific clone path
  #[serde(default)]
  #[builder(default)]
  pub clone_path: String,

  /// By default, the Stack will `git pull` the repo after it is first cloned.
  /// If this option is enabled, the repo folder will be deleted and recloned instead.
  #[serde(default)]
  #[builder(default)]
  pub reclone: bool,

  /// Whether incoming webhooks actually trigger action.
  #[serde(default = "default_webhook_enabled")]
  #[builder(default = "default_webhook_enabled()")]
  #[partial_default(default_webhook_enabled())]
  pub webhook_enabled: bool,

  /// Optionally provide an alternate webhook secret for this stack.
  /// If its an empty string, use the default secret from the config.
  #[serde(default)]
  #[builder(default)]
  pub webhook_secret: String,

  /// By default, the Stack will `DeployStackIfChanged`.
  /// If this option is enabled, will always run `DeployStack` without diffing.
  #[serde(default)]
  #[builder(default)]
  pub webhook_force_deploy: bool,

  /// The directory inside the repo holding the quadlet unit files.
  /// Relative to the repo root.
  #[serde(default)]
  #[builder(default)]
  pub run_directory: String,

  /// Whether to send StackStateChange alerts for this stack.
  #[serde(default = "default_send_alerts")]
  #[builder(default = "default_send_alerts()")]
  #[partial_default(default_send_alerts())]
  pub send_alerts: bool,

  /// Used with `registry_account` to `podman login` before pulling images.
  #[serde(default)]
  #[builder(default)]
  pub registry_provider: String,

  /// Used with `registry_provider` to `podman login` before pulling images.
  #[serde(default)]
  #[builder(default)]
  pub registry_account: String,

  /// The optional command to run before the Stack is deployed.
  #[serde(default)]
  #[builder(default)]
  pub pre_deploy: SystemCommand,

  /// The optional command to run after the Stack is deployed.
  #[serde(default)]
  #[builder(default)]
  pub post_deploy: SystemCommand,

  /// Ignore certain units when checking the stack status.
  /// For example, an init container might be exited, but the
  /// stack should be healthy. Give the unit file name, eg `init.container`.
  #[serde(default, deserialize_with = "string_list_deserializer")]
  #[partial_attr(serde(
    default,
    deserialize_with = "option_string_list_deserializer"
  ))]
  #[builder(default)]
  pub ignore_units: Vec<String>,
}

impl StackConfig {
  pub fn builder() -> StackConfigBuilder {
    StackConfigBuilder::default()
  }
}

fn default_auto_pull() -> bool {
  true
}

fn default_git_provider() -> String {
  String::from("github.com")
}

fn default_git_https() -> bool {
  true
}

fn default_branch() -> String {
  String::from("main")
}

fn default_webhook_enabled() -> bool {
  true
}

fn default_send_alerts() -> bool {
  true
}

impl Default for StackConfig {
  fn default() -> Self {
    Self {
      server_id: Default::default(),
      links: Default::default(),
      auto_pull: default_auto_pull(),
      skip_secret_interp: Default::default(),
      secrets_collection: Default::default(),
      linked_repo: Default::default(),
      git_provider: default_git_provider(),
      git_https: default_git_https(),
      git_account: Default::default(),
      repo: Default::default(),
      branch: default_branch(),
      commit: Default::default(),
      clone_path: Default::default(),
      reclone: Default::default(),
      webhook_enabled: default_webhook_enabled(),
      webhook_secret: Default::default(),
      webhook_force_deploy: Default::default(),
      run_directory: Default::default(),
      send_alerts: default_send_alerts(),
      registry_provider: Default::default(),
      registry_account: Default::default(),
      pre_deploy: Default::default(),
      post_deploy: Default::default(),
      ignore_units: Default::default(),
    }
  }
}

#[cfg(feature = "utoipa")]
impl utoipa::PartialSchema for PartialStackConfig {
  fn schema()
  -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    utoipa::schema!(#[inline] std::collections::HashMap<String, serde_json::Value>).into()
  }
}

#[cfg(feature = "utoipa")]
impl utoipa::ToSchema for PartialStackConfig {}

/// The kind of quadlet unit, by file extension.
#[typeshare]
#[derive(
  Debug,
  Clone,
  Copy,
  PartialEq,
  Eq,
  Hash,
  Default,
  Serialize,
  Deserialize,
  Display,
)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum QuadletKind {
  #[default]
  Container,
  Pod,
  Network,
  Volume,
  Build,
  Image,
  Kube,
}

impl QuadletKind {
  /// Parse from the unit file name, eg `web.container`.
  pub fn from_file_name(file_name: &str) -> Option<QuadletKind> {
    let (_, ext) = file_name.rsplit_once('.')?;
    match ext {
      "container" => Some(QuadletKind::Container),
      "pod" => Some(QuadletKind::Pod),
      "network" => Some(QuadletKind::Network),
      "volume" => Some(QuadletKind::Volume),
      "build" => Some(QuadletKind::Build),
      "image" => Some(QuadletKind::Image),
      "kube" => Some(QuadletKind::Kube),
      _ => None,
    }
  }

  /// The systemd service name quadlet generates for a unit file
  /// with this kind and the given stem (file name without extension).
  pub fn service_name(&self, stem: &str) -> String {
    match self {
      QuadletKind::Container | QuadletKind::Kube => {
        format!("{stem}.service")
      }
      QuadletKind::Pod => format!("{stem}-pod.service"),
      QuadletKind::Network => format!("{stem}-network.service"),
      QuadletKind::Volume => format!("{stem}-volume.service"),
      QuadletKind::Build => format!("{stem}-build.service"),
      QuadletKind::Image => format!("{stem}-image.service"),
    }
  }

  /// Whether this kind of unit is started directly on deploy.
  /// Network / volume / image / build units are pulled in
  /// as dependencies of the units that reference them.
  pub fn is_runnable(&self) -> bool {
    matches!(
      self,
      QuadletKind::Container | QuadletKind::Pod | QuadletKind::Kube
    )
  }
}

/// A quadlet unit belonging to a Stack.
#[typeshare]
#[derive(
  Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize,
)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct QuadletUnit {
  /// The unit file name, relative to the stack directory. Eg `web.container`.
  pub file: String,
  /// The kind of unit
  pub kind: QuadletKind,
  /// The systemd service name quadlet generates. Eg `web.service`, `db-pod.service`.
  pub service: String,
  /// The podman object name (container / pod / network / volume name).
  /// Quadlet defaults this to `systemd-{stem}` unless overridden in the unit.
  pub name: String,
  /// For container units, the `Image=`.
  #[serde(default)]
  pub image: String,
  /// For container units, the `Pod=` reference, if any (eg `db.pod`).
  #[serde(default)]
  pub pod: Option<String>,
}

#[typeshare]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct StackService {
  pub stack_id: String,
  #[serde(default)]
  pub stack_name: String,
  /// The unit file name, eg `web.container`
  pub service: String,
  /// The unit
  pub unit: QuadletUnit,
  /// The container backing the unit, if found.
  pub container: Option<ContainerListItem>,
  pub state: StackServiceState,
}

#[typeshare]
#[derive(
  Debug,
  Clone,
  Copy,
  PartialEq,
  Eq,
  PartialOrd,
  Ord,
  Default,
  Serialize,
  Deserialize,
  Display,
)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub enum StackServiceState {
  Healthy,
  Unhealthy,
  Down,
  Running,
  Created,
  Paused,
  Restarting,
  Exited,
  Stopping,
  Removing,
  Dead,
  #[default]
  Unknown,
}

impl From<ContainerStateStatusEnum> for StackServiceState {
  fn from(value: ContainerStateStatusEnum) -> Self {
    match value {
      ContainerStateStatusEnum::Running => StackServiceState::Running,
      ContainerStateStatusEnum::Created => StackServiceState::Created,
      ContainerStateStatusEnum::Paused => StackServiceState::Paused,
      ContainerStateStatusEnum::Restarting => {
        StackServiceState::Restarting
      }
      ContainerStateStatusEnum::Exited => StackServiceState::Exited,
      ContainerStateStatusEnum::Stopping => {
        StackServiceState::Stopping
      }
      ContainerStateStatusEnum::Removing => {
        StackServiceState::Removing
      }
      ContainerStateStatusEnum::Dead => StackServiceState::Dead,
      ContainerStateStatusEnum::Empty => StackServiceState::Unknown,
    }
  }
}

#[typeshare]
#[derive(Serialize, Deserialize, Debug, Clone, Copy, Default)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct StackActionState {
  pub pulling: bool,
  pub deploying: bool,
  pub starting: bool,
  pub restarting: bool,
  pub stopping: bool,
  pub destroying: bool,
}

#[typeshare]
pub type StackQuery = ResourceQuery<StackQuerySpecifics>;

#[typeshare]
#[derive(
  Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize,
)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub enum StackSortBy {
  #[default]
  Name,
  Source,
  Host,
  State,
}

#[typeshare]
#[derive(
  Serialize, Deserialize, Debug, Clone, Default, DefaultBuilder,
)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct StackQuerySpecifics {
  #[serde(default)]
  pub server_ids: Vec<String>,
  #[serde(default)]
  pub linked_repos: Vec<String>,
  #[serde(default)]
  pub repos: Vec<String>,
  #[serde(default)]
  pub states: Vec<StackState>,
}

impl super::resource::AddFilters for StackQuerySpecifics {
  fn add_filters(&self, filters: &mut Document) {
    if !self.server_ids.is_empty() {
      filters
        .insert("config.server_id", doc! { "$in": &self.server_ids });
    }
    if !self.linked_repos.is_empty() {
      filters.insert(
        "config.linked_repo",
        doc! { "$in": &self.linked_repos },
      );
    }
    if !self.repos.is_empty() {
      filters.insert("config.repo", doc! { "$in": &self.repos });
    }
  }
}
