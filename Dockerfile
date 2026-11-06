# syntax=docker/dockerfile:1.7
#
# Cooper container image. One image holds both binaries; pick one with the command:
#
#   docker build -t cooper .
#   docker run cooper                      # cooper-server (default)
#   docker run cooper /usr/local/bin/cooper-worker
#
# Targets:
#   runtime  (default) cooper-server + cooper-worker on distroless, non-root.
#   migrate  sqlx-cli + /migrations, to apply migrations as the owner role.
#
# Builds use the committed .sqlx/ data (SQLX_OFFLINE), so no database is needed.
# Keep the Rust version in step with rust-toolchain.toml and Node with web/.node-version.
# The web UI (web/) is built first and embedded into cooper-server (ADR-0010).

ARG RUST_VERSION=1.99.1
ARG NODE_VERSION=26

# --- web: build the SPA with the locked dependencies ---------------------------------------
FROM node:${NODE_VERSION}-bookworm-slim AS web
ARG PNPM_VERSION=12.18.0
RUN npm install -g pnpm@${PNPM_VERSION}
WORKDIR /web
COPY web/package.json web/pnpm-lock.yaml web/pnpm-workspace.yaml ./
RUN pnpm install --frozen-lockfile
COPY web/ ./
RUN pnpm build

# --- cargo-chef: cache dependency compilation separately from our code ---------------------
FROM rust:${RUST_VERSION}-bookworm AS chef
RUN cargo install cargo-chef --locked
WORKDIR /app

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
ENV SQLX_OFFLINE=true
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --locked --recipe-path recipe.json \
    -p cooper-server -p cooper-worker
COPY . .
COPY --from=web /web/dist ./web/dist
RUN cargo build --release --locked -p cooper-server -p cooper-worker

# --- migrate: applies ./migrations; run it as the owner role (DATABASE_URL) ----------------
FROM rust:${RUST_VERSION}-slim-bookworm AS migrate
ARG SQLX_CLI_VERSION=0.9.0
RUN cargo install sqlx-cli --version "${SQLX_CLI_VERSION}" --locked \
    --no-default-features --features rustls,postgres
WORKDIR /app
COPY migrations ./migrations
ENTRYPOINT ["sqlx"]
CMD ["migrate", "run"]

# --- runtime ---------------------------------------------------------------------------------
FROM gcr.io/distroless/cc-debian12:nonroot AS runtime
COPY --from=builder /app/target/release/cooper-server /usr/local/bin/cooper-server
COPY --from=builder /app/target/release/cooper-worker /usr/local/bin/cooper-worker
ENV COOPER_BIND_ADDR=0.0.0.0:8080 \
    COOPER_LOG_FORMAT=json
EXPOSE 8080
USER nonroot:nonroot
# Probe GET /health/live (process up) and GET /health/ready (database + migrations) from
# the orchestrator; distroless has no shell or curl for an in-image HEALTHCHECK.
ENTRYPOINT []
CMD ["/usr/local/bin/cooper-server"]
