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
- **Agents only connect over TLS.** The agent dials Core and both run a
  Noise handshake with their key pairs: Core checks the agent's public key
  against its Server, and the agent pins Core's key on first contact. The
  handshake authenticates but does not encrypt, and the session after it
  carries resolved secret values. So the agent only accepts `wss://`
  addresses and refuses to start on `ws://` or `http://`. It always verifies
  Core's certificate, against the system roots or a CA file you give it;
  there is no switch to turn verification off.
- **Where secrets live.** Secret values are never stored in mongo, in the repo
  clone, or in a file under `/etc/containers/systemd`. Core holds them in
  memory during a deploy and sends them to the agent. The agent creates
  podman secrets with podman's default file driver, which stores them
  unencrypted under `/var/lib/containers/storage/secrets`, readable by root.
  Core's own bootstrap secrets (database password, JWT secret, vault
  credentials) are root podman secrets on the Core host in the same way. When
  Core serves TLS itself, its private key is a file in the host directory
  you mount, so it can be renewed in place; keep that directory root-only.
- **Core can read the vault.** With the Vaultwarden provider, Core holds an
  API key and master password that decrypt every collection the service
  account can see. Limit that account to the collections Stacks use.
- **Alerters send outbound.** Core posts alerts to the endpoints you
  configure. Keep webhook tokens in the secret store with `[[NAME]]`;
  a token typed straight into an endpoint url is stored in mongo.
- **Agents listen on nothing.** There is no inbound port, passkey or
  terminal on an agent host.
