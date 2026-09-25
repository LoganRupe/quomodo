#!/usr/bin/env bash
# End-to-end test of Quomodo without root on the docker host.
#
# What it stands up (all throwaway, all torn down on exit unless KEEP=1):
#   - FerretDB (sqlite) as the database, in docker
#   - Core, the release binary, bound to the docker bridge address
#   - a privileged Fedora container with systemd as PID 1 + podman, running the agent
#   - a bare git repo served over HTTP with a small quadlet stack
# What it proves:
#   1. the agent dials in and the Server shows Ok
#   2. DeployStack installs the quadlet units, systemd starts them, podman runs them
#   3. DeployStackIfChanged is a no-op with nothing changed
#   4. a changed unit file redeploys only that unit
#   5. a changed bind-mounted config file restarts the unit that mounts it
#   6. a .build unit builds from its context; binary and >1 MiB files are
#      installed byte for byte, executables stay executable, and a hash
#      that counts blobs still short-circuits DeployStackIfChanged
#   7. a changed text file in the build context rebuilds the image and
#      restarts its container, and nothing else
#   8. a changed binary file in the build context does the same
#   9. DestroyStack removes units and the directory
#
# Usage: scripts/e2e.sh              (from the repo root; needs docker, cargo, python3)
#        KEEP=1 scripts/e2e.sh       (leave everything running for a look)
#        SKIP_BUILD=1 scripts/e2e.sh (use the binaries already in target/release)
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
WORK=${WORK:-/tmp/quomodo-e2e}
BRIDGE_IP=${BRIDGE_IP:-$(docker network inspect bridge -f '{{(index .IPAM.Config 0).Gateway}}')}
CORE_PORT=${CORE_PORT:-9131}
GIT_PORT=${GIT_PORT:-8791}
DB_PORT=${DB_PORT:-27127}
HOST_NAME=quomodo-e2e-host
DB_NAME=quomodo-e2e-db
STACK=e2e
CORE_URL="http://$BRIDGE_IP:$CORE_PORT"

log() { printf '\n\033[1;34m== %s\033[0m\n' "$*"; }
fail() { printf '\n\033[1;31mFAIL: %s\033[0m\n' "$*"; exit 1; }
hexec() { docker exec "$HOST_NAME" "$@"; }

teardown() {
  [ -f "$WORK/core.pid" ] && kill "$(cat "$WORK/core.pid")" 2>/dev/null || true
  [ -f "$WORK/git.pid" ] && kill "$(cat "$WORK/git.pid")" 2>/dev/null || true
  docker rm -f "$HOST_NAME" "$DB_NAME" >/dev/null 2>&1 || true
}
cleanup() {
  if [ "${KEEP:-0}" = "1" ]; then echo "KEEP=1: leaving $HOST_NAME, $DB_NAME, core (pid $(cat "$WORK/core.pid" 2>/dev/null || echo ?)) running"; return; fi
  teardown
}
trap cleanup EXIT

# Always start clean, even after a KEEP=1 run.
teardown
rm -rf "$WORK"; mkdir -p "$WORK"/{keys,repo-cache,syncs,actions,git}

if [ "${SKIP_BUILD:-0}" != "1" ]; then
  log "build"
  (cd "$ROOT" && cargo build --release -p komodo_core -p komodo_periphery -q)
fi
AGENT_BIN="$ROOT/target/release/quomodo-agent"
CORE_BIN="$ROOT/target/release/quomodo"

log "database (FerretDB, sqlite)"
docker run -d --name "$DB_NAME" -p "127.0.0.1:$DB_PORT:27017" -e FERRETDB_HANDLER=sqlite ghcr.io/ferretdb/ferretdb:1 >/dev/null

log "git repo with a plain quadlet stack, served on $BRIDGE_IP:$GIT_PORT"
mkdir -p "$WORK/git/work/$STACK"
cat > "$WORK/git/work/$STACK/web.container" <<UNIT
[Unit]
Description=e2e web
[Container]
Image=docker.io/library/alpine:3.20
ContainerName=e2e-web
Volume=./motd.txt:/motd.txt:ro
Exec=sh -c 'while true; do cat /motd.txt; sleep 30; done'
[Service]
Restart=always
[Install]
WantedBy=multi-user.target
UNIT
cat > "$WORK/git/work/$STACK/worker.container" <<UNIT
[Container]
Image=docker.io/library/alpine:3.20
ContainerName=e2e-worker
Volume=e2e-data.volume:/data
Exec=sleep infinity
[Install]
WantedBy=multi-user.target
UNIT
printf '[Volume]\n' > "$WORK/git/work/$STACK/e2e-data.volume"
echo "hello v1" > "$WORK/git/work/$STACK/motd.txt"
GIT="git -C $WORK/git/work -c user.email=e2e@test -c user.name=e2e"
$GIT init -q -b main && $GIT add -A && $GIT commit -qm "v1"
git clone -q --bare "$WORK/git/work" "$WORK/git/e2e.git" && git -C "$WORK/git/e2e.git" update-server-info
python3 -m http.server "$GIT_PORT" --bind "$BRIDGE_IP" --directory "$WORK/git" > "$WORK/git.log" 2>&1 & echo $! > "$WORK/git.pid"

log "core on $CORE_URL"
(
  export KOMODO_HOST="$CORE_URL" KOMODO_PORT="$CORE_PORT" KOMODO_BIND_IP="$BRIDGE_IP"
  export KOMODO_DATABASE_ADDRESS="127.0.0.1:$DB_PORT" KOMODO_DATABASE_USERNAME= KOMODO_DATABASE_PASSWORD=
  export KOMODO_JWT_SECRET=e2e_secret KOMODO_LOCAL_AUTH=true KOMODO_ENABLE_NEW_USERS=true
  export KOMODO_UI_PATH="$ROOT/ui/dist" KOMODO_PRIVATE_KEY="file:$WORK/keys/core.key"
  export KOMODO_REPO_DIRECTORY="$WORK/repo-cache" KOMODO_SYNC_DIRECTORY="$WORK/syncs" KOMODO_ACTION_DIRECTORY="$WORK/actions"
  export KOMODO_SSL_ENABLED=false KOMODO_LOGGING_LEVEL=info
  nohup "$CORE_BIN" > "$WORK/core.log" 2>&1 & echo $! > "$WORK/core.pid"
)
for i in $(seq 1 30); do curl -sf "$CORE_URL/version" >/dev/null && break; sleep 1; done
curl -sf "$CORE_URL/version" >/dev/null || { tail -20 "$WORK/core.log"; fail "core did not start"; }

log "api helper"
cat > "$WORK/api.py" <<PY
import json, sys, time, urllib.request
BASE = "$CORE_URL"
def post(path, body, jwt=None):
    req = urllib.request.Request(BASE + path, data=json.dumps(body).encode(), headers={"content-type": "application/json", **({"authorization": "Bearer " + jwt} if jwt else {})})
    try:
        with urllib.request.urlopen(req, timeout=120) as r: return json.loads(r.read())
    except urllib.error.HTTPError as e:
        return {"error": e.read().decode()[:500], "http": e.code}
def jwt():
    for t in ("LoginLocalUser", "SignUpLocalUser"):
        r = post("/auth/login/" + t, {"username": "e2e", "password": "e2e-pass-123"})
        j = r.get("jwt") or (r.get("data") or {}).get("jwt")
        if j: return j
    raise SystemExit("login failed: %r" % r)
J = jwt()
def api(kind, typ, params): return post("/" + kind, {"type": typ, "params": params}, J)
def wait_update(uid):
    for _ in range(120):
        u = api("read", "GetUpdate", {"id": uid})
        if u.get("status") == "Complete": return u
        time.sleep(1)
    raise SystemExit("update %s did not complete" % uid)
def execute(typ, params):
    # /execute returns the initial Update at once and runs the task in the background.
    # DeployStackIfChanged creates no Update at all when nothing changed, so an empty
    # id means: watch the stack's update list for a DeployStack to appear.
    t0 = int(time.time() * 1000)
    u = api("execute", typ, params)
    if "error" in u: raise SystemExit("%s refused: %s" % (typ, u["error"]))
    uid = u.get("id") or (u.get("_id") or {}).get("\$oid")
    if uid: return wait_update(uid)
    stack_id = api("read", "GetStack", {"stack": "$STACK"})["_id"]["\$oid"]
    for _ in range(10):
        time.sleep(1)
        for it in api("read", "ListUpdates", {"query": {"target.id": stack_id}})["updates"]:
            if it["start_ts"] >= t0 and it["operation"] == "DeployStack":
                return wait_update(it["id"])
    return {"noop": True, "success": True, "logs": []}
cmd = sys.argv[1]
if cmd == "create-server":
    print(json.dumps(api("write", "CreateServer", {"name": "$HOST_NAME", "config": {"enabled": True}, "public_key": sys.argv[2]})))
elif cmd == "server-state":
    print(api("read", "GetServerState", {"server": "$HOST_NAME"}).get("status"))
elif cmd == "create-stack":
    print(json.dumps(api("write", "CreateStack", {"name": "$STACK", "config": {"server_id": "$HOST_NAME", "git_provider": "$BRIDGE_IP:$GIT_PORT", "git_https": False, "repo": "e2e.git", "branch": "main", "run_directory": "$STACK"}}))[:300])
elif cmd == "stack":
    s = api("read", "GetStack", {"stack": "$STACK"}); i = s["info"]
    print(json.dumps({"latest_units": [u["file"] for u in i.get("latest_units", [])], "deployed_units": [u["file"] for u in (i.get("deployed_units") or [])], "deployed_hash": i.get("deployed_hash"), "contents_hash": i.get("deployed_contents_hash")}))
elif cmd == "state":
    print(api("read", "ListStacks", {"query": {"names": ["$STACK"]}})[0]["info"]["state"])
elif cmd == "services":
    print(json.dumps([(s["service"], s["state"]) for s in api("read", "ListStackServices", {"stack": "$STACK"})]))
elif cmd in ("DeployStack", "DeployStackIfChanged", "DestroyStack", "RestartStack", "StopStack"):
    u = execute(cmd, {"stack": "$STACK", **({"services": []} if cmd in ("DeployStack", "RestartStack", "StopStack") else {})})
    print(json.dumps({"success": u["success"], "noop": u.get("noop", False), "stages": [(l["stage"], l["success"]) for l in u["logs"]]}))
    if not u["success"]:
        for l in u["logs"]:
            if not l["success"]: print("--", l["stage"], "\n", l["stdout"][-1500:], "\n", l["stderr"][-1500:])
        raise SystemExit(cmd + " failed")
elif cmd == "refresh":
    print(api("write", "RefreshStackCache", {"stack": "$STACK"}))
PY
API="python3 $WORK/api.py"

log "agent host container (systemd + podman)"
docker build -q -t quomodo-agent-host -f "$ROOT/scripts/e2e/agent-host.Dockerfile" "$ROOT/scripts/e2e" >/dev/null
docker run -d --name "$HOST_NAME" --privileged --cgroupns=private --tmpfs /run --tmpfs /tmp quomodo-agent-host >/dev/null
sleep 4
docker cp "$AGENT_BIN" "$HOST_NAME:/usr/local/bin/quomodo-agent"
hexec mkdir -p /etc/quomodo/keys /var/lib/quomodo
hexec sh -c "cat > /etc/quomodo/config.toml <<CFG
core_addresses = [\"ws://$BRIDGE_IP:$CORE_PORT\"]
connect_as = \"$HOST_NAME\"
logging.level = \"info\"
CFG"
hexec sh -c 'cat > /etc/systemd/system/quomodo-agent.service <<UNIT
[Unit]
Description=Quomodo agent
After=network-online.target
[Service]
ExecStart=/usr/local/bin/quomodo-agent --config-path /etc/quomodo/config.toml
Restart=always
RestartSec=2
[Install]
WantedBy=multi-user.target
UNIT
systemctl daemon-reload && systemctl enable --now quomodo-agent'
# The agent generates its key file on first start; public-key reads it back.
PUBKEY=$(hexec /usr/local/bin/quomodo-agent --config-path /etc/quomodo/config.toml public-key)
[ -n "$PUBKEY" ] || fail "no public key"

log "1. server connects"
$API create-server "$PUBKEY" | head -c 200; echo
for i in $(seq 1 30); do [ "$($API server-state)" = "Ok" ] && break; sleep 1; done
[ "$($API server-state)" = "Ok" ] || { hexec journalctl -u quomodo-agent --no-pager | tail -20; fail "server never reached Ok"; }
echo "server Ok"

log "2. deploy"
$API create-stack; echo
$API stack
$API DeployStack
hexec systemctl is-active web.service worker.service || { hexec journalctl -u web.service -u worker.service --no-pager | tail -30; fail "units not active"; }
hexec podman ps --format '{{.Names}} {{.Status}}'
hexec test -f /etc/containers/systemd/$STACK/web.container || fail "unit file not installed"
hexec test -f /etc/containers/systemd/$STACK/motd.txt || fail "config file not installed"
hexec grep -q "UserNS=auto" /etc/containers/systemd/$STACK/web.container || fail "UserNS=auto not injected"
$API stack; sleep 12
[ "$($API state)" = "running" ] || fail "stack state is $($API state), expected running"
$API services

log "3. no change is a no-op"
WEB_ID_BEFORE=$(hexec podman inspect -f '{{.Id}}' e2e-web)
$API DeployStackIfChanged | grep -q '"noop": true' || fail "expected no DeployStack update when nothing changed"
[ "$(hexec podman inspect -f '{{.Id}}' e2e-web)" = "$WEB_ID_BEFORE" ] || fail "web was recreated with no change"
echo "short-circuited, containers untouched"

log "4. a changed unit redeploys only that unit"
WEB_ID_BEFORE=$(hexec podman inspect -f '{{.Id}}' e2e-web); WORKER_ID_BEFORE=$(hexec podman inspect -f '{{.Id}}' e2e-worker)
sed -i 's/sleep 30/sleep 20/' "$WORK/git/work/$STACK/web.container"
$GIT commit -qam "v2 web" && git -C "$WORK/git/e2e.git" fetch -q "$WORK/git/work" main:main && git -C "$WORK/git/e2e.git" update-server-info
$API DeployStackIfChanged
[ "$(hexec podman inspect -f '{{.Id}}' e2e-web)" != "$WEB_ID_BEFORE" ] || fail "web was not recreated"
[ "$(hexec podman inspect -f '{{.Id}}' e2e-worker)" = "$WORKER_ID_BEFORE" ] || fail "worker was recreated but its file did not change"
echo "only web restarted"

log "5. a changed mounted config restarts the mounting unit"
WEB_ID_BEFORE=$(hexec podman inspect -f '{{.Id}}' e2e-web)
echo "hello v3" > "$WORK/git/work/$STACK/motd.txt"
$GIT commit -qam "v3 motd" && git -C "$WORK/git/e2e.git" fetch -q "$WORK/git/work" main:main && git -C "$WORK/git/e2e.git" update-server-info
$API DeployStackIfChanged
[ "$(hexec podman inspect -f '{{.Id}}' e2e-web)" != "$WEB_ID_BEFORE" ] || fail "web was not restarted after its motd changed"
hexec podman exec e2e-web cat /motd.txt | grep -q "hello v3" || fail "container does not see the new motd"
echo "web restarted with the new file"

publish() {
  $GIT add -A && $GIT commit -qm "$1" && git -C "$WORK/git/e2e.git" fetch -q "$WORK/git/work" main:main && git -C "$WORK/git/e2e.git" update-server-info
}
installed_sha() { hexec sha256sum "/etc/containers/systemd/$STACK/$1" | cut -d' ' -f1; }
local_sha() { sha256sum "$WORK/git/work/$STACK/$1" | cut -d' ' -f1; }

log "6. a .build unit with a binary asset, a large file and an executable"
mkdir -p "$WORK/git/work/$STACK/app"
cat > "$WORK/git/work/$STACK/app.build" <<UNIT
[Build]
ImageTag=localhost/e2e-app
SetWorkingDirectory=app
File=app/Containerfile
UNIT
cat > "$WORK/git/work/$STACK/app.container" <<UNIT
[Container]
Image=app.build
ContainerName=e2e-app
[Install]
WantedBy=multi-user.target
UNIT
cat > "$WORK/git/work/$STACK/app/Containerfile" <<'CF'
FROM docker.io/library/alpine:3.20
COPY . /app
CMD ["/app/entry.sh"]
CF
printf '#!/bin/sh\nexec sleep infinity\n' > "$WORK/git/work/$STACK/app/entry.sh"
chmod +x "$WORK/git/work/$STACK/app/entry.sh"
echo v1 > "$WORK/git/work/$STACK/app/version"
python3 -c 'import sys; sys.stdout.buffer.write(bytes(range(256)) * 8)' > "$WORK/git/work/$STACK/app/logo.png"
head -c 1100000 /dev/urandom > "$WORK/git/work/$STACK/app/big.bin"
publish "v4 app"
WEB_ID_BEFORE=$(hexec podman inspect -f '{{.Id}}' e2e-web)
$API DeployStackIfChanged
hexec systemctl is-active app.service || { hexec journalctl -u app-build.service -u app.service --no-pager | tail -30; fail "app not active"; }
for f in app/logo.png app/big.bin; do
  [ "$(installed_sha $f)" = "$(local_sha $f)" ] || fail "$f was not installed byte for byte"
done
hexec test -x "/etc/containers/systemd/$STACK/app/entry.sh" || fail "entry.sh lost its executable bit"
[ "$(hexec podman exec e2e-app cat /app/version)" = "v1" ] || fail "image was not built from the context"
[ "$(hexec podman exec e2e-app sha256sum /app/logo.png | cut -d' ' -f1)" = "$(local_sha app/logo.png)" ] || fail "image has the wrong logo.png"
[ "$(hexec podman inspect -f '{{.Id}}' e2e-web)" = "$WEB_ID_BEFORE" ] || fail "web was recreated by an unrelated change"
$API DeployStackIfChanged | grep -q '"noop": true' || fail "expected a no-op: Core and agent hashes disagree once blobs are involved"
echo "built, installed, and the next deploy short-circuited"

log "7. a changed text file in the build context rebuilds"
APP_ID_BEFORE=$(hexec podman inspect -f '{{.Id}}' e2e-app)
echo v2 > "$WORK/git/work/$STACK/app/version"
publish "v5 version"
$API DeployStackIfChanged
[ "$(hexec podman inspect -f '{{.Id}}' e2e-app)" != "$APP_ID_BEFORE" ] || fail "app was not recreated"
[ "$(hexec podman exec e2e-app cat /app/version)" = "v2" ] || fail "app was not rebuilt"
[ "$(hexec podman inspect -f '{{.Id}}' e2e-web)" = "$WEB_ID_BEFORE" ] || fail "web was recreated by a build context change"
echo "rebuilt, web untouched"

log "8. a changed binary file in the build context rebuilds"
APP_ID_BEFORE=$(hexec podman inspect -f '{{.Id}}' e2e-app)
python3 -c 'import sys; sys.stdout.buffer.write(bytes(range(255, -1, -1)) * 8)' > "$WORK/git/work/$STACK/app/logo.png"
publish "v6 logo"
$API DeployStackIfChanged
[ "$(installed_sha app/logo.png)" = "$(local_sha app/logo.png)" ] || fail "new logo.png was not installed"
[ "$(hexec podman inspect -f '{{.Id}}' e2e-app)" != "$APP_ID_BEFORE" ] || fail "app was not recreated"
[ "$(hexec podman exec e2e-app sha256sum /app/logo.png | cut -d' ' -f1)" = "$(local_sha app/logo.png)" ] || fail "image has the old logo.png"
echo "rebuilt with the new binary"

log "9. destroy"
$API DestroyStack
hexec systemctl is-active web.service >/dev/null 2>&1 && fail "web.service still active after destroy"
hexec test ! -e /etc/containers/systemd/$STACK || fail "stack directory still present"
echo "destroyed"

log "PASS"
