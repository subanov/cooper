# Task runner for Cooper (https://just.systems). `just --list` shows all recipes.
set dotenv-load := true

# Run every check CI runs.
check: fmt-check lint test sqlx-check migrations-check

# Format all code.
fmt:
    cargo fmt --all

fmt-check:
    cargo fmt --all -- --check

# Clippy with warnings as errors.
lint:
    cargo clippy --workspace --all-targets --all-features -- -D warnings

# All tests. Needs DATABASE_URL and a running database (`just db`).
test:
    cargo test --workspace --all-features

# Start the local stack.
up:
    docker compose up -d

# Build and run Cooper itself in containers (migrate, server, worker) next to the stack.
up-app:
    docker compose --profile app up -d --build

# Build the runtime image.
image tag="cooper:dev":
    docker build --target runtime -t {{tag}} .

# Start only PostgreSQL.
db:
    docker compose up -d --wait postgres

# Apply migrations.
migrate:
    cargo sqlx migrate run

# Refresh offline query data (.sqlx/). Commit the result.
prepare:
    cargo sqlx prepare --workspace -- --all-targets --all-features

# Fail if .sqlx/ is stale.
sqlx-check:
    cargo sqlx prepare --workspace --check -- --all-targets --all-features

# Fail if a migration that exists on main was changed or removed.
migrations-check base="origin/main":
    scripts/check-migrations.sh {{base}}

# Run the HTTP server.
server:
    cargo run -p cooper-server

# Run background jobs.
worker:
    cargo run -p cooper-worker

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

# End-to-end tests with axe-core against a running server that serves web/dist.
web-e2e:
    pnpm --dir web e2e
