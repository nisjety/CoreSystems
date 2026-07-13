# Data Plane v2 — Backup & Restore

> **Verified 2026-07-10.** Re-checked against live containers (`docker ps`), the current
> `scripts/backup.sh` / `scripts/restore.sh` / `scripts/smoke-test.sh`, the Makefile, and the
> `data-orchestrator-go` HTTP router. Ports, Postgres version (16), the default Qdrant collection
> name (`dataplane_knowledge`), and `EMBEDDING_DIMENSION` were all confirmed correct. Two
> inaccuracies were found and fixed below: the Scenario B reindex `curl` example pointed at a
> route that doesn't exist and was missing a required header, and the claim that `restore.sh`
> "skips Postgres if the dump is absent" is false (it hard-fails with `exit 1`). See
> `apps/Data Plane v2/docs/core-research/plane-audit-2026-07-02.md` for the broader plane audit,
> including the live-confirmed finding that the orchestrator's `X-Org-ID` header on this same
> reindex endpoint is accepted with no credential check (any caller can create reindex jobs for
> any org).

## Overview

Data Plane v2 stores canonical state in two places:

1. **Postgres** — documents, knowledge_units, retrieval_traces, wiki pages, graph entities, eval results, source/maintenance logs. Backed up via `pg_dump`.
2. **Qdrant** — dense vectors keyed by `knowledge_id`. Backed up via Qdrant snapshot API.

Dragonfly and NATS are cache/transport dependencies — they do **not** require backup. They can be cold-restarted; data flows back from Postgres/Qdrant on first request.

## Quick start

```bash
# Take a backup
./scripts/backup.sh

# Restore from a backup
./scripts/restore.sh ./backups/20260508T120000Z
```

## What's captured

### Postgres dump
- Full schema + data via `pg_dump --clean --if-exists`
- gzip-compressed
- Restorable into the same major version (PG 16)

### Qdrant snapshots
- Per-collection snapshots (default: `dataplane_knowledge`)
- Created via `POST /collections/<name>/snapshots`
- Downloaded via `GET /collections/<name>/snapshots/<snapshot_name>`
- Each snapshot is a tar of the collection's segment files

## Schedule

| Environment | Frequency | Retention |
|---|---|---|
| Production | Hourly (Postgres), Daily (Qdrant) | 30 days |
| Staging | Daily | 7 days |
| Local dev | On demand | best-effort |

Hourly Postgres is cheap because the WAL + base backup is small relative to the vector store. Qdrant snapshots are larger; daily is sufficient because vectors can be rebuilt from Postgres + reindex if both fail (last-resort recovery).

## Production automation

For production, schedule via cron or a Kubernetes CronJob. Example crontab line:

```
0 * * * * cd /opt/dataplane-v2 && BACKUP_DIR=/var/backups/dpv2/$(date +\%Y\%m\%dT\%H\%M\%SZ) ./scripts/backup.sh >> /var/log/dpv2-backup.log 2>&1
```

Upload to durable storage (S3, GCS, B2) immediately after creation:

```bash
aws s3 sync /var/backups/dpv2/ s3://aquatiq-dpv2-backups/ \
  --exclude "*" --include "*.gz" --include "*/qdrant/*"
```

## Restore scenarios

### Scenario A: corrupted Postgres, intact Qdrant
1. `pg_restore` the latest dump (or stream-restore via `psql`)
2. Run `make migrate-up` to apply any pending migrations
3. Smoke test: `./scripts/smoke-test.sh`
4. Qdrant vectors are already correct — no rebuild needed

### Scenario B: corrupted Qdrant, intact Postgres
1. Restore Qdrant snapshots via `restore.sh` — **note:** the script does NOT skip Postgres if the
   dump is absent; it hard-fails with `exit 1` ("missing Postgres dump"). For a Qdrant-only
   restore, either place an (even empty/no-op) Postgres dump at `<backup-dir>/postgres.sql.gz`
   first, or upload the Qdrant snapshots manually with `curl -X POST
   $QDRANT_URL/collections/<name>/snapshots/upload -F snapshot=@<file>` instead of running the
   full script.
2. **OR** rebuild from Postgres: trigger a reindex job via the orchestrator. The real route is
   `POST /v1/orchestrator/reindex` (not `/v1/jobs`), it requires an `X-Org-ID` header (the
   orchestrator does not currently authenticate this header — treat it as a known gap, not a
   feature), and there is no `full`/wildcard org-wide flag — you must pass the explicit
   `document_ids` to reindex (fetch the org's document IDs from documents-api first):
   ```bash
   curl -X POST http://localhost:8012/v1/orchestrator/reindex \
     -H "Content-Type: application/json" \
     -H "X-Org-ID: <org-id>" \
     -d '{"document_ids":["<doc-id-1>","<doc-id-2>"]}'
   ```
   Reindex is slower (re-embeds every chunk) but always works from canonical state.

### Scenario C: total loss of both stores
1. Provision fresh Postgres + Qdrant
2. Run `make up` to bring infra online
3. `./scripts/restore.sh <backup-dir>` to replay both
4. `make migrate-up` to bring schema to current
5. `./scripts/smoke-test.sh` to validate
6. Resume traffic only after retrieval p95 < 2s on a sample query set

### Scenario D: rolling back a bad migration
1. Restore Postgres to the pre-migration backup
2. `make migrate-status` to confirm desired state
3. Qdrant typically does not need restore (migrations are schema-only)

## Verifying a backup

After every backup, validate:

```bash
# Postgres dump is non-empty and parseable
gunzip -c backups/<ts>/postgres.sql.gz | head -5

# Qdrant snapshots exist for all expected collections
ls backups/<ts>/qdrant/

# (Optional) restore into a sandbox container and run smoke test
docker run --rm -e POSTGRES_PASSWORD=test -p 5499:5432 -d --name dpv2-restore-test postgres:16
gunzip -c backups/<ts>/postgres.sql.gz | psql -h localhost -p 5499 -U postgres -d test
```

## Key non-obvious risks

- **Vector dimension mismatch**: if you change `EMBEDDING_DIMENSION` between backup and restore, Qdrant will reject the snapshot. Always restore into infra with the same model + dimension.
- **Migration drift**: a backup taken at schema version N cannot be safely restored into a database expecting N+k unless intermediate migrations are idempotent. Always run `make migrate-up` after restore.
- **Org isolation**: backups are full-database. There is no per-tenant restore today — restoring one org's data requires extracting from a full dump or running point-in-time recovery against a logical replica.
- **Trace data growth**: `retrieval_traces` accumulates fast. If your backup window matters, set up a partition or rolling-window cleanup job; otherwise dumps grow linearly with traffic.
