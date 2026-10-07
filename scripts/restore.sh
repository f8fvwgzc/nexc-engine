#!/usr/bin/env bash
# Restores a backup made by scripts/backup.sh into an EMPTY installation: a database without
# tables and a data folder without files. It refuses to write over an installation that has
# users, so a restore cannot destroy live data by mistake.
#
#   scripts/restore.sh <backup-dir>
#
# Same environment as backup.sh (NEXC_BACKUP_COMPOSE=1 for Docker Compose). To only check that
# a backup can be read, set NEXC_RESTORE_CHECK=1: it is restored into a scratch database, counted
# and removed again, and nothing of the installation is touched.
set -euo pipefail
cd "$(dirname "$0")/.."
[ -f .env ] && { set -a; . ./.env; set +a; }
SOURCE="${1:?usage: scripts/restore.sh <backup-dir>}"
[ -f "$SOURCE/database.dump" ] && [ -f "$SOURCE/files.tar.gz" ] || { echo "not a backup: $SOURCE" >&2; exit 1; }
( cd "$SOURCE" && shasum -a 256 -c SHA256SUMS >/dev/null ) || { echo "the backup's files do not match their checksums" >&2; exit 1; }
USER_="${POSTGRES_USER:-nexc}"; DB="${POSTGRES_DB:-nexc}"
if [ "${NEXC_BACKUP_COMPOSE:-}" = "1" ]; then
  PG=(docker compose -p "${COMPOSE_PROJECT_NAME:-nexc}" exec -T postgres)
else
  PG=(docker exec -i "${NEXC_BACKUP_CONTAINER:-nexc-postgres}")
fi
sql() { "${PG[@]}" psql -U "$USER_" -d "$1" -Atc "$2"; }

if [ "${NEXC_RESTORE_CHECK:-}" = "1" ]; then
  SCRATCH="nexc_restore_check_$$"
  sql "$DB" "CREATE DATABASE $SCRATCH" >/dev/null
  trap 'sql "$DB" "DROP DATABASE IF EXISTS $SCRATCH" >/dev/null' EXIT
  "${PG[@]}" pg_restore -U "$USER_" -d "$SCRATCH" --no-owner <"$SOURCE/database.dump"
  printf 'the backup restores: %s users, %s workspaces, %s issues, %s files in the archive\n' \
    "$(sql "$SCRATCH" 'SELECT count(*) FROM users')" "$(sql "$SCRATCH" 'SELECT count(*) FROM workspaces')" \
    "$(sql "$SCRATCH" 'SELECT count(*) FROM issues')" "$(tar -tzf "$SOURCE/files.tar.gz" | grep -vc '/$')"
  exit 0
fi

EXISTING="$(sql "$DB" "SELECT CASE WHEN to_regclass('public.users') IS NULL THEN 0 ELSE 1 END")"
if [ "$EXISTING" = "1" ] && [ "$(sql "$DB" 'SELECT count(*) FROM users')" != "0" ]; then
  echo "this installation already has users; restore into an empty database" >&2; exit 1
fi
"${PG[@]}" pg_restore -U "$USER_" -d "$DB" --no-owner --clean --if-exists <"$SOURCE/database.dump"
if [ "${NEXC_BACKUP_COMPOSE:-}" = "1" ]; then
  docker run --rm -v "${COMPOSE_PROJECT_NAME:-nexc}_nexc-data:/data" -v "$(cd "$SOURCE" && pwd):/in:ro" alpine \
    tar -xzf /in/files.tar.gz -C /data
else
  mkdir -p "${NEXC_DATA_DIR:-data}" && tar -xzf "$SOURCE/files.tar.gz" -C "${NEXC_DATA_DIR:-data}"
fi
echo "restored from $SOURCE; start the server with the same NEXC_MASTER_KEY, or stored AI keys cannot be read"
