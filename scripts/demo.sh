#!/usr/bin/env bash
# A throwaway Quomodo home lab for screenshots and demos.
#
# What it stands up, all in docker on one private network, torn down on exit:
#   - FerretDB (sqlite) as the database
#   - Core, the release binary, as quomodo.home.arpa, with a cert from a
#     throwaway CA, published on https://127.0.0.1:$CORE_PORT
#   - a git server (caddy, dumb http) as git.home.arpa holding homelab/stacks
#   - four "podman hosts" (the e2e agent-host image: Fedora, systemd, podman),
#     atlas, hermes, pi-garage and minerva, each running the agent
#   - a rootless agent beside the root one on hermes (user monitor) and on
#     minerva (user sync), each a lingering user with its own subordinate ids
#   - six stacks of real images: gitea and miniflux on atlas, vaultwarden on
#     hermes, uptime-kuma rootless on hermes, caddy on pi-garage, syncthing
#     rootless on minerva
#     (atlas also holds gitea 1.22.6, so the image bump in the readme demo
#     pulls no layers)
#   - a post-receive hook on the stack repo that sends every push to each
#     Stack's GitHub-style webhook, so a push runs DeployStackIfChanged
#   - the UI over plain http on 127.0.0.1:$UI_PORT, for browsers that do not
#     trust the throwaway CA
#
# Usage: scripts/demo.sh             (from the repo root; needs docker, cargo,
#                                     python3, openssl and a built ui/dist)
#        KEEP=1 scripts/demo.sh      (set up, then exit and leave it running)
#        scripts/demo.sh down        (tear down a KEEP=1 run)
#        SKIP_BUILD=1 scripts/demo.sh
#
# Then log in as demo / quomodo-demo. To change a stack, edit and push from
# $WORK/homelab (a clone of the stack repo); the hook deploys it.
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
WORK=${WORK:-/tmp/quomodo-demo}
CORE_PORT=${CORE_PORT:-9141}
UI_PORT=${UI_PORT:-9142}
NET=quomodo-demo
P=quomodo-demo
SERVERS=(atlas hermes pi-garage minerva)
declare -A REGION=([atlas]=rack [hermes]=rack [pi-garage]=garage [minerva]=office)
# Rootless agents, server:host:user. Their Servers report the host's name, so
# the Servers page lists them under it.
ROOTLESS=(hermes-monitor:hermes:monitor minerva-sync:minerva:sync)
declare -A ABOUT=(
  [atlas]="Ryzen box in the rack: git and feeds"
  [hermes]="NUC in the rack: passwords and monitoring"
  [pi-garage]="Raspberry Pi 5 in the garage: the reverse proxy"
  [minerva]="Old desktop in the office: file sync"
  [hermes-monitor]="Rootless agent for the monitoring user"
  [minerva-sync]="Rootless agent for the file sync user"
  [gitea]="Git hosting, Postgres on its own network"
  [miniflux]="Feed reader, app and Postgres in one pod"
  [vaultwarden]="Password vault"
  [uptime-kuma]="Status page and uptime checks"
  [caddy]="Reverse proxy for *.home.arpa"
  [syncthing]="File sync for the laptops"
)
CORE_URL="https://127.0.0.1:$CORE_PORT"
IMAGE_CACHE=${IMAGE_CACHE:-$HOME/.cache/quomodo-demo/images}
WEBHOOK_SECRET=demo-webhook-secret
USERNAME=demo
PASSWORD=quomodo-demo

# stack:server:images
STACKS=(
  "gitea:atlas:docker.io/gitea/gitea:1.22.4 docker.io/gitea/gitea:1.22.6 docker.io/library/postgres:16-alpine"
  "miniflux:atlas:docker.io/miniflux/miniflux:2.2.6 docker.io/library/postgres:16-alpine"
  "vaultwarden:hermes:docker.io/vaultwarden/server:1.32.7"
  "uptime-kuma:hermes-monitor:docker.io/louislam/uptime-kuma:1"
  "caddy:pi-garage:docker.io/library/caddy:2-alpine"
  "syncthing:minerva-sync:docker.io/syncthing/syncthing:1.29"
)

log() { printf '\n\033[1;34m== %s\033[0m\n' "$*"; }
fail() { printf '\n\033[1;31mFAIL: %s\033[0m\n' "$*"; exit 1; }

teardown() {
  local names=("$P-core" "$P-db" "$P-git" "$P-ui")
  for s in "${SERVERS[@]}"; do names+=("$P-$s"); done
  docker rm -f "${names[@]}" >/dev/null 2>&1 || true
  docker network rm "$NET" >/dev/null 2>&1 || true
}

if [ "${1:-}" = "down" ]; then teardown; echo "torn down"; exit 0; fi

cleanup() {
  [ -n "${SLEEP_PID:-}" ] && kill "$SLEEP_PID" 2>/dev/null
  if [ "${KEEP:-0}" = "1" ]; then
    echo "KEEP=1: leaving it running. Tear down with: scripts/demo.sh down"
    return
  fi
  teardown
}
trap cleanup EXIT
# Ctrl-C or a kill tears down too.
trap 'exit 130' INT TERM

teardown
rm -rf "$WORK"; mkdir -p "$WORK"/{core,tls,git}

if [ "${SKIP_BUILD:-0}" != "1" ]; then
  log "build"
  (cd "$ROOT" && cargo build --release -p komodo_core -p komodo_periphery -q)
fi
AGENT_BIN="$ROOT/target/release/quomodo-agent"
CORE_BIN="$ROOT/target/release/quomodo"
[ -f "$ROOT/ui/dist/index.html" ] || fail "no ui/dist: build the UI first (see AGENTS.md)"

log "images (cached in $IMAGE_CACHE)"
mkdir -p "$IMAGE_CACHE"
image_tar() { echo "$IMAGE_CACHE/$(echo "$1" | tr '/:' '__').tar"; }
for entry in "${STACKS[@]}"; do
  for img in ${entry#*:*:}; do
    tar=$(image_tar "$img")
    [ -s "$tar" ] && continue
    docker pull -q "$img" >/dev/null && docker save -o "$tar.tmp" "$img" && mv "$tar.tmp" "$tar"
    echo "cached $img"
  done
done
docker build -q -t quomodo-agent-host -f "$ROOT/scripts/e2e/agent-host.Dockerfile" "$ROOT/scripts/e2e" >/dev/null

docker network create "$NET" >/dev/null

log "throwaway CA and a certificate for quomodo.home.arpa"
openssl req -x509 -newkey ec -pkeyopt ec_paramgen_curve:P-256 -nodes -days 7 -subj "/CN=quomodo-demo-ca" \
  -addext "basicConstraints=critical,CA:TRUE" -addext "keyUsage=critical,keyCertSign,cRLSign" \
  -keyout "$WORK/tls/ca.key" -out "$WORK/tls/ca.pem" 2>/dev/null
printf 'subjectAltName=DNS:quomodo.home.arpa,DNS:localhost,IP:127.0.0.1\nbasicConstraints=CA:FALSE\nkeyUsage=critical,digitalSignature\nextendedKeyUsage=serverAuth\n' > "$WORK/tls/ext.cnf"
openssl req -newkey ec -pkeyopt ec_paramgen_curve:P-256 -nodes -subj "/CN=quomodo.home.arpa" \
  -keyout "$WORK/tls/key.pem" -out "$WORK/tls/core.csr" 2>/dev/null
openssl x509 -req -in "$WORK/tls/core.csr" -CA "$WORK/tls/ca.pem" -CAkey "$WORK/tls/ca.key" -CAcreateserial \
  -days 7 -extfile "$WORK/tls/ext.cnf" -out "$WORK/tls/cert.pem" 2>/dev/null
chmod 644 "$WORK/tls/key.pem"

log "stack repo, served as git.home.arpa/homelab/stacks"
H="$WORK/homelab"
mkdir -p "$H"
unit() { mkdir -p "$H/$(dirname "$1")"; cat > "$H/$1"; }

unit gitea/gitea.network <<'EOF'
[Network]
NetworkName=gitea
EOF
printf '[Volume]\n' | unit gitea/gitea-data.volume
printf '[Volume]\n' | unit gitea/gitea-db.volume
unit gitea/gitea-db.container <<'EOF'
[Unit]
Description=Gitea database

[Container]
Image=docker.io/library/postgres:16-alpine
ContainerName=gitea-db
Network=gitea.network
Volume=gitea-db.volume:/var/lib/postgresql/data
Environment=POSTGRES_USER=gitea POSTGRES_DB=gitea
Secret=gitea-db-password,type=env,target=POSTGRES_PASSWORD
HealthCmd=pg_isready -U gitea
HealthInterval=10s
HealthStartupCmd=pg_isready -U gitea
HealthStartupInterval=1s
Notify=healthy

[Service]
Restart=always

[Install]
WantedBy=multi-user.target
EOF
unit gitea/gitea.container <<'EOF'
[Unit]
Description=Gitea
Requires=gitea-db.service
After=gitea-db.service

[Container]
Image=docker.io/gitea/gitea:1.22.4
ContainerName=gitea
Network=gitea.network
Volume=gitea-data.volume:/data
PublishPort=3000:3000
PublishPort=2222:22
Environment=GITEA__database__DB_TYPE=postgres GITEA__database__HOST=gitea-db:5432
Environment=GITEA__database__NAME=gitea GITEA__database__USER=gitea
Secret=gitea-db-password,type=env,target=GITEA__database__PASSWD

[Service]
Restart=always

[Install]
WantedBy=multi-user.target
EOF

unit miniflux/miniflux.pod <<'EOF'
[Pod]
PodName=miniflux
PublishPort=8080:8080
EOF
printf '[Volume]\n' | unit miniflux/miniflux-db.volume
unit miniflux/miniflux-db.container <<'EOF'
[Container]
Image=docker.io/library/postgres:16-alpine
ContainerName=miniflux-db
Pod=miniflux.pod
Volume=miniflux-db.volume:/var/lib/postgresql/data
Environment=POSTGRES_USER=miniflux POSTGRES_DB=miniflux
Secret=miniflux-db-password,type=env,target=POSTGRES_PASSWORD
HealthCmd=pg_isready -U miniflux
HealthInterval=10s
HealthStartupCmd=pg_isready -U miniflux
HealthStartupInterval=1s
Notify=healthy

[Service]
Restart=always

[Install]
WantedBy=multi-user.target
EOF
unit miniflux/miniflux.container <<'EOF'
[Unit]
Requires=miniflux-db.service
After=miniflux-db.service

[Container]
Image=docker.io/miniflux/miniflux:2.2.6
ContainerName=miniflux
Pod=miniflux.pod
Environment=RUN_MIGRATIONS=1 DATABASE_URL_FILE=/run/secrets/miniflux-database-url
Secret=miniflux-database-url

[Service]
Restart=always
RestartSec=5

[Install]
WantedBy=multi-user.target
EOF

printf '[Volume]\n' | unit vaultwarden/vaultwarden-data.volume
unit vaultwarden/vaultwarden.container <<'EOF'
[Container]
Image=docker.io/vaultwarden/server:1.32.7
ContainerName=vaultwarden
Volume=vaultwarden-data.volume:/data
PublishPort=8081:80
Environment=SIGNUPS_ALLOWED=false

[Service]
Restart=always

[Install]
WantedBy=multi-user.target
EOF

printf '[Volume]\n' | unit uptime-kuma/uptime-kuma-data.volume
unit uptime-kuma/uptime-kuma.container <<'EOF'
[Container]
Image=docker.io/louislam/uptime-kuma:1
ContainerName=uptime-kuma
Volume=uptime-kuma-data.volume:/app/data
PublishPort=3001:3001

[Service]
Restart=always

[Install]
WantedBy=multi-user.target
EOF

unit caddy/Caddyfile <<'EOF'
git.home.arpa {
	tls internal
	reverse_proxy atlas.home.arpa:3000
}

rss.home.arpa {
	tls internal
	reverse_proxy atlas.home.arpa:8080
}

status.home.arpa {
	tls internal
	reverse_proxy hermes.home.arpa:3001
}
EOF
printf '[Volume]\n' | unit caddy/caddy-data.volume
unit caddy/caddy.container <<'EOF'
[Container]
Image=docker.io/library/caddy:2-alpine
ContainerName=caddy
Volume=./Caddyfile:/etc/caddy/Caddyfile:ro
Volume=caddy-data.volume:/data
PublishPort=80:80
PublishPort=443:443

[Service]
Restart=always

[Install]
WantedBy=multi-user.target
EOF

printf '[Volume]\n' | unit syncthing/syncthing-data.volume
unit syncthing/syncthing.container <<'EOF'
[Container]
Image=docker.io/syncthing/syncthing:1.29
ContainerName=syncthing
Volume=syncthing-data.volume:/var/syncthing
PublishPort=8384:8384
PublishPort=22000:22000

[Service]
Restart=always

[Install]
WantedBy=multi-user.target
EOF

GIT="git -C $H"
$GIT init -q -b main
$GIT config user.name demo && $GIT config user.email demo@example.com
$GIT add -A && $GIT commit -qm "homelab stacks"
mkdir -p "$WORK/git/homelab"
git clone -q --bare "$H" "$WORK/git/homelab/stacks"
git -C "$WORK/git/homelab/stacks" update-server-info
git -C "$H" remote remove origin 2>/dev/null || true
git -C "$H" remote add origin "$WORK/git/homelab/stacks"
git -C "$H" fetch -q origin && git -C "$H" branch -q -u origin/main
docker run -d --name "$P-git" --network "$NET" --network-alias git.home.arpa \
  -v "$WORK/git:/srv:ro" docker.io/library/caddy:2-alpine caddy file-server --root /srv --listen :80 >/dev/null

log "database and core"
docker run -d --name "$P-db" --network "$NET" --network-alias mongo \
  -e FERRETDB_HANDLER=sqlite ghcr.io/ferretdb/ferretdb:1 >/dev/null
# Core runs as the calling user so the files it writes under $WORK stay removable.
docker run -d --name "$P-core" --network "$NET" --network-alias quomodo.home.arpa \
  -p "127.0.0.1:$CORE_PORT:443" --user "$(id -u):$(id -g)" --sysctl net.ipv4.ip_unprivileged_port_start=0 \
  -v "$CORE_BIN:/usr/local/bin/quomodo:ro" -v "$ROOT/ui/dist:/ui:ro" \
  -v "$WORK/tls:/tls:ro" -v "$WORK/core:/data" \
  -e KOMODO_HOST=https://quomodo.home.arpa -e KOMODO_PORT=443 -e KOMODO_BIND_IP=0.0.0.0 \
  -e KOMODO_DATABASE_ADDRESS=mongo:27017 -e KOMODO_DATABASE_USERNAME= -e KOMODO_DATABASE_PASSWORD= \
  -e KOMODO_JWT_SECRET=demo_secret -e KOMODO_LOCAL_AUTH=true -e KOMODO_ENABLE_NEW_USERS=true \
  -e KOMODO_WEBHOOK_SECRET=$WEBHOOK_SECRET \
  -e KOMODO_UI_PATH=/ui -e KOMODO_PRIVATE_KEY=file:/data/keys/core.key \
  -e KOMODO_REPO_DIRECTORY=/data/repo-cache -e KOMODO_SYNC_DIRECTORY=/data/syncs -e KOMODO_ACTION_DIRECTORY=/data/actions \
  -e KOMODO_SSL_ENABLED=true -e KOMODO_SSL_KEY_FILE=/tls/key.pem -e KOMODO_SSL_CERT_FILE=/tls/cert.pem \
  -e KOMODO_LOGGING_LEVEL=info \
  --entrypoint /usr/local/bin/quomodo quomodo-agent-host >/dev/null
CURL="curl -sf --cacert $WORK/tls/ca.pem"
for _ in $(seq 1 30); do $CURL "$CORE_URL/version" >/dev/null && break; sleep 1; done
$CURL "$CORE_URL/version" >/dev/null || { docker logs --tail 20 "$P-core"; fail "core did not start"; }

# Plain http on loopback for browsers (and playwright) that do not trust the
# throwaway CA: caddy proxies to Core and verifies it with that CA.
printf ':80 {\n\treverse_proxy https://quomodo.home.arpa {\n\t\ttransport http {\n\t\t\ttls_trust_pool file /tls/ca.pem\n\t\t}\n\t}\n}\n' > "$WORK/ui.Caddyfile"
docker run -d --name "$P-ui" --network "$NET" -p "127.0.0.1:$UI_PORT:80" \
  -v "$WORK/ui.Caddyfile:/etc/caddy/Caddyfile:ro" -v "$WORK/tls:/tls:ro" docker.io/library/caddy:2-alpine >/dev/null

cat > "$WORK/api.py" <<PY
import json, ssl, sys, time, urllib.request
BASE = "$CORE_URL"
TLS = ssl.create_default_context(cafile="$WORK/tls/ca.pem")
def post(path, body, jwt=None):
    req = urllib.request.Request(BASE + path, data=json.dumps(body).encode(), headers={"content-type": "application/json", **({"authorization": "Bearer " + jwt} if jwt else {})})
    try:
        with urllib.request.urlopen(req, timeout=120, context=TLS) as r: return json.loads(r.read())
    except urllib.error.HTTPError as e:
        return {"error": e.read().decode()[:500], "http": e.code}
def jwt():
    # Log in once per run: Core rate limits failed logins per address.
    try: return open("$WORK/jwt").read()
    except FileNotFoundError: pass
    for t in ("SignUpLocalUser", "LoginLocalUser"):
        r = post("/auth/login/" + t, {"username": "$USERNAME", "password": "$PASSWORD"})
        j = r.get("jwt") or (r.get("data") or {}).get("jwt")
        if j:
            open("$WORK/jwt", "w").write(j)
            return j
    raise SystemExit("login failed: %r" % r)
J = jwt()
def api(kind, typ, params):
    for _ in range(5):
        r = post("/" + kind, {"type": typ, "params": params}, J)
        if not (isinstance(r, dict) and "Too many attempts" in r.get("error", "")): break
        time.sleep(11)
    if isinstance(r, dict) and "error" in r: raise SystemExit("%s failed: %s" % (typ, r["error"]))
    return r
def wait_update(uid):
    for _ in range(300):
        u = api("read", "GetUpdate", {"id": uid})
        if u.get("status") == "Complete": return u
        time.sleep(1)
    raise SystemExit("update %s did not complete" % uid)
cmd = sys.argv[1]
if cmd == "create-server":
    s = api("write", "CreateServer", {"name": sys.argv[2], "config": {"enabled": True, "region": sys.argv[4]}, "public_key": sys.argv[3]})
    api("write", "UpdateResourceMeta", {"target": {"type": "Server", "id": s["_id"]["\$oid"]}, "description": sys.argv[5]})
elif cmd == "server-state":
    print(api("read", "GetServerState", {"server": sys.argv[2]}).get("status"))
elif cmd == "create-stack":
    s = api("write", "CreateStack", {"name": sys.argv[2], "config": {"server_id": sys.argv[3], "git_provider": "git.home.arpa", "git_https": False, "repo": "homelab/stacks", "branch": "main", "run_directory": sys.argv[2]}})
    api("write", "UpdateResourceMeta", {"target": {"type": "Stack", "id": s["_id"]["\$oid"]}, "description": sys.argv[4]})
    print(s["_id"]["\$oid"])
elif cmd == "deploy":
    u = api("execute", "DeployStack", {"stack": sys.argv[2], "services": []})
    u = wait_update(u.get("id") or u["_id"]["\$oid"])
    if not u["success"]:
        for l in u["logs"]:
            if not l["success"]: print("--", l["stage"], "\n", l["stdout"][-1500:], "\n", l["stderr"][-1500:])
        raise SystemExit("DeployStack %s failed" % sys.argv[2])
    print("deployed", sys.argv[2])
elif cmd == "state":
    print(api("read", "ListStacks", {"query": {"names": [sys.argv[2]]}})[0]["info"]["state"])
PY
API="python3 $WORK/api.py"

log "podman hosts: ${SERVERS[*]}"
for s in "${SERVERS[@]}"; do
  docker run -d --name "$P-$s" --hostname "$s" --network "$NET" --network-alias "$s.home.arpa" \
    --privileged --cgroupns=private --tmpfs /run --tmpfs /tmp quomodo-agent-host >/dev/null
done
sleep 4
for s in "${SERVERS[@]}"; do
  c="$P-$s"
  docker cp "$AGENT_BIN" "$c:/usr/local/bin/quomodo-agent"
  docker exec "$c" mkdir -p /etc/quomodo/keys /var/lib/quomodo
  docker cp "$WORK/tls/ca.pem" "$c:/etc/quomodo/ca.pem"
  docker exec "$c" sh -c "cat > /etc/quomodo/config.toml <<CFG
core_addresses = [\"wss://quomodo.home.arpa\"]
core_tls_ca_file = \"/etc/quomodo/ca.pem\"
connect_as = \"$s\"
CFG
cat > /etc/systemd/system/quomodo-agent.service <<UNIT
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
systemctl daemon-reload && systemctl enable --now quomodo-agent"
  $API create-server "$s" "$(docker exec "$c" /usr/local/bin/quomodo-agent --config-path /etc/quomodo/config.toml public-key)" "${REGION[$s]}" "${ABOUT[$s]}"
done
for s in "${SERVERS[@]}"; do
  for _ in $(seq 1 30); do [ "$($API server-state "$s")" = "Ok" ] && break; sleep 1; done
  [ "$($API server-state "$s")" = "Ok" ] || { docker exec "$P-$s" journalctl -u quomodo-agent --no-pager | tail -20; fail "$s never reached Ok"; }
  echo "$s Ok"
done

log "rootless agents: ${ROOTLESS[*]}"
# Runs a command on a Server's host as the user its agent runs as.
declare -A RL_HOST=() RL_USER=()
on() {
  local server=$1; shift
  if [ -z "${RL_USER[$server]:-}" ]; then docker exec -i "$P-$server" "$@"; return; fi
  local u=${RL_USER[$server]}
  docker exec -i "$P-${RL_HOST[$server]}" runuser -u "$u" -- \
    env HOME="/home/$u" XDG_RUNTIME_DIR="/run/user/$(docker exec "$P-${RL_HOST[$server]}" id -u "$u")" "$@"
}
for entry in "${ROOTLESS[@]}"; do
  IFS=: read -r server host u <<<"$entry"
  RL_HOST[$server]=$host; RL_USER[$server]=$u
  c="$P-$host"
  docker exec "$c" sh -c "useradd -m $u && usermod --add-subuids 6000000-10194303 --add-subgids 6000000-10194303 $u && loginctl enable-linger $u"
  uid=$(docker exec "$c" id -u "$u")
  for _ in $(seq 1 30); do docker exec "$c" systemctl is-active --quiet "user@$uid.service" && break; sleep 1; done
  docker exec "$c" systemctl is-active --quiet "user@$uid.service" || fail "lingering did not start $u's systemd on $host"
  docker exec "$c" sh -c "mkdir -p /home/$u/.config/quomodo /home/$u/.config/systemd/user
cp /etc/quomodo/ca.pem /home/$u/.config/quomodo/ca.pem
cat > /home/$u/.config/quomodo/config.toml <<CFG
core_addresses = [\"wss://quomodo.home.arpa\"]
core_tls_ca_file = \"/home/$u/.config/quomodo/ca.pem\"
connect_as = \"$server\"
CFG
cat > /home/$u/.config/systemd/user/quomodo-agent.service <<'UNIT'
[Unit]
Description=Quomodo agent (rootless)
[Service]
ExecStart=/usr/local/bin/quomodo-agent --config-path %h/.config/quomodo/config.toml
Restart=always
RestartSec=2
[Install]
WantedBy=default.target
UNIT
chown -R $u:$u /home/$u/.config"
  on "$server" systemctl --user daemon-reload
  on "$server" systemctl --user enable --now quomodo-agent
  $API create-server "$server" "$(on "$server" /usr/local/bin/quomodo-agent --config-path "/home/$u/.config/quomodo/config.toml" public-key)" "${REGION[$host]}" "${ABOUT[$server]}"
done
for entry in "${ROOTLESS[@]}"; do
  server=${entry%%:*}
  for _ in $(seq 1 30); do [ "$($API server-state "$server")" = "Ok" ] && break; sleep 1; done
  [ "$($API server-state "$server")" = "Ok" ] || { on "$server" journalctl --user -u quomodo-agent --no-pager | tail -20; fail "$server never reached Ok"; }
  echo "$server Ok (rootless)"
done

log "images and podman secrets on the hosts"
declare -A LOADED=()
for entry in "${STACKS[@]}"; do
  server=$(echo "$entry" | cut -d: -f2)
  for img in ${entry#*:*:}; do
    [ -n "${LOADED[$server/$img]:-}" ] && continue
    on "$server" podman load -q < "$(image_tar "$img")" >/dev/null
    LOADED[$server/$img]=1
  done
  echo "$server: loaded images for ${entry%%:*}"
done
secret() { docker exec -i "$P-$1" podman secret create "$2" - >/dev/null; }
GITEA_PW=$(openssl rand -hex 16); MINIFLUX_PW=$(openssl rand -hex 16)
printf '%s' "$GITEA_PW" | secret atlas gitea-db-password
printf '%s' "$MINIFLUX_PW" | secret atlas miniflux-db-password
printf 'postgres://miniflux:%s@127.0.0.1/miniflux?sslmode=disable' "$MINIFLUX_PW" | secret atlas miniflux-database-url
unset GITEA_PW MINIFLUX_PW

log "stacks"
: > "$WORK/stack-ids"
for entry in "${STACKS[@]}"; do
  name=${entry%%:*}; server=$(echo "$entry" | cut -d: -f2)
  $API create-stack "$name" "$server" "${ABOUT[$name]}" >> "$WORK/stack-ids"
done
for entry in "${STACKS[@]}"; do $API deploy "${entry%%:*}"; done

log "post-receive hook: every push goes to each Stack's webhook"
cat > "$WORK/git/homelab/stacks/hooks/post-receive" <<HOOK
#!/bin/sh
git update-server-info
body='{"ref":"refs/heads/main"}'
sig=\$(printf '%s' "\$body" | openssl dgst -sha256 -hmac '$WEBHOOK_SECRET' -r | cut -d' ' -f1)
while read -r id; do
  curl -sf --cacert '$WORK/tls/ca.pem' -H 'content-type: application/json' \\
    -H "x-hub-signature-256: sha256=\$sig" -d "\$body" \\
    '$CORE_URL/listener/github/stack/'"\$id"'/deploy' >/dev/null || echo "webhook for \$id failed"
done < '$WORK/stack-ids'
HOOK
chmod +x "$WORK/git/homelab/stacks/hooks/post-receive"

for entry in "${STACKS[@]}"; do
  name=${entry%%:*}
  for _ in $(seq 1 60); do [ "$($API state "$name")" = "running" ] && break; sleep 2; done
  echo "$name: $($API state "$name")"
done

log "ready"
cat <<MSG
UI:     http://127.0.0.1:$UI_PORT  (login $USERNAME / $PASSWORD)
        $CORE_URL with the CA in $WORK/tls/ca.pem
Repo:   $H  (edit, commit, git push: the hook deploys)
Hosts:  docker exec -it $P-atlas podman ps
MSG

if [ "${KEEP:-0}" != "1" ]; then
  echo "Ctrl-C tears it all down."
  sleep infinity & SLEEP_PID=$!
  wait "$SLEEP_PID"
fi
