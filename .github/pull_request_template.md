## What and why

<!-- What changes for the user, and the issue it fixes (Fixes #123). -->

## Checks

- [ ] `cargo fmt --all -- --check`, `cargo clippy --workspace -- -D warnings` and `cargo test --workspace` pass (CI runs these too)
- [ ] Touches deploys, the agent, secrets, Alerters or ResourceSync: `scripts/e2e.sh` passes (not in CI)
- [ ] Changes API types: regenerated `client/core/ts/src/types.ts`, `ui/public/client` and `ui/public/schema/resources.json` (see AGENTS.md)
- [ ] User-visible change: `docs/` updated
- [ ] No secrets, real hostnames or personal emails in code, fixtures or commit messages
