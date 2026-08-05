# finspo-core — Requirements to Function

This is the deployment checklist: everything finspo needs in place to start,
sync, and operate correctly. Sections are ordered from "won't boot without it"
to "needed only for specific features".

---

## TL;DR — minimum to boot

finspo-api **will not start** unless all four of these are set and reachable:

| Requirement | Why | Failure mode if missing |
|-------------|-----|-------------------------|
| `FINSPO_API_KEY` | Authenticates inbound `/api/v1/*` callers | Process exits: `FINSPO_API_KEY is required` |
| `FINSPO_DSN` → a reachable Postgres **`finspo` database** | All state lives here | Process exits at startup (`open finspo database` / migration error) |
| `INTEGRATION_CORE_URL` | Where to fetch Microsoft Graph tokens | Process exits: `INTEGRATION_CORE_URL is required` |
| `INTERNAL_API_KEY` | Auth to the integration-core token broker | Process exits: `INTERNAL_API_KEY is required` |

Everything else has a safe default. The service boots into a usable state
(health/readiness green) with just these — but it cannot ingest real data until
a **Microsoft Graph connection** exists (see §3).

---

## 1. PostgreSQL database `finspo`

finspo owns its schema and applies migrations on every boot
(`internal/db.ApplyMigrations`), but it **cannot create its own database**.

- The database `finspo` must exist on the Postgres instance pointed to by
  `FINSPO_DSN` (in the standard stack: `ingestion-postgres`, owner
  `ingestion_user`).
- On a **fresh** `ingestion-postgres` volume, `init-databases.sql` creates it
  automatically.
- On a **pre-existing** volume (created before finspo existed), the database is
  missing and finspo crashes with `database "finspo" does not exist`. Two fixes:
  - Run the build script — `build-verevon-services.sh` has an idempotent
    `ensure_finspo_database` post-build hook on the Ingestion Plane stack.
  - Or create it manually:
    ```sql
    CREATE DATABASE finspo WITH OWNER = ingestion_user
        ENCODING = 'UTF8' LC_COLLATE='en_US.utf8' LC_CTYPE='en_US.utf8'
        TEMPLATE = template0;
    ```
- Migrations are embedded in the binary and tracked in `schema_migrations`;
  they are idempotent and run automatically. No manual migration step.

Tables created: `sources`, `items`, `permissions`, `delta_cursors`,
`audit_log`, `review_proposals`, `schema_migrations`.

## 2. integration-core token broker (Microsoft Graph auth)

finspo never holds Microsoft credentials. For every Graph call it asks
integration-core for a short-lived token:

```
POST {INTEGRATION_CORE_URL}/internal/connectors/token
  header: X-Internal-API-Key: {INTERNAL_API_KEY}
  body:   {"organizationId": "...", "connectorType": "microsoft-graph"}
```

Requirements:
- integration-core (`integration-api`, default `:3026`) is up and reachable on
  the `inter-plane-bus` network.
- `INTERNAL_API_KEY` matches integration-core's internal key.

## 3. An active `microsoft-graph` connection

This is the most common reason ingestion "does nothing". The token broker
returns `404 connection_not_found` until the **organization** has completed the
Microsoft OAuth consent flow through `integration-corev2`. finspo cannot create
this — it must already exist.

How to create one (no verevonv2 UI required):
- `scripts/connect-microsoft.sh` — drives integration-core's
  `connect-session` endpoint, prints the Microsoft consent URL.
- Or verevon v1's existing integrations page.
- Or the Verevon v2 onboarding/integrations page.

### Required Graph scopes

The connection's OAuth scopes determine what finspo can do. `integration-corev2`
uses capability bundles instead of Nango provider seeds. The default onboarding
and knowledge bundles request:

```
openid, profile, email, offline_access, User.Read, Files.Read.All,
Sites.Read.All, Team.ReadBasic.All, Channel.ReadBasic.All
```

| finspo capability | Graph call | Scope needed | In default seed? |
|-------------------|-----------|--------------|------------------|
| Site discovery | `GET /sites?search=*` | `Sites.Read.All` | ✅ |
| Delta sync | `GET /drives/{id}/root/delta` | `Files.Read.All` | ✅ |
| ACL capture | `GET /drives/{id}/items/{id}/permissions` | `Files.Read.All` | ✅ |
| **Execution: delete** | `DELETE /drives/{id}/items/{id}` | **`Files.ReadWrite.All`** | ❌ — must add |
| **Execution: archive** | `PATCH /drives/{id}/items/{id}` | **`Files.ReadWrite.All`** | ❌ — must add |

> Read-only ingest + governance reporting works out of the box. **Execution
> (Phase 5) requires write scopes** that are not in the default bundle — request
> the `sharepoint.write` capability and re-consent before enabling execution, or
> every execute attempt will 403 (captured in the proposal's `failure_reason`,
> so it fails safe).

> The default flow uses **delegated** (user-context) tokens: finspo sees only
> what the connecting user can access. Org-wide governance across all
> sites/drives ultimately wants **application** permissions with admin consent.

## 4. NATS event bus (optional)

- `NATS_URL` empty → finspo runs with a **no-op publisher**. Ingestion and
  governance still work; nothing is published. `/ready` reports `nats: skipped`.
- `NATS_URL` set → finspo publishes lifecycle events under `NATS_SUBJECT_PREFIX`
  (default `finspo`). In the standard stack: `nats://nats:4222`.
- Subjects: `finspo.item.upserted`, `finspo.item.deleted`, `finspo.source.synced`,
  `finspo.proposal.{created,approved,rejected,executed,failed}`.

## 5. Data Plane documents API (optional)

- `DATA_PLANE_DOCUMENTS_BASE_URL` empty → the source-object sink is a no-op;
  finspo still mirrors inventory locally and publishes events.
- Set → on every item upsert/delete finspo POSTs a source-object record (with
  hashes, ACL tags, content hash) to the Data Plane for downstream document
  ingest. finspo exchanges `FINSPO_SERVICE_API_KEY` with Auth Core for a
  short-lived `aud=data-plane`, `documents:write` token bound to the source's
  verified organization. Tokens are cached per organization, refreshed before
  expiry, and refreshed once if Data Plane returns 401. The durable credential
  never crosses into Data Plane.

---

## Environment variables (complete)

### Required (no default — boot fails if unset)
| Var | Purpose |
|-----|---------|
| `FINSPO_API_KEY` | API key inbound `/api/v1/*` callers must present |
| `FINSPO_DSN` | Postgres DSN for the `finspo` database |
| `INTEGRATION_CORE_URL` | integration-core base URL (Graph token broker) |
| `INTERNAL_API_KEY` | Auth to integration-core's internal endpoints |

### Optional (safe defaults)
| Var | Default | Purpose |
|-----|---------|---------|
| `PORT` | `3130` | HTTP listen port |
| `ENVIRONMENT` | `dev` | `dev`/`local` → debug logging |
| `SERVICE_NAME` | `finspo-core` | Log/telemetry service tag |
| `FINSPO_API_KEY_HEADER` | `X-API-Key` | Header name for the API key |
| `GRAPH_BASE_URL` | `https://graph.microsoft.com` | Graph API base |
| `NATS_URL` | _(empty → no-op)_ | NATS connection URL |
| `NATS_SUBJECT_PREFIX` | `finspo` | Event subject prefix |
| `OTEL_EXPORTER_OTLP_ENDPOINT` | _(empty)_ | OTEL collector (exporter wiring pending) |
| `DATA_PLANE_DOCUMENTS_BASE_URL` | _(empty → no-op)_ | Data Plane document ingest target |
| `AUTH_CORE_URL` | `http://auth-core:3011` | Auth Core issuer used when Data Plane forwarding is enabled |
| `FINSPO_SERVICE_ID` | `finspo-core` | Registered Auth Core service-principal id |
| `FINSPO_SERVICE_API_KEY` | _(empty)_ | Durable mint credential; required when `DATA_PLANE_DOCUMENTS_BASE_URL` is set |
| `FINSPO_SYNC_INTERVAL` | `5m` | Scheduler fan-out interval (Go duration) |
| `FINSPO_CAPTURE_PERMISSIONS` | `true` | Capture per-file ACLs during sync |

### Execution knobs (Phase 5 — destructive)
| Var | Default | Purpose |
|-----|---------|---------|
| `FINSPO_ALLOW_EXECUTION` | `false` | **Hard kill-switch.** Must be `true` before any approved proposal can run. |
| `FINSPO_ARCHIVE_FOLDER_ID` | _(empty)_ | Destination folder (within each item's drive) for `archive` proposals. |

### Backfill one-shot (`cmd/backfill-source-objects`)
| Var | Default | Purpose |
|-----|---------|---------|
| `FINSPO_BACKFILL_ORG_ID` | _(all orgs)_ | Limit backfill to one org |
| `FINSPO_BACKFILL_BATCH_SIZE` | `500` | Rows per batch |
| `FINSPO_BACKFILL_TIMEOUT` | `30m` | Overall timeout |

---

## Execution safety model

Deleting/archiving real SharePoint content passes **three independent gates**:

1. **Kill-switch** — `FINSPO_ALLOW_EXECUTION=true` (off by default). With it off,
   `POST /proposals/:id/execute` returns `503`.
2. **Human approval** — the proposal must be moved to `approved` by an operator
   (`POST /proposals/:id/approve`). Approval is *not* auto-execution.
3. **Explicit trigger** — a deliberate `POST /proposals/:id/execute` call.

Additional safeguards:
- **Delete = recycle bin**, not permanent purge (recoverable in SharePoint).
- **Folders are refused** as execution targets.
- Every per-item outcome is written to `audit_log`; the proposal's terminal
  state (`executed`/`failed`) and `failure_reason` are persisted.
- Missing write scopes surface as a Graph `403` recorded in `failure_reason` —
  the run fails safe, deleting nothing.

---

## Health, readiness, scaling

- `GET /health` — liveness. Always `200` while the process runs.
- `GET /ready` — readiness. Pings Postgres and NATS; `503` if a wired
  dependency is down. Components left unconfigured report `skipped` (still
  ready). Use this as the container healthcheck / load-balancer probe.
- **Horizontal scale:** the background scheduler takes a Postgres advisory lock
  per source (`pg_try_advisory_lock`), so running multiple finspo-api replicas
  is safe — only one replica syncs a given drive at a time. The HTTP API is
  stateless and scales freely.

## Network / platform

- Joins `ingestion-net` (intra-plane) and `inter-plane-bus` (cross-plane;
  declared external — must exist before `docker compose up`).
- Resource envelope in the standard compose: 256 MB / 0.5 CPU limit, 64 MB
  reservation.
