# finspo-core — Architecture

## Purpose

finspo is the SharePoint **ingestion + governance** connector. It maintains a
durable, queryable mirror of SharePoint drive contents and layers governance
(duplicate detection, storage analytics, reviewed cleanup) on top — feeding the
Data Plane and emitting events for the rest of the platform.

Design tenets (from the project charter):
- Use **Graph delta sync**, never repeated full recursive crawls.
- Persist source-object state: org, site, drive, item, parent, path, MIME, size,
  modified, eTag/cTag, web URL, hashes, deleted state, delta cursor.
- Capture **Graph-native hashes** (`quickXorHash`, `sha1Hash`); `sha256Hash` is
  not exposed by Graph and is intentionally absent.
- Capture **normalized ACL summaries** for reporting, but **re-verify access at
  retrieval time** — the stored summary is never authoritative for authz.
- Treat delete/archive as **reviewed, audited workflows**, never automatic.
- AI (near-duplicate / semantic) lives in the **Model Plane**, not here.

## Component map

```
                         ┌────────────────────────────────────────────┐
                         │                 finspo-api                  │
                         │                                             │
  Microsoft Graph  ◀────▶│  sharepoint/   browser · delta · perms ·    │
  (via integration-core  │                mutation clients             │
   delegated token)      │                                             │
                         │  sync/         Engine (delta loop)          │
                         │                Scheduler (ticker + adv lock)│
                         │                Executor (approved proposals)│
                         │                                             │
   Data Plane  ◀─────────│  dataplane/    SourceObjectClient (sink)    │
   (source objects)      │                                             │
                         │  store/        pgx repositories             │
                         │  events/       NATS publisher               │
                         │  api/          Fiber HTTP handlers          │
                         └───────┬───────────────────────┬─────────────┘
                                 │                       │
                          PostgreSQL `finspo`      NATS `finspo.*`
```

## Data flow

### Ingestion (per drive)
1. A **source** (org + site + drive) is registered via `POST /api/v1/sources`.
2. The **Scheduler** ticks every `FINSPO_SYNC_INTERVAL`, lists enabled sources,
   and (holding a per-source advisory lock) calls the **Engine**.
3. The **Engine** (`sync/delta.go`):
   - Resumes from the persisted `@odata.deltaLink`, else starts a fresh
     `/drives/{id}/root/delta`.
   - Follows `@odata.nextLink` pages (capped by `PageLimit`).
   - For each item: **upsert** to `items` (with hashes), or **soft-delete** on a
     `deleted` facet tombstone.
   - For files (not folders), if `FINSPO_CAPTURE_PERMISSIONS=true`, fetches the
     ACL and writes a normalized summary to `permissions`.
   - Mirrors each change to the **Data Plane** sink and **publishes** events.
   - Persists the new `deltaLink` + sync status to `delta_cursors`.

### Governance
- **Analytics** (`store/analytics.go`) runs read-only aggregations over `items`:
  largest, inactive (by `modified_at`), per-site footprint, and duplicate groups
  (by content hash).
- **Recommendations** (`store/recommendations.go`) turn analytics into pre-filled
  proposal **drafts** (keep newest duplicate, archive stale) — never persisted.
- An operator creates a **proposal** (`review_proposals`), which moves through a
  state machine: `pending → approved | rejected → executed | failed`.
- The **Executor** (`sync/executor.go`) acts on an *approved* proposal: deletes
  (recycle bin) or moves (archive) each item via the Graph mutation client,
  writes per-item `audit_log` rows, and sets the terminal proposal state.

## Database schema (PostgreSQL `finspo`)

| Table | Role |
|-------|------|
| `sources` | One row per (org, site, drive) being synced. |
| `delta_cursors` | Per-source delta token + last-sync status (1:1 with sources). |
| `items` | Source-object state for every observed DriveItem. Soft-deleted via `deleted_at`. Hashes in `quick_xor_hash` / `sha1_hash`. |
| `permissions` | Normalized ACL summary per item, deduped by a stable `perm_hash`. Reporting only. |
| `review_proposals` | Mutable governance state machine (delete/archive proposals). |
| `audit_log` | Append-only history of governance actions (proposal + per-item). |
| `schema_migrations` | Applied migration versions. |

Migrations live in `internal/db/migrations/` (`0001_init.sql`,
`0002_review_proposals.sql`), embedded in the binary and applied at boot.

## Events (NATS, prefix `finspo`)

| Subject | Payload | Fires when |
|---------|---------|-----------|
| `finspo.item.upserted` | `ItemUpserted` | An item is created/updated during sync |
| `finspo.item.deleted` | `ItemDeleted` | A delta tombstone soft-deletes an item |
| `finspo.source.synced` | `SourceSynced` | A drive's delta chain completes |
| `finspo.proposal.created` | `ProposalLifecycle` | A proposal is created |
| `finspo.proposal.approved` | `ProposalLifecycle` | A proposal is approved |
| `finspo.proposal.rejected` | `ProposalLifecycle` | A proposal is rejected |
| `finspo.proposal.executed` | `ProposalLifecycle` | An approved proposal executed OK |
| `finspo.proposal.failed` | `ProposalLifecycle` | Execution failed (≥1 item) |

Publishing is best-effort and never blocks ingestion; with `NATS_URL` empty the
publisher is a no-op.

## Graph clients (`internal/sharepoint`)

| Client | Endpoint(s) | Used by |
|--------|-------------|---------|
| `GraphBrowser` | `/sites`, `/sites/{id}/drive/root/children` | Discovery API |
| `DeltaClient` | `/drives/{id}/root/delta` | Sync engine |
| `PermissionsClient` | `/drives/{id}/items/{id}/permissions` | ACL capture |
| `MutationClient` | `DELETE` / `PATCH /drives/{id}/items/{id}` | Executor |

All resolve a delegated token per-org via `HttpAccessTokenProvider` →
integration-core.

## Testability pattern

Every cross-boundary dependency is a small interface defined where it is *used*
(`sync.DeltaFetcher`, `sync.ItemStore`, `sync.Mutator`, `api.ProposalStore`,
etc.), so handlers and the engine/executor are unit-tested with fakes and
`httptest` — no live Postgres or Graph required. Store SQL is exercised against
a real database in integration runs (skipped when no test DSN is present).

## Deliberate non-goals / future work

- **OTEL exporter** — endpoint is captured and logged; metric/trace export is
  not yet wired (zerolog structured logging is in place).
- **App-only Graph auth** — current flow is delegated (user-context); org-wide
  governance will want application permissions.
- **Cross-drive archive** — archive moves are same-drive only.
- **Approve→auto-execute** — intentionally omitted; execution is a separate
  explicit, kill-switched step.
