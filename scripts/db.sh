#!/usr/bin/env bash
# Local PostgreSQL 17 for development (docker, localhost:5432, named volume).
#   scripts/db.sh up | down | shell | status
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

CONTAINER="${NEXC_DB_CONTAINER:-nexc-postgres}"
VOLUME="${NEXC_DB_VOLUME:-nexc-pgdata-dev}"
IMAGE="postgres:17-alpine"

if [[ ! -f .env ]]; then
  echo "error: .env not found - run \`make init\` first" >&2
  exit 1
fi
set -a
# shellcheck disable=SC1091
. ./.env
set +a
: "${POSTGRES_PASSWORD:?POSTGRES_PASSWORD is empty - run make init}"
POSTGRES_USER="${POSTGRES_USER:-nexc}"
POSTGRES_DB="${POSTGRES_DB:-nexc}"
PORT="${NEXC_DB_PORT:-5432}"

exists() { docker container inspect "$CONTAINER" >/dev/null 2>&1; }
running() { [[ "$(docker container inspect -f '{{.State.Running}}' "$CONTAINER" 2>/dev/null)" == "true" ]]; }

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
      docker run -d --name "$CONTAINER" \
        -p "127.0.0.1:$PORT:5432" \
        -e POSTGRES_USER="$POSTGRES_USER" \
        -e POSTGRES_PASSWORD="$POSTGRES_PASSWORD" \
        -e POSTGRES_DB="$POSTGRES_DB" \
        -v "$VOLUME":/var/lib/postgresql/data \
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
