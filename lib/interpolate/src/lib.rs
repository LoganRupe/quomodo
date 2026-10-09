use std::collections::{HashMap, HashSet};

use anyhow::Context;
use komodo_client::entities::update::Log;

/// Replaces `[[NAME]]` in a string with values from a map, and keeps
/// `(value, [[NAME]])` pairs so callers can scrub the values from logs.
/// Core only uses it for an Action's own API key pair.
pub struct Interpolator<'a> {
  secrets: &'a HashMap<String, String>,
  pub secret_replacers: HashSet<(String, String)>,
}

impl<'a> Interpolator<'a> {
  pub fn new(
    secrets: &'a HashMap<String, String>,
  ) -> Interpolator<'a> {
    Interpolator {
      secrets,
      secret_replacers: Default::default(),
    }
  }

  pub fn interpolate_string(
    &mut self,
    target: &mut String,
  ) -> anyhow::Result<&mut Self> {
    if target.is_empty() {
      return Ok(self);
    }

    let (res, more_replacers) = svi::interpolate_variables(
      target,
      self.secrets,
      svi::Interpolator::DoubleBrackets,
      false,
    )
    .with_context(|| {
      format!("failed to interpolate secrets into target '{target}'",)
    })?;
    self.secret_replacers.extend(more_replacers);

    *target = res;

    Ok(self)
  }

  pub fn push_logs(&self, logs: &mut Vec<Log>) {
    // Only show names of interpolated secrets
    if !self.secret_replacers.is_empty() {
      logs.push(
        Log::simple("Interpolate Secrets",
        self.secret_replacers
          .iter()
          .map(|(_, variable)| format!("<span class=\"text-muted-foreground\">replaced:</span> {variable}"))
          .collect::<Vec<_>>()
          .join("\n"),)
      );
    }
  }
}
