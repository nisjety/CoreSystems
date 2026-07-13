# quickwit-adapter-rs Research Dive

Generated: 2026-06-07
Updated: 2026-07-11 (source/static re-verification plus isolated disposable-Postgres admin-job lifecycle; no shared or destructive runtime operation)

Scope: `apps/Data Plane v2/services/quickwit-adapter-rs` (container `dpv2-quickwit-adapter`, admin HTTP `:9204`), plus its Quickwit dependency (container `dpv2-quickwit`, REST API `:7280`).

## Secure-MVP current state — 2026-07-11

- **Implemented in source:** `/admin/rebuild` requires a cryptographically verified
  principal with dedicated rebuild scope and is tenant-scoped by default. Global
  intent requires separate scope, approval, and break-glass semantics. Non-preview
  work is represented by durable PostgreSQL jobs with scoped idempotency, separate
  approval, a bounded claim/lease, resumable checkpoints, and append-only audit.
- **Destructive containment:** clear requests remain fail-closed and return HTTP
  501 because the adapter does not yet have trustworthy Quickwit task-completion
  proof. No clear/rebuild/cleanup endpoint was invoked during this verification.
- **Static and test evidence (2026-07-11):** `cargo fmt --all -- --check`,
  `cargo check -p quickwit-adapter-rs --all-targets`, and
  `cargo clippy -p quickwit-adapter-rs --all-targets -- -D warnings` passed. The
  final workspace accounting passed 43 Quickwit tests with zero failures and two
  explicit PostgreSQL ignores (ZDR source filtering and admin-job lifecycle).
- **Migration/runtime-isolated evidence (2026-07-11):** after applying only
  `20260711150000_quickwit_admin_jobs.sql` to a uniquely named disposable
  PostgreSQL 16 container, the ignored lifecycle test passed 1/1. It verified
  durable/idempotent submission, two-person approval, claim/checkpoints/completion,
  immutable request fields, and append-only audit. The healthy fixture was then
  removed by its scoped cleanup trap.
- **Built/deployed/reachable/effective:** not re-verified for the 2026-07-11 source
  and migration changes. The evidence above proves implementation and isolated
  behavior only; it does not prove that a rebuilt production-profile image is
  deployed or that the historical live exposure below is closed at runtime.
- **Key isolation:** Compose mounts only Control's public verification file;
  Quickwit adapter no longer receives the signing-key directory.
- **Containment/blockers:** unsigned live-update subscribers are disabled by
  default. Production-profile rebuild/deploy plus the safe auth/admin runtime
  matrix remain required. Quickwit's directly reachable REST boundary described
  below also remains a release concern until current network exposure is
  re-verified. Never invoke a destructive rebuild on a shared stack.
- **Coverage:** after integration tests were rewired to execute the production
  library modules, `cargo llvm-cov` measured line coverage of 97.06% for
  `auth.rs`, 86.73% for `api.rs`, and 52.53% for `jobs.rs`. The jobs result is
  below the 80% target because Docker's local content store failed with an I/O
  error before the disposable-PostgreSQL lifecycle could be added to the LLVM
  profile; the separately executed functional lifecycle remains 1/1 passing.

The remainder is a superseded, sanitized pre-fix audit. Historical destructive
request examples are explanatory only and must not be replayed.

## Historical snapshot (superseded for current state)

`quickwit-adapter-rs` is the sparse read-model adapter for Data Plane v2. It ensures the Quickwit index exists, rebuilds it from canonical Postgres, and applies best-effort live updates over core NATS. `retrieval-engine-rs` consumes it as one of two interchangeable sparse-search backends (the other being a Postgres BM25 fallback).

2026-07-10 live verification confirms the service is real and functioning as documented:

- `dpv2-quickwit-adapter`, `dpv2-quickwit`, and `dpv2-retrieval-engine` are all up and healthy.
- `GET /health` and `GET /readyz` historically returned 200; bodies are omitted.
- Quickwit itself historically answered its version endpoint and served a populated
  `dataplane-corpus` index. Document, split, and byte counts are redacted.
- A direct Quickwit search historically returned a populated derived-corpus hit,
  proving the adapter moved canonical rows into Quickwit. Tenant, URL, text, and
  response body are redacted.
- retrieval's historical readiness output identified the Quickwit-with-Postgres-
  fallback backend; the response body is omitted.
- `retrieval-engine-rs`'s `QuickwitSparseBackend` (`src/search/sparse.rs`) issues a real scoped Quickwit query (`org_id:"..." AND entity_type:"knowledge_unit" AND (...)`), parses real hit shapes (flat or nested `json`/`doc`), and the `FallbackSparseBackend` wrapper is unit-tested for both the happy path and the Quickwit-down fallback path. This is genuine integration, not a mock.
- `cargo fmt -p quickwit-adapter-rs --check` and `cargo clippy -p quickwit-adapter-rs --all-targets -- -D warnings` are both **clean** — no drift, no lint violations, in this crate specifically (the previously-reported fmt/clippy drift lives in embedding-engine-rs / index-engine-rs / retrieval-engine-rs, not here).
- `grep -rniE "TODO|FIXME|mock|stub|fake|placeholder|not.?implemented|hardcod|dummy"` across all 7 source files returns **zero hits**. No stubs, no mocked responses, no placeholder logic anywhere in this crate.

Non-generated file count: 7 Rust source files (`main.rs`, `api.rs`, `config.rs`, `model.rs`, `quickwit.rs`, `rebuild.rs`, `stream.rs`) plus `Cargo.toml` and `Dockerfile`.

## Runtime Shape

- `src/main.rs` — boots Postgres pool, `QuickwitClient`, calls `ensure_index()`, optionally spawns a startup rebuild (`REBUILD_ON_START`, default `false`), connects to NATS, spawns the live subscriber, then serves the admin HTTP router.
- `src/config.rs` — env-driven config via `envy`; no auth-related settings exist (no API key, no shared secret, no allowlist) — see Findings below.
- `src/quickwit.rs` — thin `QuickwitClient` wrapping Quickwit's REST API: `ensure_index`, `clear_index`, `ingest` (NDJSON, `commit=auto`), `delete_by_query`. All calls are plain unauthenticated HTTP to `quickwit:7280` — matches Quickwit's own lack of auth (see below).
- `src/model.rs` — `QuickwitDocument` shape + small serde helpers (`string_field`, `datetime_field`, `acl_tags_field`).
- `src/rebuild.rs` — full and per-org rebuild queries against Postgres (`knowledge_units`, `wiki_page_versions`/`wiki_pages`, `source_objects`, `retrieval_runs`, `wiki_source_logs`), batched (default 500), plus single-row incremental indexers used by the NATS handlers.
- `src/stream.rs` — subscribes to 7 core NATS subjects and dispatches to the rebuild helpers.
- `src/api.rs` — `axum` router: `GET /health`, `GET /readyz`, `POST /admin/rebuild`.

Surface: admin/health HTTP on `9204` only. No product-facing route.

## Schema Cross-Check (live Postgres)

Verified against the live `dpv2-postgres` database (not just read from source):

- `documents.deleted_at` and `source_objects.deleted_at` columns exist as the rebuild queries expect (`d.deleted_at IS NULL`, `deleted_at IS NULL`) — these two queries are **not** affected by the separate, already-known wiki-store `deleted_at` bug.
- `wiki_pages` has no `deleted_at` column at all; this crate correctly uses `page_status <> 'deleted'` instead (matching the live schema), unlike wiki-store-go's repository, which queries a `deleted_at` column that doesn't exist on `wiki_pages` and 500s (that is a wiki-store-go bug, not a quickwit-adapter-rs bug — confirmed by inspecting the live table definition, which has `page_status` and no `deleted_at`).
- `retrieval_runs.zdr_mode` exists and is a nullable `text` column; the historical
  sample contained only disabled-mode rows. Row counts and tenant distribution are
  redacted. See the ZDR finding below for why existing-but-unfiltered mattered.

## API And Relationship Map

- `quickwit-adapter-rs` → Quickwit (`:7280`): sparse read-model ownership (ensure/clear/ingest/delete-by-query).
- `quickwit-adapter-rs` → Postgres: rebuild source of truth (read-only).
- `quickwit-adapter-rs` ← NATS (`documents-api-go`, `embedding-engine-rs`, `wiki-store-go`): live acceleration events. Verified real publishers for 6 of the 7 subscribed subjects (grepped across the plane):
  - `dataplane.knowledge.units.created` → published by `embedding-engine-rs/src/stream/mod.rs`
  - `dataplane.documents.indexed` → published by `documents-api-go` (readiness path) / consumed elsewhere too
  - `dataplane.documents.deleted` → published by `documents-api-go/internal/events/publisher.go`
  - `dataplane.wiki.version.published` → published by `wiki-store-go/internal/events/publisher.go`
  - `dataplane.source_objects.changed` / `dataplane.source_objects.deleted` → published by `documents-api-go/internal/events/publisher.go`
  - `dataplane.search.rebuild.requested` → **no publisher found anywhere in the codebase** (grepped all `.rs`/`.go`/`.sh` under `services/`). This subject is subscribed-to but dead: nothing in the plane currently triggers a live event-driven rebuild this way. Minor finding, not a correctness bug (the admin HTTP path covers the same use case), but it's an inactive integration surface worth pruning or wiring up.
- `retrieval-engine-rs` → Quickwit directly (not through the adapter) for search reads: `QuickwitSparseBackend` in `retrieval-engine-rs/src/search/sparse.rs` talks straight to `:7280`. `quickwit-adapter-rs` never proxies search reads — it is a write/rebuild-only sidecar, and no other service calls its HTTP API at all (grepped for `9204` / `quickwit-adapter` across all Go/Rust services — zero callers besides operator scripts).

## Findings (2026-07-10)

### CRITICAL — `POST /admin/rebuild` has zero authentication and can wipe the entire cross-org sparse index

Live-verified historically, not theoretical: a no-auth request returned HTTP 200.
The request and response body are omitted and must not be replayed.

This was reproduced with no headers, with a garbage `Authorization: Bearer ...` header (accepted identically — the header is simply never read), and with an empty JSON body. `src/config.rs`'s `Config` struct has no API-key/shared-secret field at all, and `src/api.rs`'s `rebuild_index` handler reads only `org_id`/`clear` from the body — there is no auth middleware, no header check, nothing.

This is a strictly worse instance of the same class of bug already flagged plane-wide (graph-index, data-quality, data-orchestrator trusting a caller-supplied `X-Org-ID` with no credential): here there isn't even a spoofable header to check — **any** request is accepted. And the blast radius is destructive, not just a read-scope leak:

- `{"clear":true}` with no `org_id` calls `clear_index()` then re-creates the index, **wiping every org's sparse-search data in one call**. `retrieval-engine-rs` silently falls back to Postgres BM25 for every org until someone notices and reruns the rebuild — a plane-wide, cross-tenant availability/quality regression triggerable by anyone who can reach port 9204.
- A tenant-selected clear intent could selectively wipe one tenant's sparse index;
  the destructive request body is intentionally omitted.
- Even a non-destructive `clear:false` full rebuild (as demonstrated safely above) forces a full unbounded Postgres scan across `knowledge_units`, `wiki_page_versions`, `source_objects`, `retrieval_runs`, and `wiki_source_logs` for every org, batched but uncapped in total volume — an easy unauthenticated resource-exhaustion vector.

Compounding this: `docker-compose.yml` places `quickwit-adapter` on **both** `dpv2-net` and `inter-plane-bus` and publishes `9204:9204` to the host. So this is reachable from any other plane's service on the shared bus, and from the host, not just from within the Data Plane v2 compose network.

Fix direction: require the same `X-Internal-Api-Key` credential that `documents-api-go` already enforces on its own admin/duplicate endpoints (the adapter already has that convention available in the plane — it's just not applied here), and drop the host port publish (`9204:9204`) unless an operator genuinely needs direct host access, in which case gate it behind the same key.

### HIGH — Quickwit's own REST API (`:7280`) is equally unauthenticated and directly reachable, so the adapter's gap can't be fixed by adapter-side auth alone

`dpv2-quickwit` is published on `7280:7280` and its REST API (`/api/v1/indexes/*`, `/api/v1/*/ingest`, `/api/v1/*/delete-tasks`, `/api/v1/*/search`) accepts every call the adapter itself makes with no credential — confirmed by the adapter's own `quickwit.rs` client, which never attaches an auth header, and by directly calling `:7280` in this pass. Anyone who can reach `:7280` can bypass `quickwit-adapter-rs` entirely and ingest, delete, or wipe `dataplane-corpus` directly. Adding auth to the adapter's `/admin/rebuild` endpoint (the CRITICAL finding above) closes one door but leaves this one open; Quickwit itself needs to sit behind a network boundary that isn't published to the host, or a reverse proxy that enforces the same internal key.

### MEDIUM — ZDR (Zero Data Retention) is not consulted when indexing `retrieval_runs` into the sparse read model

`rebuild_retrieval_logs` (`src/rebuild.rs`) selects and indexes **every** `retrieval_runs` row for the target org(s), including the `zdr_mode` column, but never filters on it:

```rust
SELECT trace_id, org_id, query, filters_json, mode_mix_applied, zdr_mode, created_at
FROM retrieval_runs
WHERE ($1::TEXT IS NULL OR org_id = $1)
ORDER BY created_at DESC
```

The historical sample had only disabled-mode rows, so no ephemeral row was observed
in this path. The unfiltered query nevertheless created a latent persistence risk
for any future ephemeral row. Sample counts and query/response content are omitted.

### LOW — `dataplane.search.rebuild.requested` NATS subject has no publisher anywhere in the plane

See relationship map above. Dead integration surface; either wire an actual publisher (e.g. from an ops/admin surface) or remove the subscription and the constant to reduce apparent-but-unused API surface.

## Duplicates, Redundancies, And Inactive Surfaces

Intentional redundancy (historically observed): sparse retrieval has a Quickwit
strategy and a Postgres BM25 fallback, with fallback behavior unit-tested. The
readiness response body is omitted.

Newly confirmed inactive surface: `dataplane.search.rebuild.requested` (see LOW finding above).

## Stubs, Placeholders, And Missing Connections

Re-confirmed this pass via full-crate grep: no TODO/FIXME/mock/stub/fake/placeholder/dummy/not-implemented residue anywhere in `quickwit-adapter-rs`'s 7 source files. The code genuinely does what the docs say. The gap in this service is **authorization**, not fakery — the rebuild/ingest logic itself is real and correctly wired end-to-end (Postgres → Quickwit → retrieval-engine → real search results, all live-verified above).

## API Design And Performance Notes

- Keeping this adapter off the product path is the right call in principle — the problem is that "off the product path" was implemented as "no auth" rather than "internal-key-gated," and it's on the shared inter-plane network and published to the host, so "off the product path" doesn't mean "unreachable."
- `rebuild-on-start` defaults to `false` (good — avoids a rebuild storm on every container restart); this is only ever a risk if an operator flips it on for a large corpus.
- Quickwit was resource-thin in the historical environment, consistent with a
  dev/local footprint. Corpus and split counts are redacted; no capacity conclusion
  should be carried into a production deployment.

## Current Doc Cleanup Read

Keep:

- `docs/quickwit-read-model.md` — re-verified accurate this pass: the rebuild contract (`POST /admin/rebuild`), the entity list, the live-event subject list (minus the dead `search.rebuild.requested` publisher gap noted above), and the smoke test (`scripts/smoke-quickwit-retrieval.sh`) all match the live code and were spot-checked against the running stack.
- `DATA_PLANE_DEEP_DIVE.md`

No delete-ready service-local docs were found in this pass. `docs/quickwit-read-model.md` should get a short addendum pointing at the auth gap so operators don't assume `/admin/rebuild` is safe to leave port-published.

## Historical bottom line (superseded)

`quickwit-adapter-rs` is functionally clean and doing exactly what it claims: real rebuilds, real Quickwit ingestion, real live-updated sparse search consumed by `retrieval-engine-rs`, zero fakery, zero fmt/clippy drift. The genuine problem found this pass is authorization, not correctness: its one HTTP surface (`/admin/rebuild`) is a destructive, cross-org, zero-credential endpoint reachable from the host and from every other plane on the shared bus, its Quickwit dependency has the identical gap one hop away, and its retrieval-log rebuild path doesn't yet respect the plane's own ZDR flag. None of this was caught by the prior "no product-facing API, no stubs found" read — that read was true about fakery and false about safety, because it never load-tested the admin endpoint against zero credentials.

## 2026-07-11 secure-MVP delta (current)

The unauthenticated rebuild conclusion is superseded. Admin operations require
verified dedicated scopes and tenant-bound durable jobs with preview,
idempotency, two-person approval, break-glass for global intent, leases,
checkpoints, audit, and concurrency/rate bounds. Clear remains deliberately 501.
Knowledge by-id/document/rebuild queries exclude deleted, restricted, and
unknown-classification documents; ephemeral/unknown retrieval logs are excluded.
Focused suites pass with four explicit disposable-PostgreSQL ignores across the
combined Quickwit/Index run. Deployment and crash/retry effectiveness are pending.
