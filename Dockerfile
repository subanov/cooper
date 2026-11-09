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
#   migrate  the same image running `cooper-server migrate`: applies the embedded
#            migrations as the owner role (DATABASE_URL), then exits (ADR-0023).
#
# Builds use the committed .sqlx/ data (SQLX_OFFLINE), so no database is needed.
# Release builds pass VERSION, REVISION and CREATED for the OCI labels.
# Keep the Rust version in step with rust-toolchain.toml and Node with web/.node-version.
# The web UI (web/) is built first and embedded into cooper-server (ADR-0010).

ARG RUST_VERSION=1.99.1
ARG NODE_VERSION=26

# --- web: build the SPA with the locked dependencies ---------------------------------------
FROM --platform=$BUILDPLATFORM node:${NODE_VERSION}-bookworm-slim AS web
ARG PNPM_VERSION=12.18.0
RUN npm install -g pnpm@${PNPM_VERSION}
WORKDIR /web
COPY web/package.json web/pnpm-lock.yaml web/pnpm-workspace.yaml ./
COPY web/tools/openapi-codegen/package.json ./tools/openapi-codegen/
RUN pnpm install --frozen-lockfile
COPY web/ ./
RUN pnpm build

# --- cargo-chef: cache dependency compilation separately from our code ---------------------
# Rust runs on the build machine and cross-compiles for TARGETARCH (amd64 or arm64), so
# multi-arch release images build at native speed; the final stages need no emulation.
FROM --platform=$BUILDPLATFORM rust:${RUST_VERSION}-bookworm AS chef
ARG TARGETARCH
RUN set -eux; \
    case "${TARGETARCH}" in \
      amd64) triple=x86_64-unknown-linux-gnu; gnu=x86_64-linux-gnu ;; \
      arm64) triple=aarch64-unknown-linux-gnu; gnu=aarch64-linux-gnu ;; \
      *) echo "unsupported TARGETARCH ${TARGETARCH}" >&2; exit 1 ;; \
    esac; \
    echo "${triple}" > /target-triple; \
    if [ "$(uname -m)" != "${triple%%-*}" ]; then \
      apt-get update; \
      apt-get install -y --no-install-recommends "gcc-${gnu}" "libc6-dev-${TARGETARCH}-cross"; \
      rm -rf /var/lib/apt/lists/*; \
    fi; \
    rustup target add "${triple}"
# Cross linkers (unused for a native build).
ENV CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc \
    CC_aarch64_unknown_linux_gnu=aarch64-linux-gnu-gcc \
    CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER=x86_64-linux-gnu-gcc \
    CC_x86_64_unknown_linux_gnu=x86_64-linux-gnu-gcc
RUN cargo install cargo-chef --locked
WORKDIR /app

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
ENV SQLX_OFFLINE=true
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --locked --recipe-path recipe.json --target "$(cat /target-triple)" \
    -p cooper-server -p cooper-worker
COPY . .
COPY --from=web /web/dist ./web/dist
RUN target="$(cat /target-triple)" \
    && cargo build --release --locked --target "${target}" -p cooper-server -p cooper-worker \
    && mkdir -p /out \
    && cp "target/${target}/release/cooper-server" "target/${target}/release/cooper-worker" /out/

# --- base: both binaries on distroless (glibc, CA certificates), non-root -------------------
FROM gcr.io/distroless/cc-debian12:nonroot AS base
ARG VERSION=dev
ARG REVISION=unknown
ARG CREATED=unknown
LABEL org.opencontainers.image.title="Cooper" \
      org.opencontainers.image.description="Cooper IT Service Management: cooper-server and cooper-worker" \
      org.opencontainers.image.source="https://github.com/subanov/cooper" \
      org.opencontainers.image.version="${VERSION}" \
      org.opencontainers.image.revision="${REVISION}" \
      org.opencontainers.image.created="${CREATED}"
COPY --from=builder /out/cooper-server /usr/local/bin/cooper-server
COPY --from=builder /out/cooper-worker /usr/local/bin/cooper-worker
ENV COOPER_BIND_ADDR=0.0.0.0:8080 \
    COOPER_LOG_FORMAT=json
USER nonroot:nonroot
ENTRYPOINT []

# --- migrate: applies the embedded migrations as the owner role (DATABASE_URL) -------------
FROM base AS migrate
CMD ["/usr/local/bin/cooper-server", "migrate"]

# --- runtime (default) ----------------------------------------------------------------------
FROM base AS runtime
EXPOSE 8080
# `cooper-server healthcheck` probes GET /health/live (no shell or curl in distroless).
# Orchestrators should also probe GET /health/ready. Containers running cooper-worker
# disable this check (it has no HTTP listener).
HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
    CMD ["/usr/local/bin/cooper-server", "healthcheck"]
CMD ["/usr/local/bin/cooper-server"]
