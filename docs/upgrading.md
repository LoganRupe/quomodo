# Upgrading

## 0.4.x to 0.5.0

0.5.0 moves Core onto the current major versions of Komodo's `mogh_*`
libraries (auth server 8, auth client 8, mogh_ui 4). Nothing has to be
migrated: users, password hashes, API keys, TOTP and passkey 2FA, OIDC links
and Core's and the agents' keys are read as 0.4.x stored them. Upgrade Core,
then the agents, as in [Releasing](releasing.md#rolling-out).

What changes for users:

- **Sessions.** Anyone logged in stays logged in until their session expires
  (`jwt_ttl`, a day by default), then logs in again as usual. Changing a
  password or 2FA, linking a login or creating an API key needs a login from
  the last 15 minutes: the UI asks for one when it is older. A session from
  before the upgrade counts as old.
- **Profile.** The linked logins table is now called Providers and lists
  every login the user has, including the one they signed up with. Unlinking
  that one is refused: set a password or link OIDC first, and remove the
  other one.
- **TOTP recovery codes** now log in when the authenticator app is lost. Each
  code works once.

What changes for operators:

- **Client addresses behind a proxy.** Rate limits and API key IP whitelists
  use the client's address. Core now believes `X-Forwarded-For` and
  `X-Real-IP` only from a loopback or private address, the usual place for a
  reverse proxy. Before, it believed them from anyone, so a client could pick
  its own address. If your proxy reaches Core from a public address, list it
  in `trusted_proxies` (`KOMODO_TRUSTED_PROXIES`), as ips or cidrs. With
  nothing in front of Core, set it to `["none"]`. Behind a proxy that is not
  trusted, every user shares the proxy's rate limit, and Core logs that once.
- **Config files.** A config file listed in `KOMODO_CONFIG_PATHS` that fails
  to parse now stops Core from starting, with the file and the error. 0.4.x
  skipped it with a warning and started without its settings.
- **`jwt_secret`.** A secret shorter than 32 bytes still works, and Core logs
  a warning at startup. Generate one with `openssl rand -base64 32`, as the
  [install](install.md#core) does. Changing it logs everyone out.
- **API keys** expire exactly at their expiry time. 0.4.x accepted them for
  five more minutes. A key created through the API can carry a
  `cidr_whitelist` of the addresses it may be used from.
- **OIDC.** The `oidc_*` options and the redirect URI registered at your
  provider (`<host>/auth/oidc/callback`) are unchanged.

Environment variables keep their `KOMODO_` and `PERIPHERY_` names.
