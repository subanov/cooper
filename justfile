# Task runner for Cooper (https://just.systems). `just --list` shows all recipes.
set dotenv-load := true

# Checks and tests use their own PostgreSQL (scripts/test-db.sh), never the compose one:
# a separate container, port, role and database, so test data cannot mix with dev data.
test_db_port := env("COOPER_TEST_POSTGRES_PORT", "55432")
test_database_url := "postgres://cooper_test:cooper_test@localhost:" + test_db_port + "/cooper_test"
test_env := "COOPER_TEST_POSTGRES_PORT=" + test_db_port + " DATABASE_URL=" + test_database_url

# Run every check CI runs.
check: fmt-check lint test sqlx-check migrations-check

# Format all code.
fmt:
    cargo fmt --all

fmt-check:
    cargo fmt --all -- --check

# Clippy with warnings as errors.
lint: test-db
    {{test_env}} cargo clippy --workspace --all-targets --all-features -- -D warnings

# All tests, against the test database (started on demand, see `test-db`).
test: test-db
    {{test_env}} cargo test --workspace --all-features

# Start the local stack.
up:
    docker compose up -d

# Build and run Cooper itself in containers (migrate, server, worker) next to the stack.
up-app:
    docker compose --profile app up -d --build

# Build the runtime image.
image tag="cooper:dev":
    docker build --target runtime -t {{tag}} .

# Start only PostgreSQL (development data).
db:
    docker compose up -d --wait postgres

# Apply migrations (as production does: `cooper-server migrate` with DATABASE_URL).
migrate:
    cargo run -q -p cooper-server -- migrate

# Refresh offline query data (.sqlx/) from the test database's schema. Commit the result.
prepare: test-db
    {{test_env}} cargo sqlx prepare --workspace -- --all-targets --all-features

# Fail if .sqlx/ is stale.
sqlx-check: test-db
    {{test_env}} cargo sqlx prepare --workspace --check -- --all-targets --all-features

# Fail if a migration that exists on main was changed or removed.
migrations-check base="origin/main":
    scripts/check-migrations.sh {{base}}

# Start the test PostgreSQL (container `cooper-test-postgres`, port 55432, role and
# database `cooper_test`, data on tmpfs) and apply migrations. Idempotent.
test-db:
    COOPER_TEST_POSTGRES_PORT={{test_db_port}} scripts/test-db.sh up

# Remove the test PostgreSQL and everything in it.
test-db-down:
    scripts/test-db.sh down

# Run the HTTP server.
server:
    cargo run -p cooper-server

# Run background jobs.
worker:
    cargo run -p cooper-worker

# Test coverage like CI (cargo-llvm-cov + nextest): HTML report in target/llvm-cov/html.
coverage: test-db
    {{test_env}} cargo llvm-cov nextest --workspace --all-features --html
    cargo llvm-cov report --lcov --output-path lcov.info
    scripts/coverage-summary.sh lcov.info

# Back up and restore PostgreSQL to a point in time with pgBackRest (needs Docker).
restore-test:
    deploy/single-host/scripts/test-restore.sh

# Licence, advisory and source checks.
deny:
    cargo deny check

# --- Web UI (ADR-0010): web/ is a pnpm project; Node version in web/.node-version ----------

# Install web dependencies exactly as locked.
web-install:
    pnpm --dir web install --frozen-lockfile

# Vite dev server on http://localhost:5173, proxying /api to `just server`.
web-dev:
    pnpm --dir web dev

# Regenerate web/openapi.json and the TypeScript API types from the Rust API.
web-api:
    cargo run -q -p cooper-server -- openapi > web/openapi.json
    pnpm --dir web api:generate

# Typecheck, Biome, generated-client check and Vitest.
web-check:
    pnpm --dir web check

# Build web/dist, which cooper-server serves (embedded in release builds).
web-build:
    pnpm --dir web build

# End-to-end tests with axe-core (Playwright container) against a running server that
# serves web/dist. From WSL or Docker Desktop: COOPER_E2E_URL=http://host.docker.internal:8080
web-e2e:
    pnpm --dir web e2e

# Storybook screenshot tests (Playwright container, so rendering matches CI).
web-visual:
    pnpm --dir web build-storybook --quiet
    pnpm --dir web visual

# Refresh the screenshot baselines after an intended visual change; commit them.
web-visual-update:
    pnpm --dir web build-storybook --quiet
    pnpm --dir web visual:update
