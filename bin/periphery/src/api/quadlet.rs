use std::time::Duration;

use anyhow::{Context, anyhow};
use command::{
  CommandOptions, KomodoCommandMode, run_komodo_command_with_sanitization,
  run_komodo_shell_command,
};
use formatting::format_serror;
use komodo_client::entities::{
  FileContents, all_logs_success, stack::QuadletKind, update::Log,
};
use mogh_resolver::Resolve;
use periphery_client::api::quadlet::*;
use shell_escape::unix::escape;
use tracing::Instrument;

use crate::{
  helpers::format_log_grep,
  podman,
  quadlet::{
    self, deploy, installed_units, parse_units, prepare::prepare,
    pull_or_clone_stack, read_stack_files, secret_prefix, select_units,
    stack_quadlet_dir, state, units_only,
  },
  systemd,
};

impl Resolve<crate::api::Args> for ReadStack {
  #[instrument(
    "ReadStack",
    skip_all,
    fields(id = args.id.to_string(), core = args.core, stack = self.stack.name)
  )]
  async fn resolve(
    self,
    args: &crate::api::Args,
  ) -> anyhow::Result<ReadStackResponse> {
    let ReadStack {
      stack,
      repo,
      git_token,
    } = self;
    let mut res = ReadStackResponse::default();
    let pulled = match pull_or_clone_stack(
      &stack,
      repo.as_ref(),
      git_token,
      &mut res.logs,
      args,
    )
    .await
    {
      Ok(pulled) => pulled,
      Err(e) => {
        let error = format_serror(&e.into());
        res.logs.push(Log::error("Pull Stack Repo", error.clone()));
        res.errors.push(FileContents {
          path: String::new(),
          contents: error,
        });
        return Ok(res);
      }
    };
    res.commit_hash = pulled.commit_hash;
    res.commit_message = pulled.commit_message;
    match read_stack_files(&pulled.run_directory).await {
      Ok((files, errors)) => {
        res.units = units_only(&parse_units(&files));
        res.files = files;
        res.errors = errors;
      }
      Err(e) => {
        let error = format_serror(&e.into());
        res.logs.push(Log::error("Read Stack Files", error.clone()));
        res.errors.push(FileContents {
          path: stack.config.run_directory.clone(),
          contents: error,
        });
      }
    }
    Ok(res)
  }
}

//

impl Resolve<crate::api::Args> for DeployStack {
  #[instrument(
    "DeployStack",
    skip_all,
    fields(
      id = args.id.to_string(),
      core = args.core,
      stack = self.stack.name,
      services = format!("{:?}", self.services),
    )
  )]
  async fn resolve(
    self,
    args: &crate::api::Args,
  ) -> anyhow::Result<DeployStackResponse> {
    let DeployStack {
      stack,
      repo,
      git_token,
      registry_token,
      variables,
      secrets,
      mut replacers,
      services,
    } = self;

    let mut res = DeployStackResponse::default();
    // Never let a secret value reach a log.
    for (name, value) in &secrets {
      if !value.is_empty() {
        replacers.push((value.clone(), format!("[[{name}]]")));
      }
    }

    // 1. Pull / clone
    let pulled = match pull_or_clone_stack(
      &stack,
      repo.as_ref(),
      git_token,
      &mut res.logs,
      args,
    )
    .await
    {
      Ok(pulled) => pulled,
      Err(e) => {
        res
          .logs
          .push(Log::error("Pull Stack Repo", format_serror(&e.into())));
        return Ok(res);
      }
    };
    res.commit_hash = pulled.commit_hash.clone();
    res.commit_message = pulled.commit_message.clone();
    if !all_logs_success(&res.logs) {
      return Ok(res);
    }

    // 2. Read + prepare
    let (files, errors) = match read_stack_files(&pulled.run_directory).await {
      Ok(r) => r,
      Err(e) => {
        res.logs.push(Log::error("Read Stack Files", format_serror(&e.into())));
        return Ok(res);
      }
    };
    res.file_contents = files.clone();
    res.remote_errors = errors;
    if !res.remote_errors.is_empty() {
      res.logs.push(Log::error(
        "Read Stack Files",
        res
          .remote_errors
          .iter()
          .map(|e| format!("{}: {}", e.path, e.contents))
          .collect::<Vec<_>>()
          .join("\n"),
      ));
      return Ok(res);
    }
    let prepared = match prepare(&stack, &files, &variables, &secrets) {
      Ok(p) => p,
      Err(e) => {
        res.logs.push(Log::error("Prepare Stack", format_serror(&e.into())));
        return Ok(res);
      }
    };
    if prepared.units.is_empty() {
      res.logs.push(Log::error(
        "Prepare Stack",
        format!(
          "No quadlet unit files (.container, .pod, .network, .volume, .build, .image, .kube) found under run directory '{}'",
          stack.config.run_directory
        ),
      ));
      return Ok(res);
    }
    res.units = units_only(&prepared.units);
    res.contents_hash = Some(quadlet::contents_hash(&files, &prepared.secret_hashes));
    res.logs.push(Log::simple(
      "Units",
      prepared
        .units
        .iter()
        .map(|u| format!("{} -> {} ({})", u.unit.file, u.unit.service, u.unit.name))
        .collect::<Vec<_>>()
        .join("\n"),
    ));

    let previous = match state::load(&stack.name).await {
      Ok(p) => p,
      Err(e) => {
        warn!("Ignoring unreadable stack state | {e:#}");
        None
      }
    };

    // 3. Registry login + pull
    if !stack.config.registry_provider.is_empty()
      && !stack.config.registry_account.is_empty()
      && let Err(e) = podman::login(
        &stack.config.registry_provider,
        &stack.config.registry_account,
        registry_token.as_deref(),
      )
      .await
    {
      res.logs.push(Log::error("Login to Registry", format_serror(&e.into())));
      return Ok(res);
    }
    if stack.config.auto_pull {
      for u in prepared.units.iter().filter(|u| {
        u.unit.kind == QuadletKind::Container
          && !u.unit.image.is_empty()
          && !u.unit.image.ends_with(".image")
          && !u.unit.image.ends_with(".build")
      }) {
        let log = podman::pull(&u.unit.image).await;
        let ok = log.success;
        res.logs.push(log);
        if !ok {
          return Ok(res);
        }
      }
    }

    // 4. Pre deploy
    if !stack.config.pre_deploy.is_none() {
      let path = pulled.run_directory.join(&stack.config.pre_deploy.path);
      let span = info_span!("ExecutePreDeploy");
      if let Some(log) = run_komodo_command_with_sanitization(
        "Pre Deploy",
        &stack.config.pre_deploy.command,
        CommandOptions::default().path(path.as_path()),
        if stack.config.pre_deploy.shell_mode {
          KomodoCommandMode::Shell
        } else {
          KomodoCommandMode::Multiline
        },
        &replacers,
      )
      .instrument(span)
      .await
      {
        res.logs.push(log);
        if !all_logs_success(&res.logs) {
          return Ok(res);
        }
      }
    }

    // 5. Plan, write, secrets, apply
    let plan = deploy::plan(&prepared, previous.as_ref(), &services);
    let target = stack_quadlet_dir(&stack.name);
    let file_hashes = match deploy::write_files(&target, &prepared, &mut res.logs).await {
      Ok(h) => h,
      Err(e) => {
        res.logs.push(Log::error("Write Stack Files", format_serror(&e.into())));
        return Ok(res);
      }
    };
    if let Err(e) = deploy::create_secrets(&prepared, previous.as_ref(), &mut res.logs).await {
      res.logs.push(Log::error("Podman Secrets", format_serror(&e.into())));
      return Ok(res);
    }
    let applied = deploy::apply(&plan, &mut res.logs).await?;
    for log in res.logs.iter_mut() {
      log.sanitize(&replacers);
    }

    // Save state even on failure: files and secrets are on the host now.
    let next = deploy::next_state(file_hashes, &prepared);
    if let Err(e) = state::save(&stack.name, &next).await {
      res.logs.push(Log::error("Save Stack State", format_serror(&e.into())));
      return Ok(res);
    }
    res.deployed = applied;
    if !applied {
      return Ok(res);
    }

    // 6. Post deploy
    if !stack.config.post_deploy.is_none() {
      let path = pulled.run_directory.join(&stack.config.post_deploy.path);
      let span = info_span!("ExecutePostDeploy");
      if let Some(log) = run_komodo_command_with_sanitization(
        "Post Deploy",
        &stack.config.post_deploy.command,
        CommandOptions::default().path(path.as_path()),
        if stack.config.post_deploy.shell_mode {
          KomodoCommandMode::Shell
        } else {
          KomodoCommandMode::Multiline
        },
        &replacers,
      )
      .instrument(span)
      .await
      {
        res.logs.push(log);
      }
    }

    Ok(res)
  }
}

//

impl Resolve<crate::api::Args> for PullStack {
  #[instrument(
    "PullStack",
    skip_all,
    fields(id = args.id.to_string(), core = args.core, stack = self.stack.name)
  )]
  async fn resolve(
    self,
    args: &crate::api::Args,
  ) -> anyhow::Result<PullStackResponse> {
    let PullStack {
      stack,
      repo,
      git_token,
      registry_token,
      services,
    } = self;
    let mut res = PullStackResponse::default();
    let pulled = match pull_or_clone_stack(
      &stack,
      repo.as_ref(),
      git_token,
      &mut res.logs,
      args,
    )
    .await
    {
      Ok(pulled) => pulled,
      Err(e) => {
        res
          .logs
          .push(Log::error("Pull Stack Repo", format_serror(&e.into())));
        return Ok(res);
      }
    };
    let (files, _) = read_stack_files(&pulled.run_directory).await?;
    let units = parse_units(&files);
    let selected = select_units(&units, &services)?;
    if !stack.config.registry_provider.is_empty()
      && !stack.config.registry_account.is_empty()
      && let Err(e) = podman::login(
        &stack.config.registry_provider,
        &stack.config.registry_account,
        registry_token.as_deref(),
      )
      .await
    {
      res.logs.push(Log::error("Login to Registry", format_serror(&e.into())));
      return Ok(res);
    }
    for u in selected.into_iter().filter(|u| {
      u.unit.kind == QuadletKind::Container
        && !u.unit.image.is_empty()
        && !u.unit.image.ends_with(".image")
        && !u.unit.image.ends_with(".build")
    }) {
      res.logs.push(podman::pull(&u.unit.image).await);
    }
    if res.logs.is_empty() {
      res.logs.push(Log::simple("Pull Images", String::from("No container units with a pullable image")));
    }
    Ok(res)
  }
}

//

impl Resolve<crate::api::Args> for StackUnits {
  #[instrument(
    "StackUnits",
    skip_all,
    fields(id = args.id.to_string(), core = args.core, stack = self.stack, action = format!("{:?}", self.action))
  )]
  async fn resolve(
    self,
    args: &crate::api::Args,
  ) -> anyhow::Result<Log> {
    let StackUnits {
      stack,
      action,
      services,
    } = self;
    let units = installed_units(&stack).await?;
    if units.is_empty() {
      return Err(anyhow!(
        "Stack '{stack}' is not installed on this host ({})",
        stack_quadlet_dir(&stack).display()
      ));
    }
    let selected = select_units(&units, &services)?
      .into_iter()
      .filter(|u| u.unit.kind.is_runnable())
      // Containers in a pod follow the pod when acting on everything.
      .filter(|u| !services.is_empty() || u.unit.pod.is_none() || u.unit.kind != QuadletKind::Container)
      .map(|u| u.unit.service.clone())
      .collect::<Vec<_>>();
    let (stage, verb) = match action {
      StackUnitAction::Start => ("Start Stack", "start"),
      StackUnitAction::Stop => ("Stop Stack", "stop"),
      StackUnitAction::Restart => ("Restart Stack", "restart"),
    };
    Ok(systemd::systemctl(stage, verb, &selected).await)
  }
}

//

impl Resolve<crate::api::Args> for DestroyStack {
  #[instrument(
    "DestroyStack",
    skip_all,
    fields(id = args.id.to_string(), core = args.core, stack = self.stack)
  )]
  async fn resolve(
    self,
    args: &crate::api::Args,
  ) -> anyhow::Result<Log> {
    let stack = self.stack;
    let dir = stack_quadlet_dir(&stack);
    let units = installed_units(&stack).await?;
    let mut lines = Vec::new();
    let runnable = units
      .iter()
      .filter(|u| u.unit.kind.is_runnable())
      .map(|u| u.unit.service.clone())
      .collect::<Vec<_>>();
    if !runnable.is_empty() {
      let log = systemd::systemctl("Stop Stack", "stop", &runnable).await;
      lines.push(format!("stop {}: {}", runnable.join(" "), if log.success { "ok" } else { "failed" }));
      if !log.success {
        lines.push(log.stderr);
      }
    }
    if dir.is_dir() {
      tokio::fs::remove_dir_all(&dir)
        .await
        .with_context(|| format!("Failed to remove {}", dir.display()))?;
      lines.push(format!("removed {}", dir.display()));
    } else {
      lines.push(format!("{} was not present", dir.display()));
    }
    let reload = systemd::daemon_reload().await;
    lines.push(format!("daemon-reload: {}", if reload.success { "ok" } else { "failed" }));

    // Secrets: those recorded in state, plus any with the stack prefix.
    let prefix = format!("{}_", secret_prefix(&stack));
    let mut names = state::load(&stack)
      .await
      .ok()
      .flatten()
      .map(|s| s.secrets.keys().cloned().collect::<Vec<_>>())
      .unwrap_or_default();
    if let Ok(existing) = podman::secret_names().await {
      names.extend(existing.into_iter().filter(|n| n.starts_with(&prefix)));
    }
    names.sort();
    names.dedup();
    for name in names {
      let log = podman::secret_rm(&name).await;
      lines.push(format!("secret rm {name}: {}", if log.success { "ok" } else { "failed" }));
    }
    state::remove(&stack).await;

    let mut log = Log::simple("Destroy Stack", lines.join("\n"));
    log.success = reload.success;
    Ok(log)
  }
}

//

async fn stack_container_names(
  stack: &str,
  services: &[String],
) -> anyhow::Result<Vec<String>> {
  let units = installed_units(stack).await?;
  let selected = select_units(&units, services)?;
  let names = selected
    .into_iter()
    .filter(|u| u.unit.kind == QuadletKind::Container)
    .map(|u| u.unit.name.clone())
    .collect::<Vec<_>>();
  if names.is_empty() {
    return Err(anyhow!("No container units matched in stack '{stack}'"));
  }
  Ok(names)
}

impl Resolve<crate::api::Args> for GetStackLog {
  async fn resolve(
    self,
    _: &crate::api::Args,
  ) -> anyhow::Result<Log> {
    let GetStackLog {
      stack,
      services,
      tail,
      timestamps,
    } = self;
    let names = stack_container_names(&stack, &services).await?;
    let timestamps = if timestamps { " --timestamps" } else { "" };
    let command = names
      .iter()
      .map(|n| {
        format!(
          "echo '==> {n} <=='; podman logs --tail {tail}{timestamps} -- {} 2>&1",
          escape(n.into())
        )
      })
      .collect::<Vec<_>>()
      .join("; ");
    Ok(
      run_komodo_shell_command(
        "Get Stack Log",
        command,
        CommandOptions::default().timeout(Duration::from_secs(15)),
      )
      .await,
    )
  }
}

impl Resolve<crate::api::Args> for GetStackLogSearch {
  async fn resolve(
    self,
    _: &crate::api::Args,
  ) -> anyhow::Result<Log> {
    let GetStackLogSearch {
      stack,
      services,
      terms,
      combinator,
      invert,
      timestamps,
    } = self;
    let names = stack_container_names(&stack, &services).await?;
    let timestamps = if timestamps { " --timestamps" } else { "" };
    let grep = format_log_grep(&terms, combinator, invert);
    let command = names
      .iter()
      .map(|n| {
        format!(
          "echo '==> {n} <=='; podman logs --tail 5000{timestamps} -- {} 2>&1 | {grep}",
          escape(n.into())
        )
      })
      .collect::<Vec<_>>()
      .join("; ");
    Ok(
      run_komodo_shell_command(
        "Search Stack Log",
        command,
        CommandOptions::default().timeout(Duration::from_secs(15)),
      )
      .await,
    )
  }
}
