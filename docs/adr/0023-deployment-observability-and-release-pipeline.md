# 0023. Ship one signed multi-arch image, migrate in a separate step, back up with pgBackRest, and expose Prometheus metrics and OTLP traces

- **Status:** Accepted
- **Date:** 2026-11-07
- **Issue:** #84, #85, #86, #87, #88, #90, #92, #93
- **Requirements:** FR-SYS-006; Appendix A (backups with point-in-time recovery,
  observability)
- **Related:** [ADR-0009](0009-shared-crates-and-dependency-baseline.md) (dependency
  baseline), [ADR-0004](0004-tenant-isolation-with-row-level-security.md) (owner and
  application roles)

## Context

P1 needs Cooper to be deployable and operable, not only buildable:

- a production image, and a safe way to change the schema while old servers still run;
- a reference single-host deployment with TLS;
- backups that can restore to a point in time (Appendix A), including attachments;
- logs, traces and metrics an operator can alert on (HTTP latency, job durations, mailbox
  and SLA lag, outbox backlog);
- a release process with versions, a changelog, signed images and SBOMs, plus security
  scanning and coverage in CI.

## Decision

### Images (#85)

- One Dockerfile, one set of binaries, two targets: `runtime` (default; `cooper-server`,
  or `cooper-worker` as the command) and `migrate`. Both run on
  `gcr.io/distroless/cc-debian12:nonroot` and carry OCI labels (version, revision,
  created, source).
- Rust **cross-compiles** on the build machine for `TARGETARCH`, so `linux/arm64` images
  build at native speed (no QEMU); the final stages have no `RUN` step.
- Distroless has no shell or curl, so the binary probes itself: `cooper-server
  healthcheck` (exit 0 when `GET /health/live` answers 200) is the image `HEALTHCHECK`.
  Containers running the worker disable it.

### Migrations (#87)

- `cooper-server migrate` applies the migrations embedded at build time as the owner role
  (`DATABASE_URL`) and exits. It is a separate deployment step, never part of server
  start-up: servers connect as the application role, which cannot change the schema, and
  `/health/ready` reports `503` until the schema matches.
- A PostgreSQL advisory lock serialises concurrent runs; a second run is a no-op. The
  `migrate` image no longer installs `sqlx-cli`.
- Schema changes follow **expand/contract** so the previous release keeps working while
  the new one rolls out (`docs/runbooks/deployment.md`). Migrations stay forward-only.

### Single-host deployment (#92)

`deploy/single-host/`: Docker Compose with Caddy (automatic Let's Encrypt TLS, HSTS,
unbuffered SSE), PostgreSQL, `migrate`, `server` and `worker`. Only Caddy publishes ports.
Secrets come from a git-ignored `.env`. Upgrades change `COOPER_VERSION` and run
`docker compose up -d`, which migrates first.

### Backups (#93)

- **pgBackRest** in the PostgreSQL image (from the PGDG repository the official image
  already uses): continuous WAL archiving plus scheduled full/differential backups,
  `zstd` compression, two full backups retained, optional S3-compatible second repository.
  Chosen over WAL-G for its built-in `check`, retention and `--delta` restores.
- `scripts/backup.sh` backs up the database, then copies attachment files (content-
  addressed, so copying without overwriting is enough; copying after the database means
  every referenced file exists). `scripts/restore.sh` restores to the latest state or a
  point in time.
- CI runs `scripts/test-restore.sh` on every PR: write, back up, write again, restore to
  the time in between, and check that the second write is absent.
- The encryption keys (`COOPER_ENCRYPTION_KEYS`) must be backed up separately; a database
  backup without them cannot decrypt stored secrets.

### Observability (#88)

- **Logs:** JSON in containers. The HTTP request span carries `request_id`, `actor`
  (`analyst:<uuid>`, `api_key:<uuid>`, …) and `tenant` (when a request touches exactly
  one Company): IDs only, never names or email addresses.
- **Metrics:** the `metrics` facade with `metrics-exporter-prometheus` (recorder only, no
  bundled HTTP server). When `COOPER_METRICS_ADDR` is set, server and worker serve
  `GET /metrics` on that separate address, never on the public listener. Names are in
  `cooper_platform::metrics::names` and `cooper_http::metrics`, and are a contract:
  - `cooper_http_requests_total` / `cooper_http_request_duration_seconds` by method, route
    template and status;
  - `cooper_job_runs_total`, `cooper_job_duration_seconds` and
    `cooper_job_last_success_timestamp_seconds` per job, so mailbox-poll and SLA-monitor
    lag is `time() - cooper_job_last_success_timestamp_seconds{job="…"}`;
  - `cooper_outbox_deliveries_total` and the backlog gauges (pending, oldest pending age,
    dead), sampled by the worker every 15 s.
  Labels hold bounded sets only (no IDs).
- **Traces:** optional OTLP/HTTP export behind the `otlp` cargo feature (on by default in
  both binaries), enabled at run time by the standard `OTEL_EXPORTER_OTLP_ENDPOINT`.
  `opentelemetry-otlp` uses reqwest with the ring rustls provider (no aws-lc, no tonic).
  Trace context is not yet propagated from incoming `traceparent` headers.

### Release pipeline (#86)

- Semantic versioning from the workspace version in `Cargo.toml`. A `vX.Y.Z` tag must
  match it.
- On a tag: multi-arch `runtime` and `migrate` images to `ghcr.io`, with BuildKit SBOM
  and provenance attestations, signed keyless with cosign. A GitHub release with notes
  from **git-cliff** (Conventional Commits, `cliff.toml`) and CycloneDX SBOMs of both
  binaries is attached.

### CI security and coverage (#84, #90)

- CodeQL (Rust, Actions, TypeScript; `security-extended`) on PRs, `main` and weekly.
- Trivy scans the built image. Results go to code scanning, and fixable critical or high
  findings fail the job. The third-party action is pinned to a commit.
- `cargo-cyclonedx` SBOMs on every PR (artifact) and release (asset).
- Tests run under `cargo llvm-cov nextest`. The job summary and a sticky PR comment show
  line coverage overall and of each crate's domain layer.

## Consequences

- New runtime dependencies: `metrics`, `metrics-exporter-prometheus`, and (feature
  `otlp`) `opentelemetry`, `opentelemetry_sdk`, `opentelemetry-otlp`,
  `tracing-opentelemetry`. All are Apache-2.0 or MIT. A build without `otlp` drops the
  OpenTelemetry crates.
- Coverage instrumentation makes the test job slower; it replaces the plain test run
  rather than adding a second one.
- Kubernetes (Helm), staging and DAST are P2 work (#154, #155, #157) and build on these
  images, probes and metrics.
- Revisit backups if Cooper moves to a managed PostgreSQL (use the provider's PITR) or to
  object storage for attachments (#242: bucket versioning replaces the copy step).
