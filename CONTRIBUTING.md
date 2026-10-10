# Contributing

Issues and pull requests are welcome. Everyone taking part follows the
[Code of Conduct](CODE_OF_CONDUCT.md). For anything bigger than a bug fix,
open an issue first so we can agree on the shape before you write it.

## Scope

Quomodo deploys podman quadlets to Linux hosts and keeps secrets out of its
database. Docker, compose, Swarm, terminals and the inbound Periphery
listener were removed on purpose and won't come back. New secret providers,
git providers, and anything that makes quadlet stacks more predictable are
in scope.

## Build and test

You need Rust (stable), Node 22 and yarn.

```sh
cargo fmt --all -- --check
cargo clippy --workspace -- -D warnings
cargo test --workspace

cd client/core/ts && yarn && yarn build && yarn link
cd ../../../ui && yarn link komodo_client && yarn && yarn build
```

CI runs exactly these. [`scripts/e2e.sh`](scripts/e2e.sh) is the end-to-end
test: Core, FerretDB, and a privileged Fedora container running systemd,
podman and the agent, all under docker on your machine. It needs docker,
cargo, python3 and openssl. Run it for any change
to deploys, secrets, or the agent. It is not in CI because it needs a
privileged container.

## Conventions

- Commit subjects start with the area: `stacks: ...`, `agent: ...`,
  `core: ...`, `ui: ...`, `ci: ...`.
- The Rust crates are still named `komodo_*` and `periphery`, and config
  environment variables still start with `KOMODO_` and `PERIPHERY_`. Keep
  them that way; renaming them breaks every existing install.
- A user-visible change updates the docs in [`docs/`](docs) in the same PR.

## License

Quomodo is GPL-3.0-or-later, like Komodo. By contributing you agree your
work is released under the same license.
