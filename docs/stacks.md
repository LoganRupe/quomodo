# Stacks

A Stack is a directory in git holding podman quadlet files: `.container`,
`.pod`, `.network`, `.volume`, `.image`, `.build` and `.kube` units, plus any
config files they mount and any build contexts they use. The Stack resource
names the repo, branch and `run_directory`.

Deploying copies the directory to `/etc/containers/systemd/<stack>/` on the
Server, runs `systemctl daemon-reload`, and starts or restarts the units
whose inputs changed. [`example/stack`](../example/stack) is a small working
stack.

## Secrets

Plain configuration goes in the files themselves: they are in git, so a
domain or a port is just written where it is used. `[[NAME]]` in a text file
is always a secret. Core reads it at deploy time from the Stack's
`secrets_collection` in the secret store ([secrets.md](secrets.md)).
`[[NAME]]` reads the item's password; `[[NAME.field]]` reads one of its
custom fields. The item's Username field is never read: a username is not a
secret, so write it in the unit file.

Secrets follow two rules, so a value never lands in a file under `/etc`:

- In a unit file, a secret may only appear on a `Secret=` line:
  `Secret=[[DB_PASSWORD]],type=env,target=POSTGRES_PASSWORD`. The agent
  creates the podman secret `<stack>_DB_PASSWORD` and the line points at it.
  A secret anywhere else in a unit file fails the deploy.
- A config file that contains a secret must be mounted as a secret:
  `Secret=file:config.yml,type=mount,target=/etc/app/config.yml`. Core
  interpolates it in memory and the agent creates the podman secret
  `<stack>_config.yml`. The file itself is not written to the quadlet
  directory. A config file holding a secret that no unit mounts this way
  fails the deploy.

Podman secret names keep `[A-Za-z0-9_.-]`; any other character in the stack
name or path becomes `_`.

Set `skip_secret_interp = true` on a Stack to install its files without
resolving `[[NAME]]`.

Quomodo has no Variables. Komodo's global Variables lived in the database,
outside git; they were removed in v0.2.0. A `[[variable]]` block in a
ResourceSync file is ignored.

## Files that are not text

Binary files and text files over 1 MiB are copied byte for byte. They are
never interpolated, never shown in the UI, and cannot be a `Secret=file:`.
File modes are copied too, so a script in a build context stays executable.

## What a deploy restarts

`DeployStackIfChanged` does nothing unless the repo commit, a file, or a
resolved secret value changed, or the stack is not running. It also skips a
stack that another action, such as a deploy, is still running on. Files that
changed after that action started deploy on the next run.

It decides before the call returns. With nothing to deploy, the call returns a
finished Update with no id and a log stage of `Diff stack files` (unchanged)
or `Deploy skipped` (busy). That Update is not stored, so it doesn't show in
the updates list. Otherwise it returns the stored `DeployStack` Update, which
you can follow by its id. `BatchDeployStackIfChanged` returns one of these per
stack.

When it does deploy, and for a plain `DeployStack`:

- A changed `.container` or `.kube` restarts only that unit.
- A changed `.pod` restarts the pod; its members follow it.
- A changed `.network`, `.volume`, `.image` or `.build` restarts every unit
  that references it.
- A changed config file restarts the units that bind-mount it.
- A changed file in a `.build` context rebuilds the image and restarts the
  containers using it. Files the build's `.containerignore` or
  `.dockerignore` excludes don't count.
- A changed secret value restarts the units that reference it.
- A unit that failed, or never started, starts again even with nothing
  changed. A unit that ran and exited on its own, like an init container,
  is left alone.

A deploy waits for `systemctl start` as long as the stack's longest
`TimeoutStartSec=` plus a minute, and at least 10 minutes. A first start
that pulls large images, or a rootless one that re-owns every image layer
for the user's namespace, can take that long; give such a unit a
`TimeoutStartSec=` that covers it.

A stack can reference a unit file from another stack on the same Server,
such as `Network=app.network` from the stack that owns `app.network`.
Stacks deploy in parallel, so the deploy waits up to 5 minutes for the
other stack to install that file before it starts anything, then goes
ahead with a warning.

Pass `services` to `DeployStack` to restart named units only, whether they
changed or not. `auto_pull = true` pulls every container image before the
restart. `ignore_units` leaves units out of the stack's health state, for
example an init container that exits.

## Unit names

Quadlet names each systemd service after its unit file's name alone, not
the directory the file is in. `web.container` in two stacks on one host
would be one `web.service`, and `Network=app.network` could mean either
stack's file. A deploy fails if any of its units would share a service with
a unit file already under the quadlet directory, from another stack or put
there by hand. Prefix unit files that recur across stacks with the stack
name, for example `forgejo-newt.container`.

## User namespaces

Podman's `UserNS=auto` picks a new uid range each time a container is
created, and quadlet recreates the container on every start. After a reboot
or a redeploy, files a container wrote into a volume can belong to someone
else's uids and read as `nobody`.

The agent hands out fixed ranges instead. Every `.pod`, and every
`.container` that is not in a pod, gets its own 65536-id slice of the
`containers` subordinate range in `/etc/subuid` and `/etc/subgid`. The agent
picks the slice the first time it sees the unit, records it in
`/var/lib/quomodo/idmap.json`, and writes it into the installed unit as
`UIDMap=` / `GIDMap=`. A slice is never handed back: destroy a stack and
deploy it again, and its volumes keep their owners.

What the agent does with each `UserNS=` setting:

| In the unit | Result |
| --- | --- |
| No `UserNS=` | A fixed slice |
| `UserNS=auto`, with or without `size=`, `uidmapping=`, `gidmapping=` | A fixed slice, carve-outs kept, at most 65536 ids wide |
| A pod member with no `UserNS=` | Shares the pod's namespace |
| A pod member with `UserNS=auto` | Error. Podman would run it in the host namespace |
| `host`, `keep-id`, `nomap`, `ns:`, `container:`, or an explicit `UIDMap=` / `GIDMap=` | Left alone |

A carve-out (`uidmapping=C:H:S`) may not map onto the ids slices are taken
from, or it would share them with another unit. The deploy fails if it does.

A [rootless agent](install.md#rootless-agent) takes its slices from its
user's own subordinate ids instead of `containers`, and records them in
`~/.local/share/quomodo/idmap.json`. In a rootless unit, the host side of
`UIDMap=` and `GIDMap=` counts in the user's namespace: 0 is the user and
1 onwards are the user's subordinate ids, every range of theirs in
`/etc/subuid` order. Slices start at 1. A carve-out may only map onto the
user, as `uidmapping=1000:0:1`, which makes container uid 1000 the user on
the host.

Rootless quadlets written by hand often leave out `UserNS=`, so the
container's root is the user, and their volumes hold files owned by the
user. Deployed through Quomodo, such a unit gets a slice, and those files
read as `nobody` inside it. Set `UserNS=` (empty) to keep the user as root,
or `UserNS=keep-id` to run as the user, before the first deploy.

Outside Quomodo, podman's `:idmap` mount option is the usual fix. A volume
mounted with `Volume=data.volume:/data:idmap` keeps the same file owners
whatever uid range the container got, so `UserNS=auto` stops turning them
into `nobody`. The agent passes `Volume=` options through as written, so
`:idmap` works alongside a fixed slice too. It needs a filesystem that
supports idmapped mounts (ext4, xfs and btrfs do; NFS does not) and has to be
on every mount. The fixed slices need neither.

## Resource files

Stacks, Servers, Procedures and the rest can live in git as ResourceSync TOML.
A Stack:

```toml
[[stack]]
name = "app"
tags = ["auto-deploy"]
[stack.config]
server_id = "web-01"
git_provider = "github.com"
git_account = "deploy-bot"
repo = "me/infra"
branch = "main"
run_directory = "stacks/app"
secrets_collection = "app"
auto_pull = true
```

A Procedure that runs on a push webhook and on a schedule can sync these files
and then deploy whatever changed:

```toml
[[procedure]]
name = "update-stacks"
config.schedule_format = "Cron"
config.schedule = "0 */15 * * * *"
[[procedure.config.stage]]
name = "Sync"
enabled = true
executions = [
  { execution.type = "RunSync", execution.params.sync = "infra", enabled = true },
]
[[procedure.config.stage]]
name = "Deploy"
enabled = true
executions = [
  { execution.type = "BatchDeployStackIfChanged", execution.params = { pattern = "*", tags = ["auto-deploy"] }, enabled = true },
]
```

`DeployStackIfChanged` treats a stack that is not running as changed. The
tag filter is what stops a new stack from starting on merge, or a stack you
stopped on purpose from coming back. Tag a stack after its first manual
deploy.
