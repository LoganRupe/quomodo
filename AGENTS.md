# AGENTS.md

Guidance for AI coding agents (Claude Code, Codex, Copilot, Cursor) working in
this repo. Human contributors: the same rules apply.

## What this is

Quomodo is a hard fork of [Komodo](https://github.com/moghtech/komodo) v2.3.3
that deploys podman quadlets. Core (API, UI, mongo) runs as a pod; an agent
runs on each host and dials out to Core over `wss://`. As root it deploys
rootful quadlets; as any other user, that user's rootless ones
(`bin/periphery/src/host.rs`). Stacks are git directories of quadlet files.
Stack secrets come from a vault at deploy time.

The readme's first line credits Komodo as the starting point. Komodo's author
asked for that attribution as the condition for the fork: never remove or bury
it.

| Path | What it is |
| --- | --- |
| `bin/core` | Core: API, UI server, monitoring, alert sending |
| `bin/periphery` | The agent (`quomodo-agent`) |
| `bin/cli` | `km` |
| `lib/quadlet` | Unit parsing, secret interpolation rules, user namespace slices; shared by Core and agent |
| `lib/transport` | Core and agent websocket, Noise login |
| `client/core/rs` | API and entity types (Rust); source of the TS types |
| `client/core/ts` | TypeScript client the UI uses |
| `ui` | React UI |
| `scripts/e2e.sh` | End-to-end test |
| `docs/` | User docs; update them with any user-visible change |

## Invariants

Breaking one of these is a bug, whatever the change was for.

- **No secrets at rest.** Secret values never go into mongo, the repo clone or
  a file under `/etc/containers/systemd`. Core resolves them from the vault at
  deploy time and the agent turns them into podman secrets.
- **`[[NAME]]` is always a secret.** There are no Variables. In a unit file a
  secret may only appear on a `Secret=` line, and a config file holding one
  must be mounted with `Secret=file:`. The rules are in
  `lib/quadlet/src/prepare.rs`.
- **The agent only connects over `wss://` and always verifies Core's
  certificate.** The Noise handshake authenticates but does not encrypt, and
  the session carries secret values. Do not add `ws://`, an "insecure skip
  verify" switch, or an inbound listener on the agent.
- **Fixed user namespace slices.** The agent gives each pod and standalone
  container a fixed 65536-id slice, recorded in `idmap.json`. A slice is never
  reused or moved. A rootless agent takes them from its user's own
  subordinate ids, counting up from 1, so adding a range never moves a slice.
- **Names stay.** Crates are `komodo_*` and `periphery`; env vars start with
  `KOMODO_` and `PERIPHERY_`. Renaming them breaks every install.
- **Removed on purpose:** docker, compose, Swarm, Deployments, Builds,
  Builders, terminals and container exec, Variables. Don't bring them back.

## Build and test

```sh
cargo fmt --all -- --check
cargo clippy --workspace -- -D warnings
cargo test --workspace

cd client/core/ts && yarn && yarn build && yarn link
cd ../../../ui && yarn link komodo_client && yarn && yarn build
```

CI runs these. `scripts/e2e.sh` is not in CI (it needs a privileged
container). Run it for any change to deploys, the agent, secrets, Alerters or
ResourceSync. It needs docker, cargo, python3 and openssl.

`yarn link` is global per machine. If another checkout registered
`komodo_client` first, the UI typechecks against that checkout's client and
reports nothing wrong. Check `readlink -f ui/node_modules/komodo_client`
points at this checkout's `client/core/ts`.

## Generated files

Change an API or entity type in `client/core/rs`, then regenerate all of
these and commit them:

1. `node client/core/ts/generate_types.mjs` writes `client/core/ts/src/types.ts`.
   `client/core/ts/src/responses.ts` is hand-written: add or remove request
   entries there yourself.
2. `cd client/core/ts && yarn build`, then copy `dist/.` into
   `ui/public/client/` and run `node ui/fix_public_client.mjs`. Actions import
   this copy from Core at runtime; it was once left stale for weeks.
3. `cargo xtask generate resource-schema --pretty --file=./ui/public/schema/resources.json`
   for the ResourceSync editor's autocomplete.

`runfile.toml`'s `gen-client` task does steps 1 and 2.

## Adding a resource type

Several lists of resource types are kept by hand. Search for
`New resource types need to be added here manually` and add the type to each,
plus `extend_resources` in `bin/core/src/sync/file.rs`. Then add it to
`EVERY_TYPE` in that file's test, which fails if a sync file's resources are
dropped. Also check the OpenAPI lists in `client/core/rs/src/api/*/openapi.rs`
and `km list`.

## Releasing

See `docs/releasing.md`. The tag must equal `[workspace.package] version` in
`Cargo.toml` or the release builds nothing. Push tags by name, never with
`--tags`: the repo carries upstream's `v2.3.3` tag.

## Conventions

- Commit subjects start with the area: `stacks:`, `agent:`, `core:`, `ui:`,
  `sync:`, `ci:`, `docs:`.
- PRs are squash-merged; the PR title becomes the commit subject.
- Docs and comments use plain, specific sentences. Examples use example.com
  and private address ranges, never a real host.
