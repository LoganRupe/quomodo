# Secrets

Core reads stack secrets from an external store at deploy time, sends them to
the agent, and the agent turns them into podman secrets. Secret values are
never written to mongo, the repo clone, or a file under `/etc`.
[stacks.md](stacks.md#secrets) covers how stack files reference
them.

Each Stack names one collection with `secrets_collection`. A Stack that
references a secret without naming a collection fails to deploy. Alerters
resolve `[[NAME]]` in their endpoint url the same way, from their own
`secrets_collection` ([alerters.md](alerters.md)).

## Providers

`secrets.provider` (`KOMODO_SECRETS_PROVIDER`) picks the store:

| Value | Store |
| --- | --- |
| `none` (default) | No store. Any secret reference fails the deploy. |
| `vaultwarden` | A Vaultwarden or Bitwarden vault, over the Bitwarden vault protocol |

## Vaultwarden and Bitwarden

Vaultwarden does not implement Bitwarden Secrets Manager, so Core logs in
the way a Bitwarden client does: a user's API key, plus that user's master
password to decrypt items on Core's side. KDFs supported: PBKDF2-SHA256 and
Argon2id.

Set up a service account for Core:

1. Create a vault user for Core alone, and generate its API key under
   Account settings, Security, Keys. The client id looks like `user.<uuid>`.
2. Invite it to the organization that holds your stack collections. Give it
   read access to those collections and nothing else.
3. In each collection, name each item after the reference: the item `DB_PASSWORD`
   answers `[[DB_PASSWORD]]` with its password, and its custom field `user`
   answers `[[DB_PASSWORD.user]]`.

Collection names must be unique across every organization the account can
see. Core matches collections and items by their decrypted names.

Core config:

| Field | Env | |
| --- | --- | --- |
| `secrets.vaultwarden.url` | `KOMODO_VAULTWARDEN_URL` | The vault's base URL |
| `secrets.vaultwarden.client_id` | `KOMODO_VAULTWARDEN_CLIENT_ID` | API key client id |
| `secrets.vaultwarden.client_secret` | `KOMODO_VAULTWARDEN_CLIENT_SECRET` | API key client secret |
| `secrets.vaultwarden.master_password` | `KOMODO_VAULTWARDEN_MASTER_PASSWORD` | The account's master password |

The last three also accept `_FILE` variants. In the example Core pod they
come from root podman secrets:

```sh
printf '%s' 'user.xxxxxxxx' | sudo podman secret create quomodo-vaultwarden-client-id -
sudo podman secret create quomodo-vaultwarden-client-secret ./client_secret.txt
sudo podman secret create quomodo-vaultwarden-master-password ./master_password.txt
```

Core logs in once and reuses the session for 50 minutes. It syncs the vault
on every deploy, so an edited item takes effect on the next deploy, and
`DeployStackIfChanged` sees the change.

### Bitwarden cloud

Core calls `<url>/identity/connect/token` and `<url>/api/sync`. The Bitwarden
web vault serves both paths, so `url = "https://vault.bitwarden.com"` (or
`https://vault.bitwarden.eu`) should work. Nobody has tested it against
Bitwarden's servers yet. Reports welcome.

The master password sits in Core's environment, so treat the Core host like a
machine that can read those collections.

## Adding a provider

A provider implements `SecretProvider` in
[`bin/core/src/secrets/mod.rs`](../bin/core/src/secrets/mod.rs): given a
collection and a list of names, return every value or fail. Add a variant to
`SecretsProvider` in `client/core/rs/src/entities/config/core.rs` and its
config beside `VaultwardenConfig`. Bitwarden Secrets Manager, OpenBao/Vault
KV and 1Password Connect would all fit.
