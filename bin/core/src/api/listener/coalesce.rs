use std::{
  collections::HashMap,
  sync::{Mutex, MutexGuard},
};

/// Coalesces webhook runs per key: at most one run in progress
/// and at most one pending.
///
/// Webhook runs sync from the branch HEAD, so a run that starts
/// after the latest push covers every push before it. Queueing one
/// run per webhook only repeats work; dropping the trailing run
/// would miss a push that landed after the running one pulled.
#[derive(Default)]
pub struct Coalescer {
  /// Present while a run is in progress. The value is whether
  /// another run is pending.
  runs: Mutex<HashMap<String, bool>>,
}

impl Coalescer {
  /// Runs `run` now if nothing is running for `key`, then once more
  /// for each batch of triggers that arrived while it ran. If a run
  /// is already in progress, marks `key` pending and returns `Ok`
  /// immediately.
  ///
  /// Returns the result of the last run. Errors from earlier runs
  /// are logged.
  pub async fn run<F, Fut>(
    &self,
    key: String,
    run: F,
  ) -> anyhow::Result<()>
  where
    F: Fn() -> Fut,
    Fut: Future<Output = anyhow::Result<()>>,
  {
    {
      let mut runs = self.runs();
      if let Some(pending) = runs.get_mut(&key) {
        *pending = true;
        debug!(
          "Webhook run already in progress for {key}, marked pending"
        );
        return Ok(());
      }
      runs.insert(key.clone(), false);
    }
    let _guard = ClearOnPanic {
      coalescer: self,
      key: &key,
    };
    loop {
      let res = run().await;
      let mut runs = self.runs();
      if runs.get_mut(&key).map(std::mem::take) == Some(true) {
        drop(runs);
        if let Err(e) = res {
          warn!("Webhook run for {key} failed | {e:#}");
        }
        continue;
      }
      runs.remove(&key);
      return res;
    }
  }

  fn runs(&self) -> MutexGuard<'_, HashMap<String, bool>> {
    // Never held across an await or user code, so poisoning
    // carries no broken state.
    self.runs.lock().unwrap_or_else(|e| e.into_inner())
  }
}

/// A run that panics must not leave its key marked in progress,
/// or every later webhook for it would only mark it pending.
struct ClearOnPanic<'a> {
  coalescer: &'a Coalescer,
  key: &'a str,
}

impl Drop for ClearOnPanic<'_> {
  fn drop(&mut self) {
    if std::thread::panicking() {
      self.coalescer.runs().remove(self.key);
    }
  }
}

#[cfg(test)]
mod tests {
  use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
  };

  use tokio::sync::Notify;

  use super::*;

  /// A run that counts itself and waits for `release` to finish.
  struct Gate {
    runs: AtomicUsize,
    started: Notify,
    release: Notify,
  }

  impl Gate {
    fn new() -> Arc<Gate> {
      Arc::new(Gate {
        runs: AtomicUsize::new(0),
        started: Notify::new(),
        release: Notify::new(),
      })
    }

    async fn run(&self) -> anyhow::Result<()> {
      self.runs.fetch_add(1, Ordering::SeqCst);
      self.started.notify_one();
      self.release.notified().await;
      Ok(())
    }

    fn runs(&self) -> usize {
      self.runs.load(Ordering::SeqCst)
    }
  }

  fn trigger(
    coalescer: &Arc<Coalescer>,
    key: &str,
    gate: &Arc<Gate>,
  ) -> tokio::task::JoinHandle<anyhow::Result<()>> {
    let coalescer = coalescer.clone();
    let key = key.to_string();
    let gate = gate.clone();
    tokio::spawn(
      async move { coalescer.run(key, || gate.run()).await },
    )
  }

  #[tokio::test]
  async fn idle_trigger_runs_immediately() {
    let coalescer = Arc::new(Coalescer::default());
    let gate = Gate::new();
    let first = trigger(&coalescer, "a", &gate);
    gate.started.notified().await;
    assert_eq!(gate.runs(), 1);
    gate.release.notify_one();
    first.await.unwrap().unwrap();
    assert!(coalescer.runs().is_empty());
  }

  #[tokio::test]
  async fn triggers_while_busy_give_one_follow_up() {
    let coalescer = Arc::new(Coalescer::default());
    let gate = Gate::new();
    let first = trigger(&coalescer, "a", &gate);
    gate.started.notified().await;

    for _ in 0..10 {
      // Each returns at once, having only marked "a" pending.
      trigger(&coalescer, "a", &gate).await.unwrap().unwrap();
    }
    assert_eq!(gate.runs(), 1);

    gate.release.notify_one();
    gate.started.notified().await;
    assert_eq!(gate.runs(), 2);

    // A trigger during the follow-up gives exactly one more.
    trigger(&coalescer, "a", &gate).await.unwrap().unwrap();
    trigger(&coalescer, "a", &gate).await.unwrap().unwrap();
    gate.release.notify_one();
    gate.started.notified().await;
    assert_eq!(gate.runs(), 3);

    gate.release.notify_one();
    first.await.unwrap().unwrap();
    assert_eq!(gate.runs(), 3);
    assert!(coalescer.runs().is_empty());
  }

  #[tokio::test]
  async fn keys_do_not_block_each_other() {
    let coalescer = Arc::new(Coalescer::default());
    let a = Gate::new();
    let b = Gate::new();
    let run_a = trigger(&coalescer, "a", &a);
    a.started.notified().await;
    let run_b = trigger(&coalescer, "b", &b);
    b.started.notified().await;
    assert_eq!((a.runs(), b.runs()), (1, 1));

    b.release.notify_one();
    run_b.await.unwrap().unwrap();
    a.release.notify_one();
    run_a.await.unwrap().unwrap();
    assert_eq!((a.runs(), b.runs()), (1, 1));
  }

  #[tokio::test]
  async fn panicking_run_clears_key() {
    let coalescer = Arc::new(Coalescer::default());
    let c = coalescer.clone();
    let res = tokio::spawn(async move {
      c.run("a".into(), || async { panic!("boom") }).await
    })
    .await;
    assert!(res.is_err());
    assert!(coalescer.runs().is_empty());
  }

  #[tokio::test]
  async fn error_returns_from_last_run() {
    let coalescer = Coalescer::default();
    let res = coalescer
      .run("a".into(), || async { anyhow::bail!("failed") })
      .await;
    assert!(res.is_err());
    assert!(coalescer.runs().is_empty());
  }
}
