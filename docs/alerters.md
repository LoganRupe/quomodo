# Alerters

Core records an alert when something goes wrong: a Server stops answering or
runs out of disk, a Server's agent can't list its containers, a Stack changes
state on its own, a Procedure or Action fails, a ResourceSync has pending
changes. An Alerter sends those alerts somewhere you will see them. Alerters
live under Settings, Alerters.

## Endpoints

| Type | Sends |
| --- | --- |
| Custom | The alert as JSON in a POST to your url |
| Slack | A message to a Slack incoming webhook url |
| Discord | A message to a Discord webhook url |
| Ntfy | A message to an ntfy topic url, optionally emailed via `email` |
| Pushover | A message to a Pushover url carrying `token` and `user` |

The Custom body is the `Alert` type from the API: `level` (`OK`, `WARNING`,
`CRITICAL`), `target`, `resolved`, and `data` with the alert `type` and its
fields.

## Tokens stay in the secret store

A webhook url usually is the credential. So the url, and the ntfy email, can
reference secrets the same way stack files do: write `[[NAME]]` and set the
Alerter's `secrets_collection`. Core resolves the references from the secret
store each time it sends ([secrets.md](secrets.md)), and scrubs the values
from any error it logs. Nothing secret is written to mongo.

```toml
[[alerter]]
name = "discord"
[alerter.config]
enabled = true
secrets_collection = "alerters"
endpoint.type = "Discord"
endpoint.params.url = "[[DISCORD_WEBHOOK_URL]]"
```

An endpoint that references a secret without a collection fails to send, and
the error names the missing reference.

## Filters

- `alert_types`: only send these types. Empty sends every type.
- `resources`: only send alerts about these resources. Empty means all.
- `except_resources`: never send alerts about these.
- `maintenance_windows`: send nothing during these windows.

Servers also choose which of their alerts exist at all: `send_unreachable_alerts`,
`send_cpu_alerts`, `send_mem_alerts`, `send_disk_alerts`,
`send_version_mismatch_alerts`. Stacks have `send_alerts`, ResourceSyncs have
`pending_alert`.

## When the agent can't list containers

The agent runs `podman ps` on every poll. When that fails (it times out after
20 seconds, or podman prints something the agent can't parse), Core keeps each
Stack's last known state rather than marking it down, so the Stack page goes
stale without anything changing colour.

If the listing keeps failing for 5 minutes, Core opens a
`ServerPodmanUnresponsive` warning on the Server. The first listing that
succeeds resolves it, even one with no containers. Five minutes is well past an
image prune: podman holds its storage lock while it deletes images, and
removing a 1.4 GB image has held it for over a minute. The time is fixed. The
alert follows the Server's `send_unreachable_alerts` switch and its maintenance
windows.

While the alert is open, check `podman ps` on the host as the agent's user,
and the agent's log for the error.

## Testing

`TestAlerter` (the Test button on the Alerter page) sends a test alert through
that one Alerter, ignoring its filters. `SendAlert` sends a custom message
through any set of Alerters, and works as a Procedure step.

[`scripts/e2e.sh`](../scripts/e2e.sh) points a Custom Alerter at a local
webhook and checks that the test alert, a real `ServerUnreachable` alert and
its resolution all arrive.
