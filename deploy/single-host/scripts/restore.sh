#!/usr/bin/env bash
# Restores PostgreSQL from pgBackRest, to the latest state or to a point in time, then
# restores missing attachment files. Stops Cooper while restoring
# (docs/runbooks/backup-and-restore.md).
#
#   scripts/restore.sh                                  # latest consistent state
#   scripts/restore.sh "2026-11-07 09:30:00+00"         # point in time (UTC offset!)
set -euo pipefail
cd "$(dirname "$0")/.."
target="${1:-}"
# shellcheck source=/dev/null
set -a && . ./.env && set +a
project="${COMPOSE_PROJECT_NAME:-cooper}"

args=(--stanza=cooper --delta)
if [ -n "$target" ]; then
  args+=(--type=time "--target=$target" --target-action=promote)
fi

docker compose stop caddy server worker postgres
docker compose run --rm --no-deps -T -u postgres --entrypoint pgbackrest postgres "${args[@]}" restore
docker compose up -d --wait postgres

if [ -n "${COOPER_BACKUP_DIR:-}" ] && [ -d "$COOPER_BACKUP_DIR/blobs" ]; then
  docker run --rm \
    -v "$COOPER_BACKUP_DIR/blobs:/src:ro" \
    -v "${project}_blobs:/dst" \
    alpine:3.22 cp -an /src/. /dst/
fi

docker compose up -d
echo "restore complete${target:+ to $target}; check /health/ready and sign in"
