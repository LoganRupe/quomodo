use std::{
  path::{Path, PathBuf},
  sync::OnceLock,
  time::Duration,
};

use anyhow::anyhow;

use command::{CommandOptions, run_komodo_standard_command};
use formatting::format_serror;
use komodo_client::entities::{
  RepoExecutionArgs, RepoExecutionResponse, all_logs_success,
  komodo_timestamp, update::Log,
};
use mogh_cache::TimeoutCache;

use crate::{check_installed, get_commit_hash_log};

/// The last pull of each path: when it started, and its result. The
/// error is kept as its message, so waiting callers can share it.
type PullResult = (i64, Result<RepoExecutionResponse, String>);

fn pull_cache() -> &'static TimeoutCache<PathBuf, PullResult> {
  static PULL_CACHE: OnceLock<TimeoutCache<PathBuf, PullResult>> =
    OnceLock::new();
  // Only callers queued behind a pull use its result, so it need not
  // outlive the longest pull by much.
  PULL_CACHE
    .get_or_init(|| TimeoutCache::new(Duration::from_secs(60 * 60)))
}

/// This will pull in a way that handles edge cases
/// from possible state of the repo. For example, the user
/// can change branch after clone, or even the remote.
pub async fn pull<T>(
  clone_args: T,
  root_repo_dir: &Path,
  access_token: Option<String>,
) -> anyhow::Result<RepoExecutionResponse>
where
  T: Into<RepoExecutionArgs> + std::fmt::Debug,
{
  check_installed().await?;

  let args: RepoExecutionArgs = clone_args.into();
  let repo_url = args.remote_url(access_token.as_deref())?;

  let mut res = RepoExecutionResponse {
    path: args.path(root_repo_dir),
    logs: Vec::new(),
    commit_hash: None,
    commit_message: None,
  };

  // When this caller asked. A cached pull is only good enough if it
  // started after this: one that started earlier may predate the commit
  // the caller is pulling for (a push webhook arriving while another
  // pull of the same repo is running, or just after one finished).
  let requested = komodo_timestamp();

  // Acquire the path lock
  let lock = pull_cache().get_lock(res.path.clone()).await;

  // Lock the path lock, prevents simultaneous pulls by
  // ensuring simultaneous pulls will wait for first to finish
  // and checking cached results.
  let mut locked = lock.lock().await;

  // Callers that queued behind a pull which started after they asked
  // share its result instead of pulling again.
  if let Some(Ok((last_started, last_res))) = locked.fresh_res()
    && last_started > requested
  {
    return last_res.map_err(|e| anyhow!(e));
  }
  let started = komodo_timestamp();

  let res = async {
    // Check for '.git' path to see if the folder is initialized as a git repo
    let dot_git_path = res.path.join(".git");
    if !dot_git_path.exists() {
      crate::init::init_folder_as_repo(
        &res.path,
        &args,
        access_token.as_deref(),
        &mut res.logs,
      )
      .await;
      if !all_logs_success(&res.logs) {
        return Ok(res);
      }
    }

    // Set remote url
    let mut set_remote = run_komodo_standard_command(
      "Set Git Remote",
      format!("git remote set-url origin {repo_url}"),
      CommandOptions::default().path(res.path.as_ref()),
    )
    .await;
    // Sanitize the output
    if let Some(token) = access_token {
      set_remote.command =
        set_remote.command.replace(&token, "<TOKEN>");
      set_remote.stdout =
        set_remote.stdout.replace(&token, "<TOKEN>");
      set_remote.stderr =
        set_remote.stderr.replace(&token, "<TOKEN>");
    }
    res.logs.push(set_remote);
    if !all_logs_success(&res.logs) {
      return Ok(res);
    }

    // First fetch remote branches before checkout
    let fetch = run_komodo_standard_command(
      "Git Fetch",
      "git fetch --all --prune",
      CommandOptions::default().path(res.path.as_ref()),
    )
    .await;
    if !fetch.success {
      res.logs.push(fetch);
      return Ok(res);
    }

    let checkout = run_komodo_standard_command(
      "Checkout branch",
      format!("git checkout -f {}", args.branch),
      CommandOptions::default().path(res.path.as_ref()),
    )
    .await;
    res.logs.push(checkout);
    if !all_logs_success(&res.logs) {
      return Ok(res);
    }

    let pull_log = run_komodo_standard_command(
      "Git pull",
      format!("git pull --rebase --force origin {}", args.branch),
      CommandOptions::default().path(res.path.as_ref()),
    )
    .await;
    res.logs.push(pull_log);
    if !all_logs_success(&res.logs) {
      return Ok(res);
    }

    if let Some(commit) = args.commit {
      let reset_log = run_komodo_standard_command(
        "Set commit",
        format!("git reset --hard {commit}"),
        CommandOptions::default().path(res.path.as_ref()),
      )
      .await;
      res.logs.push(reset_log);
      if !all_logs_success(&res.logs) {
        return Ok(res);
      }
    }

    match get_commit_hash_log(&res.path).await {
      Ok((log, hash, message)) => {
        res.logs.push(log);
        res.commit_hash = Some(hash);
        res.commit_message = Some(message);
      }
      Err(e) => {
        res.logs.push(Log::simple(
          "Latest Commit",
          format_serror(
            &e.context("Failed to get latest commit").into(),
          ),
        ));
      }
    };

    anyhow::Ok(res)
  }
  .await;

  // Set the cache with results, stamped with when this pull started.
  // Calls that were waiting on the lock and asked before then use it.
  let shared = match &res {
    Ok(res) => Ok(res.clone()),
    Err(e) => Err(format!("{e:#}")),
  };
  locked.set(&Ok((started, shared)));

  res
}
