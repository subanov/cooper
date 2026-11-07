#!/usr/bin/env bash
# Proves the backup and point-in-time recovery procedure works (CI job `backup-restore`):
# writes a row, takes a full backup, notes the time, writes another row, restores to the
# noted time and checks that only the first row is back. Uses its own throw-away project.
set -euo pipefail
cd "$(dirname "$0")/.."
export COMPOSE_PROJECT_NAME=cooper-restore-test
# Values the compose file requires; only PostgreSQL starts here.
export POSTGRES_PASSWORD=restore-test COOPER_APP_DB_PASSWORD=restore-test-app \
  COOPER_VERSION=test COOPER_DOMAIN=localhost ACME_EMAIL=test@example.com \
  COOPER_ENCRYPTION_KEYS=unused
dc() { docker compose --env-file /dev/null "$@"; }
psql() { dc exec -T -u postgres postgres psql -v ON_ERROR_STOP=1 -At -U cooper -d cooper -c "$1"; }
pgbackrest() { dc exec -T -u postgres postgres pgbackrest --stanza=cooper "$@"; }
cleanup() { dc down -v --remove-orphans >/dev/null 2>&1 || true; }
trap cleanup EXIT

cleanup
dc build postgres
dc up -d --wait postgres
pgbackrest stanza-create
pgbackrest check

psql "CREATE TABLE restore_probe (v text)"
psql "INSERT INTO restore_probe VALUES ('before')"
pgbackrest --type=full backup
sleep 1
target="$(psql "SELECT to_char(clock_timestamp() AT TIME ZONE 'UTC', 'YYYY-MM-DD HH24:MI:SS.US') || '+00'")"
sleep 1
psql "INSERT INTO restore_probe VALUES ('after')"
psql "SELECT pg_switch_wal()" >/dev/null
pgbackrest check   # waits until the WAL with 'after' is archived

dc stop postgres
dc run --rm --no-deps -T -u postgres --entrypoint pgbackrest postgres \
  --stanza=cooper --delta --type=time "--target=$target" --target-action=promote restore
dc up -d --wait postgres

rows="$(psql "SELECT string_agg(v, ',' ORDER BY v) FROM restore_probe")"
if [ "$rows" != "before" ]; then
  echo "point-in-time restore to $target returned rows: $rows (expected: before)" >&2
  exit 1
fi
echo "point-in-time restore OK: restored to $target, 'after' row correctly absent"
