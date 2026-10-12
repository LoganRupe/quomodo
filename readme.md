<picture>
  <source media="(prefers-color-scheme: dark)" srcset="ui/public/quomodo-logo-dark.svg">
  <img alt="Quomodo" src="ui/public/quomodo-logo-light.svg" width="360">
</picture>

# Quomodo

A hard fork of [Komodo](https://github.com/moghtech/komodo) v2.3.3 for
podman: stacks are directories of quadlet files, agents only dial out, and
stack secrets come from a vault at deploy time instead of the database.
GPL-3.0-or-later, same as upstream. Quomodo is an independent project, not
affiliated with or endorsed by Komodo or moghtech.

<p align="center">
  <a href="https://github.com/user-attachments/assets/b931a219-2a44-47d1-b46d-21f50d0ab74f">
    <img src="docs/images/demo.gif" alt="A one-line image bump in gitea.container is pushed; the push webhook runs DeployStackIfChanged and only gitea.service restarts while the other containers keep their uptime" width="960">
  </a>
  <br>
  <sub>One line of <code>gitea.container</code> changes and gets pushed. The webhook deploys and only <code>gitea.service</code> restarts. Click for the video (0:22).</sub>
</p>

It runs one home lab today. It is young (0.x) and the config
format may still change between releases.

| Servers | A Stack | A deploy |
| --- | --- | --- |
| [![Four servers, all Ok](docs/images/servers.png)](docs/images/servers.png) | [![The gitea Stack and its units](docs/images/stack.png)](docs/images/stack.png) | [![A webhook deploy that restarted only gitea.service](docs/images/deploy-result.png)](docs/images/deploy-result.png) |

The pictures come from `scripts/demo.sh`, a throwaway lab of four podman
hosts in docker.

## What it is

- **Core** (`quomodo`): the API and UI. Runs as a root quadlet pod with mongo.
- **Agent** (`quomodo-agent`): runs as root on every host and dials out to
  Core over `wss://` only. No inbound listener, no passkeys, no terminals.
- **Stacks**: a directory in git holding raw `.pod`, `.container`, `.network`,
  `.volume`, `.build`, `.image` or `.kube` files plus config files. Deploying
  copies the directory to `/etc/containers/systemd/<stack>/`, reloads systemd
  and restarts the units whose files changed. A changed file restarts the
  units that bind-mount it, and a change to a file a `.build` context
  sends (minus its `.containerignore` / `.dockerignore`) rebuilds the image
  and restarts its containers. Binary files and files
  over 1 MiB are copied as is, never interpolated.
- **User namespaces**: every `.pod`, and every `.container` outside a pod,
  runs in its own user namespace on a fixed 65536-id slice of the host's
  `containers` subordinate range. The agent picks the slice the first time
  it sees the unit, records it in `/var/lib/quomodo/idmap.json`, and writes
  it as `UIDMap=` / `GIDMap=`, so a redeploy or a reboot never moves a
  container's root to another host uid (podman's `UserNS=auto` does, and
  files in volumes turn into `nobody`). `UserNS=auto:uidmapping=...` keeps
  its carve-outs; `UserNS=host` and friends are left alone; a pod member
  shares the pod's namespace.
- **Secrets**: `[[NAME]]` in a stack file is always a secret, read from the
  secret store named by the Stack's `secrets_collection`. Core fetches the
  values at deploy time, sends them to the agent over the key-authenticated
  websocket, and the agent creates podman secrets. A secret may only be
  referenced on a `Secret=` line; a config file holding a secret reference is
  mounted with `Secret=file:<path>` and never written under `/etc`. Nothing
  with a resolved secret touches mongo. First provider: Vaultwarden or
  Bitwarden, over the Bitwarden vault protocol.

Resources: Server, Stack, Repo, ResourceSync, Procedure, Action, Alerter.
Auth: local, OIDC, API keys. Webhooks: GitHub (Forgejo and Gitea send the
same format) and GitLab. Alerters send alerts to a Custom webhook, Slack,
Discord, ntfy or Pushover, with their tokens kept in the secret store.

Gone from upstream: Deployment, Build, Builder, Swarm, Variables,
Google and GitHub OAuth, terminals and container exec, docker and compose,
image update polling. The Rust module names inside still say `komodo` and `periphery`,
and config environment variables still start with `KOMODO_` / `PERIPHERY_`.
There is no migration from Komodo: stacks have to be rewritten as quadlets.

## Docs

- [Install](docs/install.md): Core, agents, the first stack, webhooks
- [Stacks](docs/stacks.md): files, secrets, what a deploy
  restarts, user namespaces
- [Secrets](docs/secrets.md): providers and the vault service account
- [Alerters](docs/alerters.md): where alerts go, and keeping webhook tokens
  out of the database
- [Releasing](docs/releasing.md): tags, images, binaries
- [Security](SECURITY.md): trust model and reporting
- [Contributing](CONTRIBUTING.md)
- [Code of Conduct](CODE_OF_CONDUCT.md)

## Layout

- `bin/core`, `bin/periphery` (the agent), `bin/cli` (`km`)
- `client/core/rs` types, `client/core/ts` the TypeScript client the UI uses
- `lib/quadlet` unit parsing and the interpolation rules shared by core and agent
- `config/core.config.toml`, `config/agent.config.toml` documented defaults
- `example/quomodo-core` the core pod as quadlet files
- `example/stack` a sample stack directory
- `scripts/e2e.sh` the end-to-end test

## Build

```
cargo build --release -p komodo_core -p komodo_periphery -p komodo_cli
cd client/core/ts && yarn && yarn build && yarn link
cd ui && yarn link komodo_client && yarn && yarn build
```

Tagged releases publish the x86_64 agent binary as a release asset and the
Core image to `ghcr.io/loganrupe/quomodo` ([releasing](docs/releasing.md)).

## Upstream

Pinned at v2.3.3. Security fixes are cherry-picked by hand; there are no
tracking merges.
