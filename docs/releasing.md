# Releasing

Releases are tag-triggered only. Pushing a `v*` tag builds:

- the agent binaries, attached to the release as `quomodo-agent-x86_64` and
  `quomodo-agent-aarch64`, each with its `.sha256`;
- the Core image for linux/amd64 and linux/arm64, pushed as one manifest list
  to `<image>:<tag>` and `<image>:latest`;
- on GitHub, release notes generated from the merged pull requests.

To release:

1. Bump `[workspace.package] version` in `Cargo.toml` and run `cargo check`
   so `Cargo.lock` follows. Commit that on `main`.
2. Tag that commit `v<version>` and push the tag.

The workflow starts by comparing the tag with the `Cargo.toml` version and
builds nothing if they differ, so a binary never reports a version its source
doesn't say. Pre-1.0, a breaking change bumps the minor version.

The repository also carries upstream's `v2.3.3` tag. Push tags by name, not
with `--tags`: on the forge it would sit beside Quomodo's `v0.x` tags and
sort above them.

## Dry run

To test a change to the release workflow without releasing, run it by hand
on the branch:

```sh
gh workflow run release.yml --ref <branch>
```

It builds both architectures and runs the same checks, then stops short of
publishing. The agent binaries stay as workflow artifacts and no release is
created. The image goes to `<image>-dryrun:run-<run id>`, a separate package,
and the real tags are untouched. Delete old versions of the `-dryrun` package
from its settings page when they pile up.

## GitHub

[`.github/workflows/release.yml`](../.github/workflows/release.yml) uses the
automatic `GITHUB_TOKEN` for every job. Each architecture builds on its own
native runner (`ubuntu-24.04` and `ubuntu-24.04-arm`), with no emulation. The
agent builds in a Debian bookworm container, so the binaries need glibc 2.36
or newer. The job fails if a binary needs a newer glibc than that. The image
goes to `ghcr.io/<owner>/<repo>`, lowercased. To push somewhere else on ghcr.io, set
the repository variable `QUOMODO_IMAGE`.

A new ghcr.io package is private. Make it public under the package's
settings after the first release, or hosts need a registry login to pull it.

## Rolling out

- Core: with `AutoUpdate=registry` and the `:latest` tag, run
  `podman auto-update` on the Core host, or wait for its timer.
- Agents: install the new binary on each host and restart `quomodo-agent`.
  An agent upgrade redeploys nothing by itself. Run `DeployStack` on a stack
  to re-apply it with the new agent.
