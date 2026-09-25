//! `.containerignore` / `.dockerignore` matching, so a change to a file
//! the build never sees does not rebuild the image.
//!
//! Follows moby's patternmatcher, which buildah uses:
//! - `#` in column 1 is a comment; lines are trimmed; blank lines skipped.
//! - `!` makes an exception. The last matching pattern wins.
//! - Patterns are cleaned (`.` / `..` resolved) and a leading `/` dropped.
//! - `*` and `?` do not cross `/`; `**` matches any number of directories.
//! - A pattern matching a parent directory matches everything under it.

use regex::Regex;

#[derive(Debug, Default)]
pub struct IgnoreRules {
  patterns: Vec<Pattern>,
}

#[derive(Debug)]
struct Pattern {
  exclusion: bool,
  regex: Regex,
}

impl IgnoreRules {
  pub fn parse(contents: &str) -> IgnoreRules {
    let contents =
      contents.strip_prefix('\u{feff}').unwrap_or(contents);
    let patterns = contents
      .lines()
      .filter(|line| !line.starts_with('#'))
      .filter_map(|line| {
        let line = line.trim();
        let (exclusion, pattern) = match line.strip_prefix('!') {
          Some(rest) => (true, rest.trim()),
          None => (false, line),
        };
        let pattern = clean(pattern)?;
        let regex = Regex::new(&to_regex(&pattern)).ok()?;
        Some(Pattern { exclusion, regex })
      })
      .collect();
    IgnoreRules { patterns }
  }

  /// Whether `path` (relative to the build context, `/` separated)
  /// is left out of the build context.
  pub fn excludes(&self, path: &str) -> bool {
    let parents = path
      .rsplit_once('/')
      .map(|(dir, _)| dir.split('/').collect::<Vec<_>>())
      .unwrap_or_default();
    let mut matched = false;
    for pattern in &self.patterns {
      // An exclusion only matters once matched, an inclusion before.
      if pattern.exclusion != matched {
        continue;
      }
      let hit = pattern.regex.is_match(path)
        || (1..=parents.len())
          .any(|i| pattern.regex.is_match(&parents[..i].join("/")));
      if hit {
        matched = !pattern.exclusion;
      }
    }
    matched
  }
}

/// filepath.Clean, then drop a leading `/`. None for an empty pattern.
fn clean(pattern: &str) -> Option<String> {
  if pattern.is_empty() {
    return None;
  }
  let mut parts = Vec::new();
  for part in pattern.split('/') {
    match part {
      "" | "." => {}
      ".." => {
        parts.pop();
      }
      part => parts.push(part),
    }
  }
  if parts.is_empty() {
    // `/` or `.`: the whole context.
    return Some(String::from("."));
  }
  Some(parts.join("/"))
}

fn to_regex(pattern: &str) -> String {
  let mut out = String::from("^");
  let mut chars = pattern.chars().peekable();
  while let Some(ch) = chars.next() {
    match ch {
      '*' if chars.peek() == Some(&'*') => {
        chars.next();
        // `**/` reads as `**`.
        if chars.peek() == Some(&'/') {
          chars.next();
        }
        if chars.peek().is_none() {
          out.push_str(".*");
        } else {
          out.push_str("(.*/)?");
        }
      }
      '*' => out.push_str("[^/]*"),
      '?' => out.push_str("[^/]"),
      '\\' => {
        if let Some(next) = chars.next() {
          out.push_str(&regex::escape(&next.to_string()));
        }
      }
      '[' => {
        // Character class, passed through to the end bracket.
        out.push('[');
        for c in chars.by_ref() {
          if c == '\\' {
            out.push_str("\\\\");
            continue;
          }
          out.push(c);
          if c == ']' {
            break;
          }
        }
      }
      ch => out.push_str(&regex::escape(&ch.to_string())),
    }
  }
  out.push('$');
  out
}

#[cfg(test)]
mod tests {
  use super::*;

  fn excluded(rules: &str, paths: &[&str]) -> Vec<String> {
    let rules = IgnoreRules::parse(rules);
    paths
      .iter()
      .filter(|p| rules.excludes(p))
      .map(|p| p.to_string())
      .collect()
  }

  #[test]
  fn no_rules_exclude_nothing() {
    assert!(excluded("", &["a", "b/c"]).is_empty());
    assert!(
      excluded("# only a comment\n\n", &["a", "b/c"]).is_empty()
    );
  }

  /// Recorded from podman 5.7: `FROM scratch` + `COPY . /ctx/` with each
  /// `.containerignore` below over `PODMAN_FILES`, listing what landed in
  /// the image. The rest is what podman excluded.
  const PODMAN_FILES: &[&str] = &[
    "a.md",
    "README.md",
    "README-secret.md",
    "docs/x.md",
    "docs/keep.txt",
    "docs/y.txt",
    "a.go",
    "x/y/b.go",
    "x/y/b.rs",
    "node_modules/p/i.js",
    "include/rootless.c",
    "a/b/c.c",
    "out/output.log",
    "output",
    "src/main.go",
    "srcs/x",
    "tempa",
    "tempab",
    "d/temp1",
    "d/e/temp2",
    "b",
    "c",
    "a.txt",
    "d.txt",
    "spaced.txt",
  ];

  #[test]
  fn matches_podman() {
    let cases: &[(&str, &[&str])] = &[
      (
        "*.md\n!README*.md\nREADME-secret.md",
        &["a.md", "README-secret.md"],
      ),
      (
        "**/*.go\n/node_modules",
        &["a.go", "x/y/b.go", "node_modules/p/i.js", "src/main.go"],
      ),
      (
        "*\n!x",
        &[
          "a.md",
          "README.md",
          "README-secret.md",
          "docs/x.md",
          "docs/keep.txt",
          "docs/y.txt",
          "a.go",
          "node_modules/p/i.js",
          "include/rootless.c",
          "a/b/c.c",
          "out/output.log",
          "output",
          "src/main.go",
          "srcs/x",
          "tempa",
          "tempab",
          "d/temp1",
          "d/e/temp2",
          "b",
          "c",
          "a.txt",
          "d.txt",
          "spaced.txt",
        ],
      ),
      ("x/**", &["x/y/b.go", "x/y/b.rs"]),
      (
        "*/*.c\n**/output*\nsrc",
        &[
          "include/rootless.c",
          "out/output.log",
          "output",
          "src/main.go",
        ],
      ),
      ("docs/\n!docs/keep.txt", &["docs/x.md", "docs/y.txt"]),
      ("temp?\n*/temp*", &["tempa", "d/temp1"]),
      ("a/../b\n./c", &["b", "c"]),
      ("[a-c].txt", &["a.txt"]),
      ("  # not comment\n#comment\n  spaced.txt  ", &["spaced.txt"]),
    ];
    for (rules, podman_excluded) in cases {
      assert_eq!(
        excluded(rules, PODMAN_FILES),
        podman_excluded.to_vec(),
        "rules: {rules:?}"
      );
    }
  }
}
