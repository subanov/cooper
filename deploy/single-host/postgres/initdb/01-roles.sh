#!/usr/bin/env bash
# Runs once when the database volume is first created.
#
# `cooper` (POSTGRES_USER) owns the schema and runs `cooper-server migrate`. The
# application connects as `cooper_app`, which is not a superuser, so Row-Level Security
# applies as a second layer of tenant isolation (ADR-0004).
set -euo pipefail
: "${COOPER_APP_DB_PASSWORD:?set COOPER_APP_DB_PASSWORD}"
psql -v ON_ERROR_STOP=1 --username "$POSTGRES_USER" --dbname "$POSTGRES_DB" \
     -v app_password="$COOPER_APP_DB_PASSWORD" <<'SQL'
CREATE ROLE cooper_app LOGIN PASSWORD :'app_password';
GRANT CONNECT ON DATABASE cooper TO cooper_app;
GRANT USAGE ON SCHEMA public TO cooper_app;
ALTER DEFAULT PRIVILEGES FOR ROLE cooper IN SCHEMA public
    GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO cooper_app;
ALTER DEFAULT PRIVILEGES FOR ROLE cooper IN SCHEMA public
    GRANT USAGE, SELECT ON SEQUENCES TO cooper_app;
ALTER DEFAULT PRIVILEGES FOR ROLE cooper IN SCHEMA public
    GRANT EXECUTE ON FUNCTIONS TO cooper_app;
SQL
