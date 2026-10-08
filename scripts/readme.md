# Scripts

- `e2e.sh`: the end-to-end test. Builds Core and the agent, then stands up
  FerretDB, Core, a git repo and a privileged Fedora container running
  systemd, podman and the agent, and checks deploys, restarts, user
  namespace slices and teardown. Needs docker, cargo and python3; the header
  lists what it proves and its options.
- `e2e/agent-host.Dockerfile`: the throwaway agent host `e2e.sh` runs.

Installing the agent on a real host is in [docs/install.md](../docs/install.md#agent).
