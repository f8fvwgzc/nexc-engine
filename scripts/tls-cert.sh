#!/usr/bin/env bash
# A certificate for the console's HTTPS listener (HTTP/2, and HTTP/3 over QUIC) on this machine:
# localhost, 127.0.0.1 and ::1, written to NEXC_TLS_DIR (default data/tls) as tls.crt and
# tls.key, which docker-compose.yml mounts into the frontend.
#
#   scripts/tls-cert.sh               # (re)write it
#   scripts/tls-cert.sh --if-missing  # only when there is none (make init)
#
# With mkcert installed (https://github.com/FiloSottile/mkcert, `mkcert -install` once), it is
# signed by mkcert's local CA, which this machine's browsers trust; otherwise it is self-signed,
# and a browser asks once before it opens https://localhost:8443. Either way it is for this
# machine only: a server needs a certificate for its own name (docs/adr/0001-*.md).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

DIR="${NEXC_TLS_DIR:-./data/tls}"
CRT="$DIR/tls.crt"
KEY="$DIR/tls.key"

if [[ "${1:-}" == "--if-missing" && -f "$CRT" && -f "$KEY" ]]; then
  exit 0
fi
mkdir -p "$DIR"

if command -v mkcert >/dev/null 2>&1; then
  mkcert -cert-file "$CRT" -key-file "$KEY" localhost 127.0.0.1 ::1 >/dev/null 2>&1
  how="signed by mkcert's local CA"
else
  command -v openssl >/dev/null 2>&1 || { echo "error: openssl is required" >&2; exit 1; }
  openssl req -x509 -newkey ec -pkeyopt ec_paramgen_curve:prime256v1 -nodes -days 397 \
    -subj "/CN=localhost" \
    -addext "subjectAltName=DNS:localhost,IP:127.0.0.1,IP:::1" \
    -addext "extendedKeyUsage=serverAuth" \
    -keyout "$KEY" -out "$CRT" >/dev/null 2>&1
  how="self-signed"
fi
# nginx runs as uid 101 in the container and must read the key through the bind mount.
chmod 644 "$CRT" "$KEY"
echo "  wrote $CRT ($how, for localhost)"
