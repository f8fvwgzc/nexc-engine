#!/usr/bin/env bash
# Create (or complete) an env file and generate every missing secret. Idempotent: values that are
# already set are never overwritten.
#
#   scripts/init-env.sh          # .env for local dev and Docker Compose (from .env.example)
#   scripts/init-env.sh --k8s    # deploy/k8s/secret.env for Kubernetes (from secret.example.env)
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

TEMPLATE=.env.example
OUT=.env
DB_HOST=localhost
if [[ "${1:-}" == "--k8s" ]]; then
  TEMPLATE=deploy/k8s/secret.example.env
  OUT=deploy/k8s/secret.env
  DB_HOST=postgres
fi

command -v openssl >/dev/null 2>&1 || { echo "error: openssl is required" >&2; exit 1; }

if [[ ! -f "$OUT" ]]; then
  cp "$TEMPLATE" "$OUT"
  echo "created $OUT from $TEMPLATE"
fi
chmod 600 "$OUT"

get() {
  # Last assignment wins, like a shell would; prints nothing when the key is absent.
  grep -E "^$1=" "$OUT" | tail -n 1 | cut -d= -f2- || true
}

put() {
  local key="$1" value="$2" tmp
  tmp="$(mktemp "${OUT}.XXXXXX")"
  if grep -qE "^${key}=" "$OUT"; then
    KEY="$key" VALUE="$value" awk 'BEGIN { k = ENVIRON["KEY"]; v = ENVIRON["VALUE"] }
      index($0, k "=") == 1 { print k "=" v; next } { print }' "$OUT" > "$tmp"
  else
    cat "$OUT" > "$tmp"
    printf '%s=%s\n' "$key" "$value" >> "$tmp"
  fi
  chmod 600 "$tmp"
  mv "$tmp" "$OUT"
}

fill() {
  if [[ -z "$(get "$1")" ]]; then
    put "$1" "$2"
    echo "  generated $1"
  fi
}

echo "completing $OUT:"
fill NEXC_JWT_SECRET "$(openssl rand -hex 32)"
fill NEXC_MASTER_KEY "$(openssl rand -base64 32)"
fill NEXC_RUNTIME_TOKEN "$(openssl rand -hex 32)"
fill NEXC_METRICS_TOKEN "$(openssl rand -hex 32)"
fill NEXC_ONTO_TOKEN "$(openssl rand -hex 32)"
# Signs onto-intelli's browser sessions, the same on every onto-intelli replica (32+ characters).
fill ONTO_SESSION_SECRET "$(openssl rand -hex 32)"
# onto-intelli's own PostgreSQL login: never nexc's database credentials.
fill ONTO_POSTGRES_PASSWORD "$(openssl rand -hex 24)"
fill POSTGRES_USER nexc
fill POSTGRES_DB nexc
fill POSTGRES_PASSWORD "$(openssl rand -hex 24)"

DB_PORT=5432
if [[ "$OUT" == ".env" ]]; then
  DB_PORT="$(get NEXC_DB_PORT)"
  DB_PORT="${DB_PORT:-5432}"
fi

if [[ "$OUT" == ".env" ]]; then
  # Where the runtime of `make dev` fetches the backend's agent-token keys (an .env written before
  # agent tokens were Ed25519 has no such line, and the runtime does not start without it).
  API_PORT="$(get NEXC_PORT)"
  fill NEXC_AGENT_TOKEN_JWKS_URL "http://localhost:${API_PORT:-8080}/api/v1/agent-tokens/jwks.json"
fi

if [[ -z "$(get NEXC_DATABASE_URL)" ]]; then
  put NEXC_DATABASE_URL \
    "postgres://$(get POSTGRES_USER):$(get POSTGRES_PASSWORD)@${DB_HOST}:${DB_PORT}/$(get POSTGRES_DB)"
  echo "  wrote NEXC_DATABASE_URL (${DB_HOST}:${DB_PORT})"
fi

# Demo mode is the default until an Anthropic key is configured.
if [[ -z "$(get ANTHROPIC_API_KEY)" && -z "$(get NEXC_LLM_PROVIDER)" ]]; then
  put NEXC_LLM_PROVIDER demo
  echo "  NEXC_LLM_PROVIDER=demo (no ANTHROPIC_API_KEY yet)"
fi

if [[ "$OUT" == ".env" ]]; then
  mkdir -p data/symphony
  provider="$(get NEXC_LLM_PROVIDER)"
  echo
  echo "done. LLM provider: ${provider:-anthropic}."
  if [[ "$provider" == "demo" ]]; then
    echo "  Demo mode needs no API key. For real results set ANTHROPIC_API_KEY in .env and"
    echo "  NEXC_LLM_PROVIDER=anthropic (or add your own key in the app under Settings -> LLM)."
  fi
  echo "next: make setup && make dev   |   docker compose up -d --build"
fi
