# Deploying and upgrading Cooper

How to run Cooper in production on one host, how releases change the database safely,
and what to monitor. Decisions: [ADR-0023](../adr/0023-deployment-observability-and-release-pipeline.md).

## What runs

| Container | Image | Role |
|---|---|---|
| `caddy` | `caddy:2.10-alpine` | HTTPS (Let's Encrypt), the only published ports (80, 443) |
| `postgres` | built from `deploy/single-host/postgres` | PostgreSQL 18 + pgBackRest WAL archiving |
| `migrate` | `ghcr.io/subanov/cooper:<version>` | `cooper-server migrate`, then exits |
| `server` | same image | `cooper-server`: API, web UI, Live Updates |
| `worker` | same image, command `cooper-worker` | jobs, outbox, mailbox polling, SLA monitor |

Servers and workers are stateless apart from the attachment volume (`blobs`), and connect
as the `cooper_app` role so Row-Level Security applies. Only `migrate` uses the owner role.

## First installation

Requirements: a Linux host with Docker Engine and the Compose plugin, a DNS name pointing
at it, ports 80 and 443 open.

```bash
git clone https://github.com/subanov/cooper.git && cd cooper/deploy/single-host
cp .env.example .env
```

Fill in `.env`:

- `COOPER_DOMAIN`, `ACME_EMAIL`, and `COOPER_VERSION` (a released version).
- `POSTGRES_PASSWORD` and `COOPER_APP_DB_PASSWORD`: `openssl rand -base64 32` each.
- `COOPER_ENCRYPTION_KEYS`: `1:` followed by `openssl rand -base64 32`. **Store a copy
  outside the host.** Without it, backups cannot decrypt stored secrets
  ([key rotation](encryption-key-rotation.md)).
- `COOPER_SMTP_URL` for outbound email.

```bash
docker compose up -d --build     # builds the PostgreSQL image, migrates, starts everything
docker compose ps                # server healthy, migrate exited 0
```

Open `https://<COOPER_DOMAIN>/` and complete first-run setup. Then schedule backups
([backup-and-restore.md](backup-and-restore.md)).

## Upgrading

1. Read the release notes for the new version, especially **breaking** entries.
2. Take a backup: `scripts/backup.sh`.
3. Set `COOPER_VERSION` in `.env` to the new version.
4. `docker compose pull && docker compose up -d`.

   Compose runs `migrate` to completion before it recreates `server` and `worker`. If
   `migrate` fails, the old containers keep running on the old schema. Check
   `docker compose logs migrate`, fix the cause, and run `up -d` again.
5. Check `docker compose ps` and `https://<COOPER_DOMAIN>/health/ready`.

Downgrades are not supported: migrations are forward-only. To go back, restore the backup
from step 2.

## Migrations: one step, before the new version

- `cooper-server migrate` (the `migrate` image or service) applies the migrations built
  into that release, as the owner role in `DATABASE_URL`. It takes an advisory lock, so
  starting it twice at once is safe, and it does nothing when the schema is current.
- Servers never migrate on start. A server whose schema is behind reports `503` on
  `/health/ready` (and the worker refuses to start) until the migration has run.
- Other orchestrators run the same step as a pre-deploy job (for example a Kubernetes
  `Job` or Helm pre-upgrade hook, #154).

### Writing migrations that allow zero downtime (expand/contract)

During a rolling upgrade the previous release runs against the new schema for a while.
Every migration must therefore be **backward compatible with the release before it**:

| Change | Release N (expand) | Release N+1 (contract) |
|---|---|---|
| Add a column | Add it nullable or with a default; code writes it | Make it `NOT NULL` once every row has a value |
| Rename a column | Add the new column, write both, backfill | Read only the new one; drop the old one |
| Drop a column or table | Stop reading and writing it | Drop it |
| Add a `NOT NULL` or `CHECK` | Add it `NOT VALID`, fix data | `VALIDATE CONSTRAINT` |
| Large index | `CREATE INDEX CONCURRENTLY` in its own migration file (no transaction) | — |

Also: no long table rewrites or locks on hot tables (`tickets`, `outbox_events`), and
backfills in batches from a job, not in one migration statement. Merged migrations never
change (`scripts/check-migrations.sh`).

## Monitoring

- **Health:** `GET /health/live` (process up, used by the image `HEALTHCHECK`) and
  `GET /health/ready` (database reachable and schema current; Caddy uses it).
- **Logs:** JSON on stdout (`docker compose logs`). Request lines carry `request_id`,
  `actor` and `tenant` IDs. Warnings and errors also appear in the System Log screen.
- **Metrics:** set `COOPER_METRICS_ADDR=0.0.0.0:9100` and scrape `server:9100/metrics`
  and `worker:9100/metrics` from a Prometheus on the `cooper` network. The port is not
  published. Useful alerts:

  ```promql
  # API latency (P95 over 5 minutes) above 300 ms
  histogram_quantile(0.95, sum by (le) (rate(cooper_http_request_duration_seconds_bucket{route=~"/api/.*"}[5m]))) > 0.3
  # 5xx rate
  sum(rate(cooper_http_requests_total{status=~"5.."}[5m])) > 0
  # Mailboxes not polled successfully for 10 minutes
  time() - cooper_job_last_success_timestamp_seconds{job="mailbox-poll"} > 600
  # SLA monitor behind by more than 5 minutes
  time() - cooper_job_last_success_timestamp_seconds{job="sla-monitor"} > 300
  # Outbox stuck or failing
  cooper_outbox_oldest_pending_age_seconds > 300 or cooper_outbox_dead_events > 0
  ```

- **Traces:** set `OTEL_EXPORTER_OTLP_ENDPOINT` (for example
  `http://otel-collector:4318`) to export spans over OTLP/HTTP. The other standard
  `OTEL_EXPORTER_OTLP_*` and `OTEL_SERVICE_NAME` variables apply.
