#!/usr/bin/env bash
# Local PostgreSQL 17 for development (docker, localhost:5432, named volume).
#   scripts/db.sh up | down | shell | status
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

if [[ ! -f .env ]]; then
  echo "error: .env not found - run \`make init\` first" >&2
  exit 1
fi
set -a
# shellcheck disable=SC1091
. ./.env
set +a
: "${POSTGRES_PASSWORD:?POSTGRES_PASSWORD is empty - run make init}"
# Read after .env, so that a choice made there holds for every `make dev`.
CONTAINER="${NEXC_DB_CONTAINER:-nexc-postgres}"
VOLUME="${NEXC_DB_VOLUME:-nexc-pgdata-dev}"
# NEXC_DB_IMAGE=pgvector/pgvector:pg17 gives memory and document search an HNSW index. Use it
# with a NEW volume (NEXC_DB_VOLUME) only: the stock image is Alpine (musl) and pgvector's is
# Debian (glibc), and text indexes built under one are not valid under the other. To move
# existing data, dump it (pg_dump -Fc) and restore it into the new volume.
IMAGE="${NEXC_DB_IMAGE:-postgres:17-alpine}"
POSTGRES_USER="${POSTGRES_USER:-nexc}"
POSTGRES_DB="${POSTGRES_DB:-nexc}"
PORT="${NEXC_DB_PORT:-5432}"

exists() { docker container inspect "$CONTAINER" >/dev/null 2>&1; }
running() { [[ "$(docker container inspect -f '{{.State.Running}}' "$CONTAINER" 2>/dev/null)" == "true" ]]; }

# `docker run` pulls an image it does not have, and the repo's own pgvector image
# (nexc-postgres-pgvector:<tag>, deploy/postgres/pgvector.Dockerfile) is on no registry: without
# this, a machine where it was never built (or was pruned) fails with "pull access denied".
ensure_image() {
  if docker image inspect "$IMAGE" >/dev/null 2>&1; then
    return 0
  fi
  if [[ "$IMAGE" == nexc-postgres-pgvector:* ]]; then
    echo "image $IMAGE is not on this machine; building it from deploy/postgres/pgvector.Dockerfile"
    docker build -t "$IMAGE" -f deploy/postgres/pgvector.Dockerfile deploy/postgres
  elif ! docker pull "$IMAGE"; then
    echo "error: image $IMAGE is neither on this machine nor on a registry; check NEXC_DB_IMAGE in .env" >&2
    exit 1
  fi
}

case "${1:-}" in
  up)
    if running; then
      echo "postgres is already running ($CONTAINER)"
    elif exists; then
      docker start "$CONTAINER" >/dev/null
    else
      if (exec 3<>"/dev/tcp/127.0.0.1/$PORT") 2>/dev/null; then
        echo "error: localhost:$PORT is already in use (another PostgreSQL?)." >&2
        echo "  Either stop it, or set NEXC_DB_PORT=<free port> in .env and update the port in" >&2
        echo "  NEXC_DATABASE_URL (or clear NEXC_DATABASE_URL and re-run make init)." >&2
        exit 1
      fi
      ensure_image
      docker run -d --name "$CONTAINER" \
        -p "127.0.0.1:$PORT:5432" \
        -e POSTGRES_USER="$POSTGRES_USER" \
        -e POSTGRES_PASSWORD="$POSTGRES_PASSWORD" \
        -e POSTGRES_DB="$POSTGRES_DB" \
        -v "$VOLUME":/var/lib/postgresql/data \
        --shm-size=1g \
        --health-cmd "pg_isready -U $POSTGRES_USER -d $POSTGRES_DB" \
        --health-interval 2s \
        "$IMAGE" >/dev/null
    fi
    printf "waiting for postgres"
    for _ in $(seq 1 60); do
      if docker exec "$CONTAINER" pg_isready -U "$POSTGRES_USER" -d "$POSTGRES_DB" -h 127.0.0.1 >/dev/null 2>&1; then
        echo " ready on localhost:$PORT (db $POSTGRES_DB, user $POSTGRES_USER)"
        exit 0
      fi
      printf "."
      sleep 1
    done
    echo
    echo "error: postgres did not become ready; see: docker logs $CONTAINER" >&2
    exit 1
    ;;
  down)
    if exists; then
      docker rm -f "$CONTAINER" >/dev/null
      echo "postgres stopped (data kept in volume $VOLUME; remove it with: docker volume rm $VOLUME)"
    else
      echo "postgres is not running"
    fi
    ;;
  shell)
    running || { echo "postgres is not running - make db-up" >&2; exit 1; }
    exec docker exec -it "$CONTAINER" psql -U "$POSTGRES_USER" -d "$POSTGRES_DB"
    ;;
  status)
    if running; then echo "running"; else echo "stopped"; fi
    ;;
  *)
    echo "usage: $0 up|down|shell|status" >&2
    exit 2
    ;;
esac
