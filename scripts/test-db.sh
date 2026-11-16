#!/usr/bin/env bash
# A throwaway PostgreSQL for tests and checks, separate from the docker compose stack.
#
#   scripts/test-db.sh up     # start (if needed), wait until ready, apply migrations
#   scripts/test-db.sh url    # print the owner URL tests use as DATABASE_URL
#   scripts/test-db.sh down   # stop and remove it (all test data is gone)
#
# It is a plain container (not a compose service), so `docker compose up/down -v` never
# touches it, and it has its own port, credentials and database: `cooper_test` instead of
# `cooper`. Tests (`#[sqlx::test]` creates a database per test, plus test roles such as
# `cooper_rls_probe`) therefore never write into the development database. Data lives on
# a tmpfs and fsync is off: fast, and gone when the container stops.
set -euo pipefail

name="${COOPER_TEST_POSTGRES_CONTAINER:-cooper-test-postgres}"
port="${COOPER_TEST_POSTGRES_PORT:-55432}"
image="postgres:18-alpine"
user="cooper_test"
password="cooper_test"
db="cooper_test"
url="postgres://${user}:${password}@localhost:${port}/${db}"

running() {
    [ "$(docker inspect -f '{{.State.Running}}' "$name" 2>/dev/null)" = "true" ]
}

up() {
    if ! running; then
        docker rm -f "$name" >/dev/null 2>&1 || true
        docker run -d --name "$name" \
            --label org.cooper.role=test-database \
            -e POSTGRES_USER="$user" -e POSTGRES_PASSWORD="$password" -e POSTGRES_DB="$db" \
            -p "127.0.0.1:${port}:5432" \
            --tmpfs /var/lib/postgresql:rw \
            "$image" \
            -c fsync=off -c synchronous_commit=off -c full_page_writes=off \
            -c max_connections=300 >/dev/null
    fi
    # TCP, not the socket: during initdb the entrypoint's temporary server is socket-only.
    for _ in $(seq 1 60); do
        if docker exec "$name" pg_isready -q -h 127.0.0.1 -U "$user" -d "$db"; then
            DATABASE_URL="$url" sqlx migrate run --source migrations >/dev/null
            echo "test database ready: $url" >&2
            return 0
        fi
        sleep 1
    done
    docker logs "$name" >&2 || true
    echo "test database did not become ready" >&2
    exit 1
}

case "${1:-up}" in
    up) up ;;
    url) echo "$url" ;;
    down) docker rm -f "$name" >/dev/null 2>&1 || true ;;
    *) echo "usage: $0 [up|url|down]" >&2; exit 2 ;;
esac
