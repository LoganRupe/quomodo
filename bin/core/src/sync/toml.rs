use std::collections::HashMap;

use anyhow::Context;
use indexmap::IndexMap;
use komodo_client::entities::{
  action::Action, procedure::Procedure, repo::Repo,
  resource::Resource, server::Server, stack::Stack,
  sync::ResourceSync, tag::Tag, toml::ResourceToml,
};
use partial_derive2::PartialDiff;

use crate::{
  resource::KomodoResource, sync::replace_ids::ReplaceIds,
};

pub const TOML_PRETTY_OPTIONS: toml_pretty::Options =
  toml_pretty::Options {
    tab: "  ",
    skip_empty_string: true,
    // Usually we do this, but has to be changed for some cases.
    skip_empty_object: true,
    max_inline_array_length: 30,
    inline_array: false,
  };

pub trait ToToml: ReplaceIds {
  fn edit_config_object(
    _resource: &ResourceToml<Self::PartialConfig>,
    config: IndexMap<String, serde_json::Value>,
  ) -> anyhow::Result<IndexMap<String, serde_json::Value>> {
    Ok(config)
  }

  fn push_additional(
    _resource: ResourceToml<Self::PartialConfig>,
    _toml: &mut String,
  ) {
  }

  fn push_to_toml_string(
    mut resource: ResourceToml<Self::PartialConfig>,
    toml: &mut String,
  ) -> anyhow::Result<()> {
    resource.config =
      Self::Config::default().minimize_partial(resource.config);

    let mut resource_map: IndexMap<String, serde_json::Value> =
      serde_json::from_str(&serde_json::to_string(&resource)?)?;
    resource_map.shift_remove("config");

    let config = serde_json::from_str(&serde_json::to_string(
      &resource.config,
    )?)?;

    let config = Self::edit_config_object(&resource, config)?;

    toml.push_str(
      &toml_pretty::to_string(&resource_map, TOML_PRETTY_OPTIONS)
        .context("failed to serialize resource to toml")?,
    );

    toml.push_str(&format!(
      "\n[{}.config]\n",
      Self::resource_type().toml_header()
    ));

    toml.push_str(
      &toml_pretty::to_string(&config, TOML_PRETTY_OPTIONS)
        .context("failed to serialize resource config to toml")?,
    );

    Self::push_additional(resource, toml);

    Ok(())
  }
}

pub fn resource_toml_to_toml_string<R: ToToml>(
  resource: ResourceToml<R::PartialConfig>,
) -> anyhow::Result<String> {
  let mut toml = String::new();
  toml
    .push_str(&format!("[[{}]]\n", R::resource_type().toml_header()));
  R::push_to_toml_string(resource, &mut toml)?;
  Ok(toml)
}

pub fn resource_push_to_toml<R: ToToml>(
  mut resource: Resource<R::Config, R::Info>,
  deploy: bool,
  after: Vec<String>,
  toml: &mut String,
  all_tags: &HashMap<String, Tag>,
) -> anyhow::Result<()> {
  R::replace_ids(&mut resource.config);
  if !toml.is_empty() {
    toml.push_str("\n\n##\n\n");
  }
  toml
    .push_str(&format!("[[{}]]\n", R::resource_type().toml_header()));
  R::push_to_toml_string(
    convert_resource::<R>(resource, deploy, after, all_tags),
    toml,
  )?;
  Ok(())
}

pub fn resource_to_toml<R: ToToml>(
  resource: Resource<R::Config, R::Info>,
  deploy: bool,
  after: Vec<String>,
  all_tags: &HashMap<String, Tag>,
) -> anyhow::Result<String> {
  let mut toml = String::new();
  resource_push_to_toml::<R>(
    resource, deploy, after, &mut toml, all_tags,
  )?;
  Ok(toml)
}

pub fn convert_resource<R: KomodoResource>(
  resource: Resource<R::Config, R::Info>,
  deploy: bool,
  after: Vec<String>,
  all_tags: &HashMap<String, Tag>,
) -> ResourceToml<R::PartialConfig> {
  ResourceToml {
    name: resource.name,
    description: resource.description,
    template: resource.template,
    tags: resource
      .tags
      .iter()
      .filter_map(|t| all_tags.get(t).map(|t| t.name.clone()))
      .collect(),
    deploy,
    after,
    // The config still needs to be minimized.
    // This happens in ToToml::push_to_toml
    config: resource.config.into(),
  }
}

impl ToToml for Server {}
impl ToToml for Action {}
impl ToToml for ResourceSync {}

impl ToToml for Stack {
  fn edit_config_object(
    _resource: &ResourceToml<Self::PartialConfig>,
    config: IndexMap<String, serde_json::Value>,
  ) -> anyhow::Result<IndexMap<String, serde_json::Value>> {
    config
      .into_iter()
      .map(|(key, value)| {
        if key == "server_id" {
          Ok((String::from("server"), value))
        } else {
          Ok((key, value))
        }
      })
      .collect()
  }
}

impl ToToml for Repo {
  fn edit_config_object(
    _resource: &ResourceToml<Self::PartialConfig>,
    config: IndexMap<String, serde_json::Value>,
  ) -> anyhow::Result<IndexMap<String, serde_json::Value>> {
    config
      .into_iter()
      .map(|(key, value)| {
        match key.as_str() {
          "server_id" => return Ok((String::from("server"), value)),
          "builder_id" => {
            return Ok((String::from("builder"), value));
          }
          _ => {}
        }
        Ok((key, value))
      })
      .collect()
  }
}

impl ToToml for Procedure {
  fn push_to_toml_string(
    mut resource: ResourceToml<Self::PartialConfig>,
    toml: &mut String,
  ) -> anyhow::Result<()> {
    resource.config =
      Self::Config::default().minimize_partial(resource.config);

    let mut parsed: IndexMap<String, serde_json::Value> =
      serde_json::from_str(&serde_json::to_string(&resource)?)?;

    let config = parsed
      .get_mut("config")
      .context("procedure has no config?")?
      .as_object_mut()
      .context("config is not object?")?;

    let stages = config.remove("stages");

    toml.push_str(
      &toml_pretty::to_string(&parsed, TOML_PRETTY_OPTIONS)
        .context("failed to serialize procedures to toml")?,
    );

    if let Some(stages) = stages {
      let stages =
        stages.as_array().context("stages is not array")?;
      for stage in stages {
        toml.push_str("\n\n[[procedure.config.stage]]\n");
        toml.push_str(
          &toml_pretty::to_string(
            stage,
            // If the execution.params are fully missing,
            // deserialization will fail.
            TOML_PRETTY_OPTIONS.skip_empty_object(false),
          )
          .context("failed to serialize procedures to toml")?,
        );
      }
    }

    Ok(())
  }
}
