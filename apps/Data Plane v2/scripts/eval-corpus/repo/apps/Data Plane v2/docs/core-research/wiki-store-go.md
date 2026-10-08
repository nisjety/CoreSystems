# wiki-store-go Research Dive

Generated: 2026-06-07
Live-verified: 2026-07-10 (re-verifies and supersedes the 2026-06-07 pass; do not read the original snapshot as current)

Scope: `apps/Data Plane v2/services/wiki-store-go`

## 2026-07-15 final isolated acceptance delta

The final rebuilt service passed HTTP 401/401/200/403 and every WikiService gRPC
family. Same-tenant writes reached exact signed-ZDR/approval guards and
cross-tenant calls reached `request tenant does not match verified identity`.
Migrations/startup and supported signed Wiki broker delivery/redelivery pass in
isolation. Shared rollout and database-backed outbox coverage remain pending.

## 2026-07-15 isolated runtime delta

The current-source image `e7c75b756544` carries revision
`eeebd0bc98c66434936460020958891066eb05fd`. Its disposable HTTP four-shape
matrix returned 401/401/200/403 after the bearer fixture was corrected to carry
the exact wiki scopes. Migrations/startup passed in this isolated stack. Signed
Wiki→JetStream PubAck→embedding redelivery and the shared deployment remain
pending.

## Secure-MVP current state — 2026-07-11

- **Implemented:** HTTP and gRPC now require verified RS256/JWKS claims, pin tenant
  identity, and enforce `wiki.read`, `wiki.write`, `wiki.approve`, and
  `wiki.maintenance.write` scopes. The additive
  `20260710090000_reconcile_wiki_runtime_schema.sql` migration reconciles page,
  source-log, and maintenance-log runtime contracts. Wiki publication now uses
  a producer-scoped RS256 envelope and the additive
  `20260711180000_wiki_event_outbox.sql` transactional outbox: page/version and
  event intent commit atomically; replicas claim with leased
  `FOR UPDATE SKIP LOCKED`; failed sends are retried; only a valid JetStream
  PubAck marks an intent delivered. Production startup requires NATS, a valid
  signing key, and a compatible `DATAPLANE_WIKI` WorkQueue stream.
- **Tested:** the full Go suite passed with `-race`; `go vet ./...` passed,
  `govulncheck` reported no reachable vulnerability, and the disposable migration
  integration test passed for the earlier schema reconciliation. On 2026-07-11,
  outbox unit/contract tests passed, signer coverage measured 88.1%, and both
  disposable PostgreSQL outbox tests compile and skip safely without
  `WIKI_TEST_DATABASE_URL`. The full outbox database lifecycle remains pending.
- **Built/reachable/effective in isolation:** the revised image and migrator built
  locally with verified revision/build labels; isolated migrations/startup and
  the authenticated endpoint matrix passed. No shared deployment occurred.
- **Blockers:** run the disposable PostgreSQL outbox lifecycle and prove signed
  Wiki→JetStream PubAck→embedding consumption end to end, then perform a
  controlled shared deployment.
  Process readiness does not prove route, migration, outbox, or downstream
  embedding effectiveness.

The remainder is a superseded, sanitized pre-fix audit retained for root-cause
history. Its header-only and schema-500 observations do not describe verified
rebuilt behavior.

## Historical pre-fix live audit (superseded)

Container `dpv2-wiki-store` was up and `healthy` (`docker ps`, 14h uptime, port 8011), and `/readyz` returned `200`. That health check only proves the process can reach Postgres — it does not probe the actual page routes, so it stayed green throughout every failure reproduced below.

Live-reproduced, with exact evidence, in this pass:

1. **CONFIRMED — `GET /v1/wiki/pages` (list pages) always returns HTTP 500.** Root cause is a genuine code bug, not a skipped migration: `wiki_pages` never had a `deleted_at` column, in `init.sql` or in any numbered migration, yet `ListPages` unconditionally filters on it.
2. **NEW (not in the 2026-07-02 baseline) — the same class of schema/code mismatch also breaks the wiki source-log and maintenance-log surfaces.** `wiki_source_logs` and `wiki_maintenance_logs` were defined in `init.sql` with an older column shape (`original_chunks`/`processing_model`/`synthesis_prompt_hash` and `issue_type`/`issue_details`/`proposed_fix`/`issue_status` respectively) that predates the current repo-layer contract (`org_id`, `source_type`, `source_ref`, `sync_status`, `details` / `org_id`, `action`, `actor`, `details`). Every one of `CreateSourceLog`, `ListSourceLogs`, `CreateMaintenanceLog`, `ListMaintenanceLogs` is dead on arrival against the live schema.
3. **NEW — `POST /v1/wiki/maintenance/sweep` (the D4+D5 "honest partial success" batch endpoint) returns HTTP 200 with `accepted:0` for every item**, because it hits the same broken `wiki_maintenance_logs` insert. It never surfaces as a 500, so a caller or smoke test that only checks the HTTP status would see a "successful" 200 response that silently did nothing — the same "fake green" contract-degradation shape already flagged elsewhere in this plane's 2026-07-10 audit (the v3 gateway knowledge fan-out).
4. **NEW — `POST /v1/wiki/operating-map/refresh` (AI Operating Map refresh) also returns HTTP 500**, because `RefreshOperatingMap` → `operatingMapEvidenceRefs` → `WikiRepo.ListPages` internally, so finding (1) has a second, less obvious blast radius beyond the sidebar page list.
5. **CONFIRMED — org scoping on every `/v1/wiki/*` route is `X-Org-ID`-header-only, no credential.** `OrgIDMiddleware` in `internal/handler/wiki.go` reads `X-Org-ID` and nothing else; `cmd/main.go` mounts every route under `r.Route("/v1/wiki", ...)` with only that middleware. This is the same trust-boundary shape already flagged for `graph-index`, `data-quality`, and `data-orchestrator` (and, before that, Control Plane's session-core/user-core `X-User-Role` gap) — wiki-store-go is a fourth live instance of it, not previously called out by name in the plane audit.
6. **CONFIRMED — `gofmt -l` reports drift in `services/wiki-store-go/internal/grpcserver/server.go`.** Consistent with the plane-audit addendum's "Go formatting drift spans durable Data services" line, now pinned to the exact file for this service.
7. Everything else probed live works as designed: `CreatePage`, `GetPage`, `GetPageByPath`, `CreateVersion`/`ListVersions`, `DiffVersions`, `GetBacklinks`, `SubmitProposal`, `GetOperatingMap` (empty-state), and NATS publish-on-write (`wiki publisher attached` in container logs) all returned correct 2xx responses in this pass.

### Sanitized reproduction: `GET /v1/wiki/pages` → 500

The historical test created a transactionally isolated page (201), read it by
path (200), and then listed pages (500). Organization, page ID, title, path,
content, and all response bodies are redacted.

Historical schema inspection showed the following column shape. Connection and
credential material is omitted:

```
$ docker exec dpv2-postgres psql -U dataplane -d dataplane -c '\d wiki_pages'
                                   Table "public.wiki_pages"
       Column       |           Type           | Nullable |         Default
--------------------+--------------------------+----------+-------------------------
 page_id            | text                     | not null | gen_random_uuid()::text
 org_id             | text                     | not null |
 workspace_id       | text                     | not null |
 title              | text                     | not null |
 path               | text                     | not null |
 current_version_id | text                     |          |
 page_status        | text                     |          | 'draft'::text
 backlinks          | jsonb                    |          |
 metadata           | jsonb                    |          |
 created_at         | timestamptz              | not null | now()
 updated_at         | timestamptz              | not null | now()
```

No `deleted_at` column exists on the live table. `internal/repo/wiki_repo.go`'s `ListPages` (all four query-shape branches, `wiki_repo.go:74-134`) unconditionally appends `AND deleted_at IS NULL` to both the `COUNT(*)` and the `SELECT` query, e.g.:

```go
// wiki_repo.go:120-134 (the no-filter "default" branch, hit by a bare GET /v1/wiki/pages)
err = r.pool.QueryRow(ctx,
    `SELECT COUNT(*) FROM wiki_pages WHERE org_id = $1 AND deleted_at IS NULL`,
    orgID,
).Scan(&total)
...
rows, err = r.pool.Query(ctx, `
    SELECT page_id, org_id, workspace_id, title, path, current_version_id,
           page_status, backlinks, metadata, created_at, updated_at
    FROM wiki_pages
    WHERE org_id = $1 AND deleted_at IS NULL
    ORDER BY updated_at DESC LIMIT $2 OFFSET $3
`, orgID, limit, offset)
```

Postgres returned SQLSTATE 42703 for the missing column. `ListPages` wrapped it and
the handler returned a generic 500; the response body is omitted. The wrapped error
was not logged, which made source/schema inspection necessary.

`GetPage` and `GetPageByPath` (`wiki_repo.go:156-186`) do **not** reference `deleted_at` at all, which is why single-page reads succeed while the list view fails — the bug is narrowly scoped to the four `ListPages` query branches (and anything that calls `ListPages` internally, see finding 4).

### Root cause: code bug, not a missing/unapplied migration

This is a genuine schema/code mismatch introduced at authoring time, not a migration that exists but was never run:

- `infra/postgres/migrations/` has **zero** files touching `wiki_pages`, `wiki_source_logs`, or `wiki_maintenance_logs`. The wiki tables are defined only in `infra/postgres/init.sql` (run once on first container start) — no follow-up migration for any of them exists anywhere in the tree.
- `git log -S 'CREATE TABLE IF NOT EXISTS wiki_pages' -- infra/postgres/init.sql` and `git log -S 'deleted_at IS NULL' -- services/wiki-store-go/internal/repo/wiki_repo.go` both resolve to the **same commit**, `1eecf8d0` ("feat(data-plane,model-plane): RDI content-update freshness + context-engine integration", 2026-05-30). The table DDL and the `ListPages` code that assumes a `deleted_at` column landed together, in one commit — the column was simply never added to the DDL in the commit that introduced the filter that depends on it.
- The soft-delete convention (`deleted_at TIMESTAMPTZ`, nullable, no default) is real and in active use elsewhere in the same `init.sql` — `documents` (line ~83) and `source_objects` (line ~30 of `infra/postgres/migrations/20260526090000_add_source_objects_for_quickwit.sql` and again in `init.sql`) both have it, and `documents`' unique index and count logic actively filter on it. `wiki_pages` was written immediately after `documents` in the same file (`init.sql:264-283`) but the column was dropped somewhere between copying the pattern and finalizing the DDL.
- `git status --porcelain` on `services/wiki-store-go/` and `infra/postgres/` is clean — this is not stray uncommitted WIP; it has been sitting in committed history since 2026-05-30.

Conclusion: **write the missing migration** (`ALTER TABLE wiki_pages ADD COLUMN deleted_at TIMESTAMPTZ;`, backfill nothing since no soft-delete path currently sets it) rather than removing the `deleted_at` filter from the code — the intent (support soft-deleted pages, matching `documents`/`source_objects`) is coherent with the rest of the schema; only the DDL is missing.

### The wider pattern: `wiki_source_logs` and `wiki_maintenance_logs` have the identical mismatch shape

Live schema for both tables:

```
$ docker exec dpv2-postgres psql -U dataplane -d dataplane -c '\d wiki_source_logs'
      Column           |    Type     | Nullable |         Default
-----------------------+-------------+----------+-------------------------
 log_id                | text        | not null | gen_random_uuid()::text
 page_id               | text        | not null |
 original_chunks       | jsonb       |          |
 processing_model      | text        |          |
 synthesis_prompt_hash | text        |          |
 metadata              | jsonb       |          |
 created_at            | timestamptz | not null | now()

$ docker exec dpv2-postgres psql -U dataplane -d dataplane -c '\d wiki_maintenance_logs'
    Column     |    Type     | Nullable |         Default
---------------+-------------+----------+-------------------------
 log_id        | text        | not null | gen_random_uuid()::text
 page_id       | text        | not null |
 issue_type    | text        |          |
 issue_details | jsonb       |          |
 proposed_fix  | text        |          |
 issue_status  | text        |          | 'open'::text
 metadata      | jsonb       |          |
 created_at    | timestamptz | not null | now()
 resolved_at   | timestamptz |          |
 kind          | text        |          |
 detected_at   | timestamptz | not null | now()
```

But `wiki_repo.go` inserts/reads columns that were never added to either table:

```go
// wiki_repo.go:413-416 — org_id, source_type, source_ref, sync_status, details don't exist on wiki_source_logs
_, err := r.pool.Exec(ctx, `
    INSERT INTO wiki_source_logs (log_id, org_id, page_id, source_type, source_ref, sync_status, details)
    VALUES ($1, $2, $3, $4, $5, $6, $7)
`, id, input.OrgID, input.PageID, input.SourceType, input.SourceRef, input.SyncStatus, details)

// wiki_repo.go:467-470 — org_id, action, actor don't exist on wiki_maintenance_logs
_, err := r.pool.Exec(ctx, `
    INSERT INTO wiki_maintenance_logs (log_id, org_id, page_id, action, actor, details)
    VALUES ($1, $2, $3, $4, $5, $6)
`, id, input.OrgID, input.PageID, input.Action, input.Actor, details)
```

The sanitized historical matrix returned HTTP 500 for source-log create/list and
maintenance-log create/list. Organization, page/source identifiers, request
payloads, and response bodies are redacted.

The `MaintenanceSweep` batch handler (`/v1/wiki/maintenance/sweep`) surfaces the underlying Postgres error verbatim in its per-item error array instead of masking it, which is how the exact SQLSTATE was captured live:

The historical maintenance-sweep request returned HTTP 200 while accepting no
items; its per-item error carried SQLSTATE 42703. Request and response bodies are
redacted.

That last response is the sharpest example of a "fake green" contract in this service: HTTP 200, a well-formed envelope, and `accepted:0` for literally every item that will ever be posted to it — a caller or CI smoke test asserting on status code alone would never notice this endpoint does nothing.

`wiki_page_versions` and `wiki_proposals` do **not** have this mismatch — their live columns line up with what the repo code selects/inserts, and `CreateVersion`, `ListVersions`, `DiffVersions`, `SubmitProposal` all worked correctly live in this pass.

### Cascading impact on the AI Operating Map feature

`RefreshOperatingMap` (`wiki_repo.go:613-630`) calls `r.operatingMapEvidenceRefs`, which calls `r.ListPages(ctx, orgID, "", "", limit+1, 0)` to gather evidence page IDs before generating a proposal. Because that hits the same broken `ListPages` default branch:

The historical operating-map refresh request returned HTTP 500. Tenant and
response body are redacted.

`GetOperatingMap` (read-only, empty state) and `SubmitOperatingMapProposal`/`ReviewOperatingMapProposal` (which don't call `ListPages`) worked fine live. Only the refresh entry point is affected, but it is the entry point a real operator or scheduled job would use to regenerate the map from current wiki content — so this bug has a second live blast radius beyond the sidebar page list called out in the plane baseline.

### Org-scoping trust gap (X-Org-ID only)

```go
// internal/handler/wiki.go:34-44
func OrgIDMiddleware(next http.Handler) http.Handler {
    return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
        orgID := r.Header.Get("X-Org-ID")
        if orgID == "" {
            writeError(w, http.StatusBadRequest, "X-Org-ID header required")
            return
        }
        ctx := context.WithValue(r.Context(), orgIDKey, orgID)
        next.ServeHTTP(w, r.WithContext(ctx))
    })
}
```

```go
// cmd/main.go:86-110 — every /v1/wiki/* route is behind this middleware and nothing else
r.Route("/v1/wiki", func(r chi.Router) {
    r.Use(handler.OrgIDMiddleware)
    ...
})
```

There is no bearer/JWT check, no Control Plane consultation, and no verification that the caller is actually a member of the org named in `X-Org-ID` — confirmed live by successfully creating/reading/versioning pages under an arbitrary `[redacted-test-org]` value with no prior provisioning. This is the same shape already flagged for `graph-index` (:9203), `data-quality` (:8013 cost summaries), and `data-orchestrator` (:8012 stale-embedding endpoints) in the 2026-07-10 plane audit, and the same class of bug already fixed for session-core bearer auth and flagged for user-core's `X-User-Role` trust gap in Control Plane's own 2026-07-10 audit (`apps/Control Plane/docs/core-research/plane-audit-2026-07-10.md`, finding #1). wiki-store-go should be added to that remediation list explicitly — it was not named in the original plane-level addendum.

### Formatting drift

```
$ gofmt -l services/wiki-store-go/
services/wiki-store-go/internal/grpcserver/server.go
```

Confirms and pins the plane-audit addendum's "Go formatting drift spans durable Data services" line to this exact file for wiki-store-go.

### gRPC surface note

The gRPC `WikiService` (`proto/wiki/v1/*.proto`) has no `ListPages` RPC at all — `ListPageVersions`, `GetPageSources`, `ListMaintenanceIssues`, `GetBacklinks`, `CreatePage`, `UpdatePageVersion`, `SubmitProposal`, `ReviewProposal`, `GetPage`, `GetPageByPath` are the full set. The paginated page-list surface (the one with the `deleted_at` bug) only exists over HTTP, added later per the code comment ("Wave 3.1 / Wave 11.C-b close: paginated wiki page enumeration for the verevon sidebar, which previously had to fall back to localStorage bookmarks"). The verevonv3 gateway proxies it directly: `apps/Frontend Plane/verevonv3/apps/gateway/src/domains/knowledge/wiki.rs:22` builds `{wiki_store_url}/v1/wiki/pages{query}` and forwards it verbatim from `GET /api/v1/wiki/pages` (`domains/knowledge.rs:112`) — so this 500 is reachable end-to-end from the real product surface, not just the raw service port.

## Runtime Shape

Key runtime entrypoints:

- `cmd/main.go`
  - Postgres, mandatory production signed JetStream/outbox publisher, HTTP server (chi router, port 8011), gRPC server (port 50054 internally)
- `internal/repo/wiki_repo.go`
  - page, version, proposal, source-log, maintenance-log, and AI Operating Map persistence — all in one file, ~1,110 lines
- `internal/handler/wiki.go`
  - HTTP wiki routes, `OrgIDMiddleware`, diff engine (LCS-based line diff)
- `internal/grpcserver/*`
  - gRPC `WikiService` (narrower surface than HTTP, see gRPC surface note above)

Primary HTTP surfaces (`cmd/main.go:86-110`):

- `POST /v1/wiki/pages`, `GET /v1/wiki/pages` (broken — see above), `GET /v1/wiki/pages/by-path`, `GET /v1/wiki/pages/{pageID}`
- `POST /v1/wiki/pages/{pageID}/versions`, `GET /v1/wiki/pages/{pageID}/versions`, `GET /v1/wiki/pages/{pageID}/diff`
- `GET /v1/wiki/pages/{pageID}/backlinks`
- `POST /v1/wiki/pages/{pageID}/proposals`, `POST /v1/wiki/proposals/review`
- `POST /v1/wiki/pages/{pageID}/source-logs` (broken), `GET /v1/wiki/pages/{pageID}/source-logs` (broken)
- `POST /v1/wiki/pages/{pageID}/maintenance-logs` (broken), `GET /v1/wiki/pages/{pageID}/maintenance-logs` (broken)
- `POST /v1/wiki/maintenance/sweep` (fake-green — see above)
- `GET /v1/wiki/operating-map`, `POST /v1/wiki/operating-map/proposals`, `POST /v1/wiki/operating-map/proposals/{proposalID}/review`, `POST /v1/wiki/operating-map/agent-blueprints`, `POST /v1/wiki/operating-map/refresh` (broken — see cascading impact above)
- gRPC `WikiService`

## API And Relationship Map

Current relationships:

- Frontend/Application Plane (verevonv3 gateway `domains/knowledge/wiki.rs`) -> `wiki-store-go`
  - page and version product surfaces, proxied same-shape over `/api/v1/wiki/*`
- `wiki-store-go` -> Postgres
  - canonical wiki truth (confirmed live: `dataplane` DB on `dpv2-postgres:5432`, internal port; host-mapped `5442`)
- `wiki-store-go` -> NATS
  - `dataplane.wiki.version.published` is now an RS256-signed, tenant/user/ZDR-bound intent inserted in the page/version transaction. A leased worker requires JetStream PubAck before delivery state. This is implemented and unit-tested, not yet rebuilt or runtime-verified.
- `embedding-engine-rs` depends on its publish flow for `wiki_block_embeddings`

## Duplicates, Redundancies, And Inactive Surfaces

No explicit duplicate source residue found. The main operational split is transport (HTTP vs gRPC), not ownership, and the two transports are no longer at parity: HTTP has page-list, source-logs, maintenance-logs, maintenance-sweep, and the full AI Operating Map surface; gRPC does not expose any of those.

## Stubs, Placeholders, And Missing Connections

- Production posture refuses startup when NATS, the signing key, JetStream, or
  the compatible wiki stream cannot initialize. Only the explicit isolated
  three-gate development posture may start with publication disabled; intents
  remain durable in PostgreSQL for later delivery.
- Four HTTP routes (`source-logs` GET/POST, `maintenance-logs` GET/POST) and one cascading route (`operating-map/refresh`) are non-functional against the live schema — see findings above. This is a stronger claim than "stub": these are wired end-to-end (handler → repo → SQL) but the SQL cannot succeed against the current table shape.

## API Design And Performance Notes

- Wiki ownership is cleanly centralized in one repo file; consistency risk across HTTP/gRPC is lower than in split implementations for the surfaces both transports share.
- The four static query shapes in `ListPages` (switch on `workspaceID`/`status` presence) avoid dynamic SQL string formatting — a deliberate tenant-isolation safeguard per the code comment — but that same rigidity means the `deleted_at` bug is baked into all four branches identically rather than isolated to one.
- The `writeError` pattern historically swallowed the underlying Postgres error
  except in `MaintenanceSweep`. Other failures returned opaque 500 responses with
  no correlation ID, while the wrapped error was not logged server-side.

## Current Doc Cleanup Read

Keep:

- `DATA_PLANE_DEEP_DIVE.md`
- `docs/schemas/wiki_events.md`

## Historical bottom line (superseded)

`wiki-store-go` is a real source-of-truth service for page/version data, and single-page reads, version history, diffing, proposals, and NATS publish-on-write are genuinely live and correct. But three of its table-backed feature surfaces — page listing, source-log tracking, and maintenance-log tracking (plus the AI Operating Map refresh path, which depends on page listing) — are dead on arrival against the live schema, from a code/DDL mismatch that has sat in committed history since 2026-05-30 (commit `1eecf8d0`), not from a migration that was written and never applied. The `MaintenanceSweep` batch endpoint additionally masks its own 100% failure rate behind an HTTP 200. None of this is caught by the container healthcheck, which only calls `/readyz`. Fix requires one additive migration (`wiki_pages.deleted_at`) plus a decision on `wiki_source_logs`/`wiki_maintenance_logs`: either migrate those two tables to the contract the Go code already expects, or roll the code back to the older `issue_type`/`processing_model`-shaped contract — right now neither side agrees with the other, and every write/read against those two tables fails.
