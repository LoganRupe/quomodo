# Scripts

- `e2e.sh`: the end-to-end test. Builds Core and the agent, then stands up
  FerretDB, Core, a git repo and a privileged Fedora container running
  systemd, podman and the agent, and checks deploys, restarts, user
  namespace slices and teardown. Needs docker, cargo and python3; the header
  lists what it proves and its options.
- `e2e/agent-host.Dockerfile`: the throwaway agent host `e2e.sh` and
  `demo.sh` run.
- `demo.sh`: a throwaway home lab for screenshots and demos. Four agent
  hosts (atlas, hermes, pi-garage, minerva), two of them also running a
  rootless agent, with six stacks of real images. Core is
  `quomodo.home.arpa`, a git server is `git.home.arpa`, and a
  post-receive hook sends each push to the Stack webhooks. The
  UI is on http://127.0.0.1:9142 (demo / quomodo-demo). Tears down on exit;
  `KEEP=1` leaves it running and `scripts/demo.sh down` removes it. Needs
  docker, cargo, python3, openssl and a built `ui/dist`.
- `demo-video/`: records the readme GIF against a `KEEP=1 scripts/demo.sh`.
  `record.mjs` drives playwright through the push-to-deploy loop, the
  Servers page and the Stacks page, one video per page plus `marks.json`.
  `assemble.py` cuts those into `demo.mp4` and `demo.gif` with ffmpeg and
  ends on `slide.html`. Both write to `/tmp/quomodo-demo-rec`; set
  `PLAYWRIGHT` to playwright's `index.mjs` if it is not installed where
  node finds it.

Installing the agent on a real host is in [docs/install.md](../docs/install.md#agent).
