//! Decide which Stacks a Resource Sync should deploy after syncing,
//! honouring `deploy` and `after` in the TOML.

use std::collections::HashMap;

use anyhow::{Context, anyhow};
use formatting::{Color, bold, colored, format_serror, muted};
use futures_util::future::join_all;
use komodo_client::{
  api::execute::DeployStack,
  entities::{
    ResourceTarget,
    stack::{PartialStackConfig, Stack, StackConfig, StackState},
    sync::SyncDeployTarget,
    toml::ResourceToml,
    update::Log,
    user::sync_user,
  },
};
use mogh_resolver::Resolve;
use uuid::Uuid;

use crate::{
  api::execute::{ExecuteArgs, ExecuteRequest},
  helpers::update::init_execution_update,
  state::stack_status_cache,
};

use super::ResourceSyncTrait;

/// All entries in here are due to be deployed,
/// after the given dependencies,
/// with the given reason.
pub type ToDeployCache = Vec<SyncDeployTarget>;

#[derive(Clone, Copy)]
pub struct SyncDeployParams<'a> {
  pub stacks: &'a [ResourceToml<PartialStackConfig>],
  // Names to stacks
  pub stack_map: &'a HashMap<String, Stack>,
}

pub async fn deploy_from_cache(
  mut to_deploy: ToDeployCache,
  logs: &mut Vec<Log>,
) {
  if to_deploy.is_empty() {
    return;
  }
  let mut log = format!(
    "{}: running executions to sync stack state",
    muted("INFO")
  );
  let mut round = 1;
  let user = sync_user();

  while !to_deploy.is_empty() {
    // Collect all waiting stacks without waiting dependencies.
    let good_to_deploy = to_deploy
      .iter()
      .filter(|SyncDeployTarget { after, .. }| {
        to_deploy.iter().all(|SyncDeployTarget { target, .. }| {
          !after.contains(target)
        })
      })
      .map(|SyncDeployTarget { target, reason, .. }| {
        (target.clone(), reason.clone())
      })
      .collect::<HashMap<_, _>>();

    // Deploy the ones ready for deployment
    let res = join_all(good_to_deploy.iter().map(
      |(target, reason)| async move {
        let id = Uuid::new_v4();
        let res = async {
          match &target {
            ResourceTarget::Stack(name) => {
              let req = ExecuteRequest::DeployStack(DeployStack {
                stack: name.to_string(),
                services: Vec::new(),
              });

              let update = init_execution_update(&req, user).await?;
              let ExecuteRequest::DeployStack(req) = req else {
                unreachable!()
              };
              req
                .resolve(&ExecuteArgs {
                  user: user.to_owned(),
                  update,
                  task_id: id,
                })
                .await
            }
            _ => unreachable!(),
          }
        }
        .await;
        (target, reason, res)
      },
    ))
    .await;

    log.push_str(&format!(
      "\n\n{}: round {}:",
      muted("INFO"),
      bold(round)
    ));

    for (target, reason, res) in res {
      let (variant, name) = target.extract_variant_id();
      match res {
        Ok(update) => {
          log.push_str(&format!(
            "\n{}: {} {} | {reason}",
            if update.success {
              colored("SUCCESS", Color::Green)
            } else {
              colored("FAILURE", Color::Red)
            },
            bold(variant),
            bold(name),
          ));
        }
        Err(e) => {
          log.push_str(&format!(
            "\n{}: {} {} | {reason} | {}",
            colored("FAILURE", Color::Red),
            bold(variant),
            bold(name),
            format_serror(&e.error.into())
          ));
        }
      }
    }

    // Remove the deployed ones from to_deploy
    to_deploy.retain(|SyncDeployTarget { target, .. }| {
      !good_to_deploy.contains_key(target)
    });

    round += 1;
  }

  logs.push(Log::simple("Sync Deploy", log));
}

pub async fn get_updates_for_view(
  params: SyncDeployParams<'_>,
) -> (Vec<SyncDeployTarget>, Option<String>) {
  match build_deploy_cache(params).await {
    Ok(targets) => (targets, None),
    Err(e) => (Vec::new(), Some(format_serror(&e.into()))),
  }
}

/// Entries are keyed by ResourceTargets wrapping "name" instead of "id".
/// If entry is None, it is confirmed no-deploy.
/// If it is Some, it is confirmed deploy with provided reason and dependencies.
type ToDeployCacheInner =
  HashMap<ResourceTarget, Option<(String, Vec<ResourceTarget>)>>;

pub async fn build_deploy_cache(
  params: SyncDeployParams<'_>,
) -> anyhow::Result<ToDeployCache> {
  let mut cache = ToDeployCacheInner::new();

  for stack in params.stacks {
    build_cache_for_stack(stack, params, &mut cache).await?;
  }

  let cache = cache
    .into_iter()
    .filter_map(|(target, entry)| {
      let (reason, after) = entry?;
      Some((target, (reason, after)))
    })
    .collect::<HashMap<_, _>>();

  // All entries in cache at this point are deploying.
  let clone = cache.clone();

  Ok(
    cache
      .into_iter()
      .map(|(target, (reason, mut after))| {
        // Only keep targets which are deploying.
        after.retain(|target| clone.contains_key(target));
        SyncDeployTarget {
          target,
          reason,
          after,
        }
      })
      .collect(),
  )
}

type BuildRes<'a> = std::pin::Pin<
  Box<
    dyn std::future::Future<Output = anyhow::Result<()>> + Send + 'a,
  >,
>;

fn build_cache_for_stack<'a>(
  stack: &'a ResourceToml<PartialStackConfig>,
  params: SyncDeployParams<'a>,
  cache: &'a mut ToDeployCacheInner,
) -> BuildRes<'a> {
  Box::pin(async move {
    let SyncDeployParams { stacks, stack_map } = params;
    let target = ResourceTarget::Stack(stack.name.clone());

    // First check existing, and continue if already handled.
    if cache.contains_key(&target) {
      return Ok(());
    }

    // Check if stack doesn't have "deploy" enabled.
    if !stack.deploy {
      cache.insert(target, None);
      return Ok(());
    }

    let after = get_after_as_resource_targets(
      &stack.name,
      &stack.after,
      stack_map,
      stacks,
    )?;

    let Some(original) = stack_map.get(&stack.name) else {
      // The stack is not created yet, should definitely deploy
      cache.insert(
        target,
        Some((String::from("deploy on creation"), after)),
      );
      return Ok(());
    };

    let status = &stack_status_cache()
      .get_or_insert_default(&original.id)
      .await
      .curr;
    let state = status.state;

    match state {
      StackState::Unknown => {
        // Can't do anything with unknown state
        cache.insert(target, None);
        return Ok(());
      }
      StackState::Running => {
        // See if any remote contents don't match deployed contents
        if let (Some(deployed_contents), Some(remote_contents)) = (
          &original.info.deployed_contents,
          &original.info.remote_contents,
        ) {
          for file in remote_contents {
            match deployed_contents.iter().find(|c| c.path == file.path) {
              Some(deployed) if deployed.contents == file.contents => {}
              Some(_) => {
                cache.insert(
                  target,
                  Some((
                    format!("File contents for {} have changed", file.path),
                    after,
                  )),
                );
                return Ok(());
              }
              None => {
                cache.insert(
                  target,
                  Some((
                    format!("New file at {}", file.path),
                    after,
                  )),
                );
                return Ok(());
              }
            }
          }
          if deployed_contents
            .iter()
            .any(|d| !remote_contents.iter().any(|r| r.path == d.path))
          {
            cache.insert(
              target,
              Some((String::from("A deployed file was removed"), after)),
            );
            return Ok(());
          }
        }

        // Merge toml resource config (partial) onto default resource config.
        let config: StackConfig = stack.config.clone().into();
        let config: PartialStackConfig = config.into();

        let mut diff =
          Stack::get_diff(original.config.clone(), config)?;

        Stack::validate_diff(&mut diff);
        // Only config fields that affect what gets installed.
        let changed = diff.server_id.is_some()
          || diff.run_directory.is_some()
          || diff.skip_secret_interp.is_some()
          || diff.secrets_collection.is_some()
          || diff.linked_repo.is_some()
          || diff.repo.is_some()
          || diff.branch.is_some()
          || diff.commit.is_some()
          || diff.clone_path.is_some();
        if changed {
          cache.insert(
            target,
            Some((String::from("stack config has changed"), after)),
          );
          return Ok(());
        }
      }
      // All other cases will require Deploy to enter Running state.
      _ => {
        cache.insert(
          target,
          Some((
            format!("stack has {} state", colored(state, Color::Red)),
            after,
          )),
        );
        return Ok(());
      }
    };

    // Check 'after' to see if they deploy.
    for parent in &after {
      let ResourceTarget::Stack(name) = parent else {
        unreachable!()
      };
      let Some(parent_stack) = stacks.iter().find(|d| &d.name == name)
      else {
        // The parent is not in the sync, so won't be deploying.
        // Cross-sync deploy dependencies are not supported.
        continue;
      };
      // Recurse to add the parent to cache, then check again.
      build_cache_for_stack(parent_stack, params, cache).await?;
      match cache.get(parent) {
        Some(Some(_)) => {
          let (variant, name) = parent.extract_variant_id();
          cache.insert(
            target.to_owned(),
            Some((
              format!(
                "{variant} parent dependency '{}' is deploying",
                bold(name)
              ),
              after,
            )),
          );
          return Ok(());
        }
        // The parent will not deploy, do nothing here.
        Some(None) => {}
        None => {
          return Err(anyhow!(
            "Did not find parent in cache after build recursion. This should not happen."
          ));
        }
      }
    }

    // If it has reached here, its not deploying
    cache.insert(target, None);
    Ok(())
  })
}

fn get_after_as_resource_targets(
  resource_name: &str,
  after: &[String],
  // Names to stacks
  stack_map: &HashMap<String, Stack>,
  stacks: &[ResourceToml<PartialStackConfig>],
) -> anyhow::Result<Vec<ResourceTarget>> {
  after
    .iter()
    .map(|name| {
      if stack_map.contains_key(name)
        || stacks.iter().any(|stack| stack.name.as_str() == name)
      {
        Ok(ResourceTarget::Stack(name.clone()))
      } else {
        Err(anyhow!("failed to match deploy dependency in 'after' list | resource: {resource_name} | dependency: {name}"))
      }
    })
    .collect::<anyhow::Result<Vec<_>>>()
    .context("Invalid 'after' list")
}
