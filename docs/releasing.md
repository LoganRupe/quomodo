# Releasing

Releases are tag-triggered only. Pushing a `v*` tag builds:

- the x86_64 agent binary, attached to the release as `quomodo-agent-x86_64`
  with its `.sha256`;
- the Core image, pushed as `<image>:<tag>` and `<image>:latest`.

Both workflows set the crate versions from the tag. Bump
`[workspace.package] version` in `Cargo.toml` to match anyway, so a local
build reports the right version to Core.

## GitHub

[`.github/workflows/release.yml`](../.github/workflows/release.yml) uses the
automatic `GITHUB_TOKEN` for both jobs. The image goes to
`ghcr.io/<owner>/<repo>`, lowercased. To push somewhere else on ghcr.io, set
the repository variable `QUOMODO_IMAGE`.

A new ghcr.io package is private. Make it public under the package's
settings after the first release, or hosts need a registry login to pull it.

## Forgejo

[`.forgejo/workflows/release.yml`](../.forgejo/workflows/release.yml) is the
same release for a copy hosted on Forgejo. Its automatic token cannot push to
the registry, so it needs:

| Setting | Kind | Value |
| --- | --- | --- |
| `CI_PACKAGE_USERNAME` | variable | A user with package write on the image's owner |
| `CI_PACKAGE_TOKEN` | secret | That user's token, with `write:package` |
| `QUOMODO_IMAGE` | variable, optional | The image, eg `forgejo.example.com/org/quomodo`. Default: `<forgejo host>/<owner>/<repo>` |

The runner needs the `docker` label and a reachable docker daemon for the
image build. The workflow expects docker-in-docker on the runner's default
gateway at port 2375; change the `DOCKER_HOST` step if yours differs.

## Rolling out

- Core: with `AutoUpdate=registry` and the `:latest` tag, run
  `podman auto-update` on the Core host, or wait for its timer.
- Agents: install the new binary on each host and restart `quomodo-agent`.
  An agent upgrade redeploys nothing by itself. Run `DeployStack` on a stack
  to re-apply it with the new agent.
