#!/usr/bin/env bash
# Starts a Core image against a throwaway database and waits for /version
# to answer. The release workflow runs it on each architecture's own runner
# before pushing; run it locally the same way:
#
#   scripts/core-image-smoke.sh ghcr.io/loganrupe/quomodo:latest
#
# Needs docker. Uses the container names quomodo-smoke-{core,db} and the
# network quomodo-smoke, and removes them on exit. The database is FerretDB,
# as in scripts/e2e.sh: mongo 8 refuses to start on Linux 6.19 and newer.
set -euo pipefail

IMAGE=${1:?usage: $0 <image>}
PORT=${SMOKE_PORT:-9120}
NET=quomodo-smoke

cleanup() {
  docker rm -f quomodo-smoke-core quomodo-smoke-db >/dev/null 2>&1 || true
  docker network rm "$NET" >/dev/null 2>&1 || true
}
trap cleanup EXIT
cleanup

echo "image architecture: $(docker image inspect --format '{{.Os}}/{{.Architecture}}' "$IMAGE")"
docker run --rm --entrypoint uname "$IMAGE" -m
docker run --rm --entrypoint km "$IMAGE" --version

docker network create "$NET" >/dev/null
docker run -d --name quomodo-smoke-db --network "$NET" \
  -e FERRETDB_HANDLER=sqlite ghcr.io/ferretdb/ferretdb:1 >/dev/null
docker run -d --name quomodo-smoke-core --network "$NET" -p "127.0.0.1:$PORT:9120" \
  -e KOMODO_HOST="http://127.0.0.1:$PORT" \
  -e KOMODO_DATABASE_ADDRESS=quomodo-smoke-db:27017 \
  -e KOMODO_DATABASE_USERNAME= -e KOMODO_DATABASE_PASSWORD= \
  -e KOMODO_JWT_SECRET=smoke -e KOMODO_LOCAL_AUTH=true \
  "$IMAGE" >/dev/null

for _ in $(seq 1 60); do
  if VERSION=$(curl -sf "http://127.0.0.1:$PORT/version"); then
    echo "core answered /version: $VERSION"
    exit 0
  fi
  sleep 2
done
docker logs --tail 40 quomodo-smoke-core
echo "core did not answer /version within 120s" >&2
exit 1
