#!/usr/bin/env bash
# make test-onto-live: the backend against a real onto-intelli, not the stub CI uses.
#
# Starts `onto serve` from the sibling checkout (../onto-intelli, built first: a no-op when it is
# up to date, so a binary from before the checkout's last change is never used; ONTO_BIN names a
# binary to use as it is) with the configuration nexc ships (deploy/onto), on a throwaway
# PostgreSQL with pgvector, then runs backend/tests/onto_live.rs against it. Everything it started
# is removed when it ends, pass or fail.
#
#   scripts/test-onto-live.sh                 # everything on free local ports
#   ONTO_LIVE_DATABASE_URL=postgres://… scripts/test-onto-live.sh
#                                             # a PostgreSQL 16+ with pgvector you run: onto-intelli
#                                             # gets the database `onto` there (created when missing,
#                                             # with psql), the tests their own
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

ONTO_DIR="${ONTO_DIR:-$ROOT/../onto-intelli}"
build_onto=""
if [[ -z "${ONTO_BIN:-}" ]]; then
  build_onto=1
  ONTO_BIN="$ONTO_DIR/target/release/onto"
fi
PORT="${ONTO_LIVE_PORT:-50060}"
PG_PORT="${ONTO_LIVE_PG_PORT:-55462}"
PG_IMAGE="${ONTO_LIVE_PG_IMAGE:-nexc-postgres-pgvector:17}"
CONTAINER="nexc-onto-live-$$"

for tool in cargo curl openssl; do
  command -v "$tool" >/dev/null 2>&1 || { echo "error: '$tool' is required" >&2; exit 1; }
done

if [[ -n "$build_onto" ]]; then
  if [[ ! -d "$ONTO_DIR" ]]; then
    echo "error: no onto-intelli at $ONTO_DIR: clone https://github.com/f8fvwgzc/onto-intelli next to this repository, or set ONTO_BIN" >&2
    exit 1
  fi
  echo "building onto-intelli in $ONTO_DIR (nothing to do when it is up to date)"
  (cd "$ONTO_DIR" && cargo build --release --locked -p onto-intelli-cli)
elif [[ ! -x "$ONTO_BIN" ]]; then
  echo "error: ONTO_BIN=$ONTO_BIN is not an executable" >&2
  exit 1
fi

work="$(mktemp -d)"
onto_pid=""
started_db=""
# shellcheck disable=SC2317,SC2329 # run by the EXIT trap (0.9 calls it SC2317, 0.10+ SC2329)
cleanup() {
  if [[ -n "$onto_pid" ]]; then
    kill "$onto_pid" 2>/dev/null || true
    wait "$onto_pid" 2>/dev/null || true
  fi
  if [[ -n "$started_db" ]]; then docker rm -f "$CONTAINER" >/dev/null 2>&1 || true; fi
  rm -rf "$work"
}
trap cleanup EXIT

if [[ -n "${ONTO_LIVE_DATABASE_URL:-}" ]]; then
  base="${ONTO_LIVE_DATABASE_URL%/*}"
  test_db="$ONTO_LIVE_DATABASE_URL"
else
  command -v docker >/dev/null 2>&1 || { echo "error: docker is required (or set ONTO_LIVE_DATABASE_URL)" >&2; exit 1; }
  if ! docker image inspect "$PG_IMAGE" >/dev/null 2>&1; then
    echo "building $PG_IMAGE from deploy/postgres/pgvector.Dockerfile"
    docker build -t "$PG_IMAGE" -f deploy/postgres/pgvector.Dockerfile deploy/postgres
  fi
  password="$(openssl rand -hex 12)"
  docker run -d --rm --name "$CONTAINER" -e POSTGRES_USER=nexc -e POSTGRES_PASSWORD="$password" \
    -e POSTGRES_DB=nexc -p "127.0.0.1:$PG_PORT:5432" "$PG_IMAGE" >/dev/null
  started_db=1
  for _ in $(seq 1 60); do
    docker exec "$CONTAINER" pg_isready -U nexc -d nexc -h 127.0.0.1 >/dev/null 2>&1 && break
    sleep 1
  done
  base="postgres://nexc:$password@127.0.0.1:$PG_PORT"
  test_db="$base/nexc"
fi

# onto-intelli's own database on the same server.
if [[ -n "$started_db" ]]; then
  docker exec "$CONTAINER" psql -U nexc -d nexc -tAc "CREATE DATABASE onto" >/dev/null
else
  command -v psql >/dev/null 2>&1 || {
    echo "error: psql is required to create the database onto at ${base##*@} (or create it yourself)" >&2
    exit 1
  }
  psql "$base/postgres" -tAc "SELECT 1 FROM pg_database WHERE datname = 'onto'" | grep -q 1 ||
    psql "$base/postgres" -qc "CREATE DATABASE onto"
fi

cp deploy/onto/nexc-audit.toml deploy/onto/nexc-workspace.toml "$work/"
cp deploy/onto/onto-intelli.toml "$work/onto-intelli.toml"
NEXC_ONTO_TOKEN="$(openssl rand -hex 24)"
ONTO_SESSION_SECRET="$(openssl rand -hex 32)"
ONTO_DATABASE_URL="$base/onto"
export NEXC_ONTO_TOKEN ONTO_SESSION_SECRET ONTO_DATABASE_URL

"$ONTO_BIN" serve --config "$work/onto-intelli.toml" --listen "127.0.0.1:$PORT" >"$work/onto.log" 2>&1 &
onto_pid=$!
url="http://127.0.0.1:$PORT"
for _ in $(seq 1 60); do
  if curl -fsS -H "authorization: Bearer $NEXC_ONTO_TOKEN" "$url/healthz" >/dev/null 2>&1; then
    break
  fi
  if ! kill -0 "$onto_pid" 2>/dev/null; then
    echo "error: onto serve exited:" >&2
    cat "$work/onto.log" >&2
    exit 1
  fi
  sleep 1
done
echo "onto serve on $url ($("$ONTO_BIN" --version)); PostgreSQL ${base##*@}"

status=0
(
  cd backend
  NEXC_ONTO_LIVE_URL="$url" NEXC_ONTO_LIVE_TOKEN="$NEXC_ONTO_TOKEN" DATABASE_URL="$test_db" \
    cargo test --locked --test onto_live -- --ignored --nocapture --test-threads=1
) || status=$?
if [[ "$status" != 0 ]]; then
  echo "--- onto serve log (last 80 lines) ---" >&2
  tail -n 80 "$work/onto.log" >&2
fi
exit "$status"
