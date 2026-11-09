# Backup, point-in-time recovery and restore

Applies to the single-host deployment in `deploy/single-host/`
([deployment.md](deployment.md)). Decisions:
[ADR-0023](../adr/0023-deployment-observability-and-release-pipeline.md).

## What is backed up, and how

| Data | How | Where |
|---|---|---|
| PostgreSQL | pgBackRest: WAL archived continuously (at least every 5 minutes, `archive_timeout`), plus full or differential backups from `scripts/backup.sh` | `pgbackrest` volume (repo1), optionally S3 (repo2) |
| Attachments | `scripts/backup.sh` copies new files after the database backup | `$COOPER_BACKUP_DIR/blobs` |
| Encryption keys | **Manually**, once and after each rotation | A password manager or vault, never next to the backups |
| `.env` | Manually | Same place as the keys |

pgBackRest keeps two full backups and all WAL since the older one. You can restore to any
moment in that window (point-in-time recovery, PITR).

**Recovery point objective:** at most 5 minutes of data (the `archive_timeout`). Off-site
copies need repo2 (below).

## Schedule backups

```bash
cd deploy/single-host
scripts/backup.sh full       # first run (it is also full whenever none exists)
```

Then schedule a weekly full and a daily differential backup, for example with cron
(`crontab -e` as a user who can run `docker compose`):

```cron
15 2 * * 0   cd /opt/cooper/deploy/single-host && scripts/backup.sh full  >> /var/log/cooper-backup.log 2>&1
15 2 * * 1-6 cd /opt/cooper/deploy/single-host && scripts/backup.sh diff  >> /var/log/cooper-backup.log 2>&1
```

Check the log and `docker compose exec -u postgres postgres pgbackrest info` from time to
time. Alert when the newest backup is older than two days.

### Off-site copies (recommended)

Add a second repository to `postgres/pgbackrest.conf`, rebuild
(`docker compose up -d --build postgres`) and run `scripts/backup.sh full` once:

```ini
repo2-type=s3
repo2-path=/cooper
repo2-s3-bucket=<bucket>
repo2-s3-endpoint=<s3.eu-west-1.amazonaws.com or your provider>
repo2-s3-region=<region>
repo2-retention-full=4
```

Pass the credentials as `PGBACKREST_REPO2_S3_KEY` and `PGBACKREST_REPO2_S3_KEY_SECRET` in
the `postgres` service environment. Copy `$COOPER_BACKUP_DIR/blobs` off-site too, for
example with `rclone sync` or `restic`.

## Restore

Restoring replaces the current database. Cooper is stopped during the restore.

```bash
cd deploy/single-host
scripts/restore.sh                                 # latest state in the archive
scripts/restore.sh "2026-11-07 09:30:00+00"        # state at that moment (give the UTC offset)
```

The script:

1. stops Caddy, the server, the worker and PostgreSQL;
2. runs `pgbackrest restore --delta` (only changed files are rewritten), with
   `--type=time --target=… --target-action=promote` for a point in time;
3. starts PostgreSQL. The database replays WAL up to the target and opens for writes;
4. copies attachment files that are missing from the volume back from
   `$COOPER_BACKUP_DIR/blobs`;
5. starts everything again.

Afterwards check `https://<domain>/health/ready` and sign in. A restore to an earlier time
loses later changes; tell Analysts which period to re-check.

### Restoring onto a new host

1. Install as in [deployment.md](deployment.md) with the **same** `.env`, in particular the
   same `COOPER_ENCRYPTION_KEYS` and passwords, but do not run `docker compose up` yet.
2. Make the pgBackRest repository available: restore the `pgbackrest` volume from its
   off-site copy, or configure repo2 (S3) and add `--repo=2` to the restore command in
   `scripts/restore.sh`.
3. `docker compose build postgres && docker compose create`, then `scripts/restore.sh`.
4. Copy the off-site attachment copy to `$COOPER_BACKUP_DIR/blobs` before step 3, so the
   script restores the files.

## How we know it works

The CI job **Backup and point-in-time restore** runs `scripts/test-restore.sh` on every
pull request. It builds the PostgreSQL image, creates the stanza, writes a row, takes a
full backup, notes the time, writes a second row, restores to the noted time, and fails
unless only the first row is back. Run it locally with
`deploy/single-host/scripts/test-restore.sh` (needs Docker).

Also rehearse a full restore onto a spare host at least once a quarter.
