#!/usr/bin/env bash
# Backs up everything a Nexc installation holds: the database (pg_dump, custom format) and the
# files on disk (uploaded documents, run artifacts). Restores with scripts/restore.sh.
#
#   scripts/backup.sh [target-dir]           # default backups/<UTC time>
#
# Local development (make dev):  the database container is nexc-postgres, files are in ./data.
# Docker Compose (make docker-up): set NEXC_BACKUP_COMPOSE=1; the database is the `postgres`
# service and the files are in the nexc-data volume.
set -euo pipefail
cd "$(dirname "$0")/.."
[ -f .env ] && { set -a; . ./.env; set +a; }
TARGET="${1:-backups/$(date -u +%Y%m%d-%H%M%S)}"
USER_="${POSTGRES_USER:-nexc}"; DB="${POSTGRES_DB:-nexc}"
mkdir -p "$TARGET"

if [ "${NEXC_BACKUP_COMPOSE:-}" = "1" ]; then
  PROJECT="${COMPOSE_PROJECT_NAME:-nexc}"
  docker compose -p "$PROJECT" exec -T postgres pg_dump -U "$USER_" -d "$DB" -Fc >"$TARGET/database.dump"
  docker run --rm -v "${PROJECT}_nexc-data:/data:ro" -v "$(cd "$TARGET" && pwd):/out" alpine \
    tar -czf /out/files.tar.gz -C /data .
else
  CONTAINER="${NEXC_BACKUP_CONTAINER:-nexc-postgres}"
  docker exec "$CONTAINER" pg_dump -U "$USER_" -d "$DB" -Fc >"$TARGET/database.dump"
  DATA="${NEXC_DATA_DIR:-data}"
  # Earlier backups and scratch output are not part of the installation's files.
  tar -czf "$TARGET/files.tar.gz" -C "$DATA" --exclude=backups --exclude=prove-loop .
fi

# What is inside, so a restore can be checked against it.
( cd "$TARGET" && shasum -a 256 database.dump files.tar.gz >SHA256SUMS )
printf 'backup written to %s (database %s, files %s)\n' "$TARGET" \
  "$(du -h "$TARGET/database.dump" | cut -f1)" "$(du -h "$TARGET/files.tar.gz" | cut -f1)"
