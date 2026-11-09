#!/usr/bin/env bash
# Backs up Cooper: a pgBackRest backup of PostgreSQL (WAL is archived continuously, so any
# point in time since the oldest kept full backup can be restored), then the attachment
# files. Run daily from cron or a systemd timer (docs/runbooks/backup-and-restore.md).
#
#   scripts/backup.sh [full|diff|incr]     # default: diff (full when none exists yet)
set -euo pipefail
cd "$(dirname "$0")/.."
type="${1:-diff}"
case "$type" in full | diff | incr) ;; *) echo "usage: $0 [full|diff|incr]" >&2; exit 2 ;; esac
# shellcheck source=/dev/null
set -a && . ./.env && set +a
: "${COOPER_BACKUP_DIR:?set COOPER_BACKUP_DIR in .env}"
project="${COMPOSE_PROJECT_NAME:-cooper}"

pgbackrest() { docker compose exec -T -u postgres postgres pgbackrest --stanza=cooper "$@"; }

# Idempotent: creates the stanza on first use, verifies it afterwards.
pgbackrest stanza-create
pgbackrest check
pgbackrest --type="$type" backup
pgbackrest info

# Attachments after the database: every file a backed-up row refers to exists by now.
# Blobs are content-addressed and immutable, so copying without overwriting is enough.
mkdir -p "$COOPER_BACKUP_DIR/blobs"
docker run --rm \
  -v "${project}_blobs:/src:ro" \
  -v "$COOPER_BACKUP_DIR/blobs:/dst" \
  alpine:3.22 cp -an /src/. /dst/
echo "backup complete: database ($type) and attachments in $COOPER_BACKUP_DIR/blobs"
