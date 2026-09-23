# Quomodo

A hard fork of [Komodo](https://github.com/moghtech/komodo) v2.3.3 stripped
to what one home lab uses, with podman quadlets as the deploy unit and no secrets
at rest. GPL-3.0-or-later, same as upstream.

## What it is

- **Core** (`quomodo`): the API and UI. Runs as a root quadlet pod with mongo.
- **Agent** (`quomodo-agent`): runs as root on every host, dials out to Core
  over the tailnet. No inbound listener, no passkeys, no terminals.
- **Stacks**: a directory in git holding raw `.pod`, `.container`, `.network`,
  `.volume`, `.build`, `.image` or `.kube` files plus config files. Deploying
  copies the directory to `/etc/containers/systemd/<stack>/`, reloads systemd
  and restarts the units whose files changed. Every `.container` gets
  `UserNS=auto`.
- **Secrets**: `[[NAME]]` resolves from non-secret Variables first, then from
  the secret store named by the Stack's `secrets_collection`. Core fetches the
  values at deploy time, sends them to the agent over the key-authenticated
  websocket, and the agent creates podman secrets. A secret may only be
  referenced on a `Secret=` line; a config file holding a secret reference is
  mounted with `Secret=file:<path>` and never written under `/etc`. Nothing
  with a resolved secret touches mongo. First provider: Vaultwarden over the
  Bitwarden vault protocol.

Resources: Server, Stack, Repo, ResourceSync, Procedure, Action, Variables.
Auth: local, OIDC, API keys. Webhooks: the Forgejo (GitHub-compatible)
listener. Alerts are recorded and shown in the UI; nothing sends them.

Gone from upstream: Deployment, Build, Builder, Swarm, Alerter, Google and
GitHub OAuth, terminals and container exec, docker and compose, image update
polling. The Rust module names inside still say `komodo` and `periphery`.

## Layout

- `bin/core`, `bin/periphery` (the agent), `bin/cli` (`km`)
- `client/core/rs` types, `client/core/ts` the TypeScript client the UI uses
- `lib/quadlet` unit parsing and the interpolation rules shared by core and agent
- `config/core.config.toml`, `config/agent.config.toml` documented defaults
- `example/quomodo-core` the core pod as quadlet files
- `example/stack` a sample stack directory

## Build

```
cargo build --release -p komodo_core -p komodo_periphery -p komodo_cli
cd client/core/ts && yarn && yarn build && yarn link
cd ui && yarn link komodo_client && yarn && yarn build
```

Releases are tagged. Forgejo Actions builds the x86_64 agent binary as a
release asset and pushes the core image to the Forgejo registry
(`.forgejo/workflows/release.yml`).

## Agent install

An Ansible role installs podman, fetches `quomodo-agent` by version
from the Forgejo release, writes `/etc/quomodo/config.toml`, the key pair and
`core.pub` under `/etc/quomodo/keys/`, and a root systemd unit. See
`scripts/readme.md` for what the unit needs.

## Upstream

Pinned at v2.3.3. Security fixes are cherry-picked by hand; there are no
tracking merges.
