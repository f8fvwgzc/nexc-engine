#!/usr/bin/env bash
# Run backend, agent runtime and frontend together with prefixed logs. Ctrl-C stops everything.
# Usage: scripts/dev.sh   (normally via `make dev`, which starts PostgreSQL first)
#
# With NEXC_ONTO_MODE=service (the default when NEXC_ONTO_URL is set) and onto-intelli checked out
# next to this repository (../onto-intelli, or ONTO_DIR), `onto serve` runs too, on the dev
# database (its own database `onto`). The dev PostgreSQL needs pgvector for it (NEXC_DB_IMAGE in
# .env); without, onto-intelli is not started and the backend runs with NEXC_ONTO_MODE=off, rather
# than onto serve exiting and taking the others down.
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

PNPM="${PNPM:-pnpm}"
for tool in cargo uv "$PNPM"; do
  command -v "$tool" >/dev/null 2>&1 || { echo "error: '$tool' is not installed (see README)" >&2; exit 1; }
done

set -m # each background job gets its own process group, so we can stop whole trees
pids=()

start() {
  local name="$1" color="$2"
  shift 2
  (
    "$@" 2>&1 | awk -v p="$(printf '\033[%sm%-8s|\033[0m ' "$color" "$name")" \
      '{ print p $0; fflush() }'
  ) &
  pids+=("$!")
}

stop() {
  trap - INT TERM EXIT
  echo
  echo "stopping..."
  for pid in ${pids[@]+"${pids[@]}"}; do
    kill -TERM -- "-$pid" 2>/dev/null || true
  done
  sleep 1
  for pid in ${pids[@]+"${pids[@]}"}; do
    kill -KILL -- "-$pid" 2>/dev/null || true
  done
}
trap stop INT TERM EXIT

# onto-intelli: where graph vocabularies, the workspace map and schema proposals live.
ONTO_DIR="${ONTO_DIR:-$ROOT/../onto-intelli}"
onto_mode="${NEXC_ONTO_MODE:-}"
if [[ -z "$onto_mode" ]]; then
  if [[ -n "${NEXC_ONTO_URL:-}" ]]; then onto_mode=service; else onto_mode=off; fi
fi
onto_note=""
db_container="${NEXC_DB_CONTAINER:-nexc-postgres}"
dev_sql() {
  docker exec "$db_container" psql -U "${POSTGRES_USER:-nexc}" -d "${POSTGRES_DB:-nexc}" "$@"
}
if [[ "$onto_mode" == service && -d "$ONTO_DIR" ]] &&
  ! dev_sql -tAc "SELECT 1 FROM pg_available_extensions WHERE name = 'vector'" 2>/dev/null | grep -q 1; then
  echo "onto-intelli: not started: the dev PostgreSQL ($db_container, ${NEXC_DB_IMAGE:-postgres:17-alpine})"
  echo "  has no pgvector, which onto-intelli needs. Set NEXC_DB_IMAGE=pgvector/pgvector:pg17 with a NEW"
  echo "  NEXC_DB_VOLUME in .env (move existing data with pg_dump and pg_restore; see scripts/db.sh),"
  echo "  or NEXC_ONTO_MODE=off. The backend runs with NEXC_ONTO_MODE=off for now: read-only starter"
  echo "  vocabulary, no workspace map."
  export NEXC_ONTO_MODE=off
elif [[ "$onto_mode" == service && -d "$ONTO_DIR" ]]; then
  : "${NEXC_ONTO_TOKEN:?NEXC_ONTO_TOKEN is empty - run make init}"
  : "${ONTO_SESSION_SECRET:?ONTO_SESSION_SECRET is empty - run make init}"
  onto_port="${ONTO_PORT:-50051}"
  export NEXC_ONTO_MODE=service
  export NEXC_ONTO_URL="${NEXC_ONTO_URL:-http://127.0.0.1:$onto_port}"
  export NEXC_ONTO_PUBLIC_URL="${NEXC_ONTO_PUBLIC_URL:-$NEXC_ONTO_URL}"
  dev_sql -tAc "SELECT 1 FROM pg_database WHERE datname = 'onto'" | grep -q 1 ||
    dev_sql -c "CREATE DATABASE onto" >/dev/null
  export ONTO_DATABASE_URL="${ONTO_DATABASE_URL:-${NEXC_DATABASE_URL%/*}/onto}"
  # The shipped configuration, with the dev web server's origin allowed to embed the UI.
  mkdir -p data/onto
  cp deploy/onto/nexc-audit.toml deploy/onto/nexc-workspace.toml data/onto/
  sed 's|^embed_origins = .*|embed_origins = ["http://localhost:5173", "http://127.0.0.1:5173", "http://localhost:8080"]|' \
    deploy/onto/onto-intelli.toml >data/onto/onto-intelli.toml
  start onto 33 cargo run --release --quiet --manifest-path "$ONTO_DIR/Cargo.toml" -p onto-intelli-cli -- \
    serve --config data/onto/onto-intelli.toml --listen "127.0.0.1:$onto_port"
  onto_note="  onto-intelli $NEXC_ONTO_URL/ui/"
elif [[ "$onto_mode" == service ]]; then
  echo "onto-intelli: $ONTO_DIR not found; the backend uses NEXC_ONTO_URL=${NEXC_ONTO_URL:-} as it is."
  echo "  Clone https://github.com/f8fvwgzc/onto-intelli next to this repository to run it here,"
  echo "  or set NEXC_ONTO_MODE=off in .env to run without it (read-only starter vocabulary, no map)."
else
  echo "onto-intelli: off (NEXC_ONTO_MODE=off): graphs keep the read-only starter vocabulary and the"
  echo "  workspace map is off. With ../onto-intelli checked out, set NEXC_ONTO_MODE=service in .env."
fi

start runtime 35 bash -c 'cd agent-runtime && exec uv run nexc-runtime'
start backend 36 cargo run --manifest-path backend/Cargo.toml -- serve
start web 32 "$PNPM" --dir frontend dev

echo "nexc dev: web http://localhost:5173  api http://localhost:${NEXC_PORT:-8080}${onto_note}  (Ctrl-C to stop)"

# Stop everything as soon as one process exits (bash 3.2 compatible; no `wait -n`).
while true; do
  for pid in ${pids[@]+"${pids[@]}"}; do
    if ! kill -0 "$pid" 2>/dev/null; then
      echo "a dev process exited; shutting down the others"
      exit 1
    fi
  done
  sleep 1
done
