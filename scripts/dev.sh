#!/usr/bin/env bash
# Run backend, agent runtime and frontend together with prefixed logs. Ctrl-C stops everything.
# Usage: scripts/dev.sh   (normally via `make dev`, which starts PostgreSQL first)
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

start runtime 35 bash -c 'cd agent-runtime && exec uv run nexc-runtime'
start backend 36 cargo run --manifest-path backend/Cargo.toml -- serve
start web 32 "$PNPM" --dir frontend dev

echo "nexc dev: web http://localhost:5173  api http://localhost:${NEXC_PORT:-8080}  (Ctrl-C to stop)"

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
