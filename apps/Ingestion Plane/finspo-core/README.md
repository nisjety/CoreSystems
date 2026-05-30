# finspo-core

SharePoint ingestion and governance connector for the Triodelab Ingestion Plane.

finspo discovers SharePoint sites/drives, keeps a live mirror of their contents
via **Microsoft Graph delta sync** (never full recrawls), captures hashes and
ACL summaries, feeds extracted content into the Data Plane, and provides a
**reviewed governance workflow** for duplicate cleanup, storage analytics, and
audited delete/archive actions.

> Destructive actions (delete/archive) are **never automatic**. They run only
> through an approval-gated, audited, kill-switched workflow. See
> [docs/REQUIREMENTS.md](docs/REQUIREMENTS.md#execution-safety-model).

## Documentation

| Doc | What it covers |
|-----|----------------|
| [docs/REQUIREMENTS.md](docs/REQUIREMENTS.md) | **What finspo must have to run** — dependencies, env vars, Graph scopes, bootstrap, health checks. Start here for deployment. |
| [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) | Components, data flow, database schema, events, design decisions. |
| [docs/API.md](docs/API.md) | HTTP endpoint reference. |

## Quick facts

- **Language / runtime:** Go 1.25, single binary (`cmd/api`), Fiber HTTP server on `:3130`.
- **State:** PostgreSQL database `finspo` (schema owned by embedded migrations).
- **Events:** NATS subjects under the `finspo.*` prefix (optional — no-op if unset).
- **Auth:** API-key header (`X-API-Key` / `x-internal-api-key`) + `X-Org-ID` on every `/api/v1/*` call.
- **AI:** none in-process. Near-duplicate / semantic analysis is owned by the **Model Plane**; finspo only emits work items.

## Local quick start

```bash
# 1. Bring up the Ingestion Plane (creates the finspo DB, NATS, integration-core).
cd "apps/Ingestion Plane"
docker compose up -d --build finspo-api

# 2. Confirm it is alive.
curl -s http://localhost:3130/health   # {"status":"ok","service":"finspo-core"}
curl -s http://localhost:3130/ready    # {"status":"ok","checks":{"db":"ok","nats":"ok"}}
```

To ingest a real tenant you must first register a `microsoft-graph` connection
for the org — see [docs/REQUIREMENTS.md](docs/REQUIREMENTS.md#3-an-active-microsoft-graph-connection)
and `scripts/connect-microsoft.sh`.

## Repository layout

```
cmd/
  api/                      # HTTP server entrypoint
  backfill-source-objects/  # one-shot: replay existing inventory into the Data Plane
internal/
  api/         # Fiber handlers (sources, analytics, recommendations, proposals)
  auth/        # API-key + org-scope middleware
  config/      # env-var configuration
  db/          # pgx pool + embedded migration runner
  dataplane/   # Data Plane source-object client (sink)
  events/      # NATS publisher, subjects, typed payloads
  sharepoint/  # Graph clients: browser, delta, permissions, mutation
  store/       # pgx repositories (sources, items, cursors, permissions,
               #   analytics, proposals, audit, recommendations, locks)
  sync/        # delta engine, scheduler, executor
  telemetry/   # zerolog + (future) OTEL
scripts/
  connect-microsoft.sh      # register a microsoft-graph connection without UI
```

## Tests

```bash
go test -race ./...
```
