#!/usr/bin/env bash
# Check local prerequisites, then run the backend's own `nexc doctor` against .env.
set -uo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT" || exit 1

status=0
ok() { printf '  \033[32mok\033[0m    %s\n' "$*"; }
warn() { printf '  \033[33mwarn\033[0m  %s\n' "$*"; }
fail() { printf '  \033[31mfail\033[0m  %s\n' "$*"; status=1; }

check() {
  local tool="$1" hint="$2"
  if command -v "$tool" >/dev/null 2>&1; then
    ok "$tool: $("$tool" --version 2>/dev/null | head -n 1)"
  else
    fail "$tool not found - $hint"
  fi
}

echo "tooling:"
check cargo "install Rust 1.99+ from https://rustup.rs"
check cc "install a C compiler (Xcode CLT / build-essential); the backend compiles C kernels"
check node "install Node.js 22.12+"
check "${PNPM:-pnpm}" "corepack enable  (or: npm i -g pnpm@10)"
check uv "install uv: https://docs.astral.sh/uv/"
check docker "install Docker (PostgreSQL for dev, Compose, images)"
check openssl "needed by make init to generate secrets"

echo "configuration:"
if [[ -f .env ]]; then
  ok ".env present"
  set -a
  # shellcheck disable=SC1091
  . ./.env
  set +a
  for key in NEXC_JWT_SECRET NEXC_MASTER_KEY NEXC_RUNTIME_TOKEN POSTGRES_PASSWORD NEXC_DATABASE_URL; do
    if [[ -n "${!key:-}" ]]; then ok "$key set"; else fail "$key empty - run make init"; fi
  done
  if [[ "${NEXC_LLM_PROVIDER:-anthropic}" == "anthropic" && -z "${ANTHROPIC_API_KEY:-}" ]]; then
    warn "NEXC_LLM_PROVIDER=anthropic but no server key: users must add their own key in Settings"
  else
    ok "LLM provider: ${NEXC_LLM_PROVIDER:-anthropic}"
  fi
else
  fail ".env missing - run make init"
fi

if [[ "$(scripts/db.sh status 2>/dev/null)" == "running" ]]; then
  ok "dev PostgreSQL running on localhost:5432"
else
  warn "dev PostgreSQL not running - make db-up"
fi

if [[ -f .env ]] && command -v cargo >/dev/null 2>&1; then
  echo "backend (nexc doctor):"
  cargo run --quiet --manifest-path backend/Cargo.toml -- doctor || status=1
fi
exit "$status"
