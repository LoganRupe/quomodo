# Security

## Reporting a vulnerability

Report it privately with the "Report a vulnerability" button on the
repository's Security tab. Please don't open a public issue for it.

Only the latest release gets fixes.

## Upstream fixes

Quomodo is a hard fork of Komodo v2.3.3. It does not merge upstream. When
Komodo publishes a security fix to code Quomodo still has, the fix is
cherry-picked by hand into a new release. A Komodo advisory about something
Quomodo removed (docker, compose, terminals, the inbound Periphery listener,
Builds, Deployments) does not apply.

## Trust model

Know these before you deploy it:

- **The agent is root.** It writes `/etc/containers/systemd`, drives systemd
  and rootful podman, and runs a Stack's pre- and post-deploy commands as
  root. Anyone who can deploy a Stack to a Server, or edit the git branch
  that Stack reads, is root on that host. Give write access on Stacks,
  Servers and their repos accordingly. This applies to API keys and webhook
  secrets too.
- **The connection is authenticated, not encrypted.** The agent dials Core
  and both run a Noise handshake with their key pairs: Core checks the
  agent's public key against its Server, and the agent pins Core's key on
  first contact. After the handshake, messages travel as plain websocket
  frames, and they include resolved secret values. Use `wss://`, or keep the
  path on an encrypted network such as WireGuard or Tailscale.
- **Where secrets live.** Secret values are never stored in mongo, in the repo
  clone, or in a file under `/etc/containers/systemd`. Core holds them in
  memory during a deploy and sends them to the agent. The agent creates
  podman secrets with podman's default file driver, which stores them
  unencrypted under `/var/lib/containers/storage/secrets`, readable by root.
  Core's own bootstrap secrets (database password, JWT secret, vault
  credentials) are root podman secrets on the Core host in the same way.
- **Core can read the vault.** With the Vaultwarden provider, Core holds an
  API key and master password that decrypt every collection the service
  account can see. Limit that account to the collections Stacks use.
- **Agents listen on nothing.** There is no inbound port, passkey or
  terminal on an agent host.
