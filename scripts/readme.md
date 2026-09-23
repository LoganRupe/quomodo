# Agent install notes

The agent is a single static-ish binary (`quomodo-agent`, OpenSSL vendored)
published as a Forgejo release asset. An Ansible role does the
install; this is what it has to produce on a host.

Files:

- `/usr/local/bin/quomodo-agent`
- `/etc/quomodo/config.toml` (see `config/agent.config.toml`)
- `/etc/quomodo/keys/agent.key` (`quomodo-agent key gen`) and
  `/etc/quomodo/keys/core.pub`
- `/var/lib/quomodo/` for repo clones and deploy state

Unit (`/etc/systemd/system/quomodo-agent.service`):

```
[Unit]
Description=Quomodo agent
After=network-online.target tailscaled.service
Wants=network-online.target

[Service]
ExecStart=/usr/local/bin/quomodo-agent --config-path /etc/quomodo/config.toml
Restart=always
RestartSec=5

[Install]
WantedBy=multi-user.target
```

The agent runs as root and drives the rootful podman engine and systemd.
It needs `podman`, `git` and `systemctl` on the PATH.
