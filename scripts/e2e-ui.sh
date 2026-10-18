#!/usr/bin/env bash
# The SPA's UI suite (frontend/e2e): every route against the mocked API, at 375, 768 and 1280 px,
# with the layout check, axe and the screenshot baselines.
#   scripts/e2e-ui.sh [playwright args]        e.g. --update-snapshots, -g issues
#
# It runs in the Playwright image CI runs (pinned to the @playwright/test version, linux/amd64),
# so the screenshots compare pixel for pixel; on an arm64 machine that image is emulated and
# slower (E2E_PLATFORM=linux/arm64 runs it natively, within the comparison threshold). Without
# Docker, or with E2E_NATIVE=1, it runs on this machine instead; the screenshot comparisons
# are then skipped off Linux (fonts render differently), everything else is checked.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT/frontend"

VERSION="$(sed -n 's/.*"@playwright\/test": "\([0-9.]*\)".*/\1/p' package.json)"
: "${VERSION:?cannot read the @playwright/test version from frontend/package.json}"
IMAGE="mcr.microsoft.com/playwright:v${VERSION}-noble"

if [[ "${E2E_NATIVE:-}" == 1 ]] || ! command -v docker >/dev/null 2>&1 || ! docker info >/dev/null 2>&1; then
  echo "e2e-ui: running on this machine (no Docker, or E2E_NATIVE=1)" >&2
  exec pnpm exec playwright test --project=ui "$@"
fi

# The checkout is mounted; node_modules lives in a volume of its own, so the Linux packages
# never mix with the host's.
exec docker run --rm --init --ipc=host --platform "${E2E_PLATFORM:-linux/amd64}" \
  -v "$ROOT:/work" \
  -v nexc-e2e-ui-node-modules:/work/frontend/node_modules \
  -w /work/frontend \
  -e CI=1 -e COREPACK_ENABLE_DOWNLOAD_PROMPT=0 \
  "$IMAGE" \
  bash -c 'corepack enable >/dev/null && pnpm install --frozen-lockfile --reporter=silent && pnpm exec playwright test --project=ui "$@"' \
  e2e-ui "$@"
