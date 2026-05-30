# Durable Request Queue + Run Checkpoints

Cycle 20 / cluster #1 (Phase A — Rust side).

## Why

`InMemoryRequestQueue` loses every queued item on edge restart. For
crawls that span hours and across re-deploys, that's data loss —
in-flight URLs are orphaned and force a full re-fetch. The Postgres
impl persists every state transition so workers can crash and resume
without re-work.

## Schema

Migrations live at `crates/quarry-runtime/migrations/0001_request_queue.sql`.

| Table                        | Purpose                                                |
| ---------------------------- | ------------------------------------------------------ |
| `quarry_request_queues`      | One row per logical queue (per crawl, batch, etc.)     |
| `quarry_queue_items`         | The URLs themselves; lifecycle `queued ↔ in_flight → acked\|failed` |
| `quarry_run_checkpoints`     | `FrontierCheckpoint` snapshots (JSONB) for crash recovery |
| `quarry_retry_events`        | Append-only audit log of every retry decision           |

Every table carries `org_id` (NOT NULL). All read paths are
org-prefixed; `(org_id, request_id)` is the unique key on
`quarry_queue_items` so cross-tenant request-ID collisions are
impossible.

## Lifecycle

```
                    enqueue()
                      │
                      ▼
         ┌──────────────────────┐
         │ status = queued      │◄────────────────┐
         └─────────┬────────────┘                 │
                   │ pop()                        │
                   ▼                              │
         ┌──────────────────────┐                 │
         │ status = in_flight   │                 │
         │ visibility_deadline  │                 │
         └─────┬────────────┬───┘                 │
               │            │                      │
       ack()   │            │ deadline elapsed → reap_expired()
               ▼            ▼                      │
         ┌──────────┐  ┌────────────────────────┘
         │ acked    │
         └──────────┘

       fail_permanently()
               │
               ▼
         ┌──────────┐
         │ failed   │
         └──────────┘
```

`pop()` uses `SELECT FOR UPDATE SKIP LOCKED` so N concurrent workers
race on the same queue without any waiting for a row lock — each skips
rows already claimed by a peer.

## Tenant isolation

Constructed via `PostgresRequestQueue::bind(pool, org_id, name, kind,
visibility_timeout)`. The bind step UPSERTs a queue row keyed on
`(org_id, name)` — so the same logical queue name across two orgs
produces two distinct `queue_id` UUIDs. Verified by the
`tenant_isolation_pop_skips_other_orgs` test.

## Checkpoints

`save_checkpoint(run_id, &FrontierCheckpoint)` serializes the frontier
state (config + queue + seen + visited_count) to JSONB and UPSERTs by
`(org_id, run_id)`. `version` bumps on every save so dashboards can
show "last saved Ns ago".

Orchestrator wiring (cycle 21 follow-up): call `save_checkpoint` every
N pages from inside the Temporal activity. On worker restart, call
`load_checkpoint(run_id)` and replay from the recovered frontier
without re-fetching seen URLs.

## Configuration

Behind cargo feature `postgres-queue` so default builds don't pull
`sqlx` + `libpq` toolchain.

```bash
cargo build -p quarry-runtime --features postgres-queue
```

Production wiring (cycle 21 follow-up) will add `QUARRY_EDGE__DATABASE_URL`
and a `data_plane_queue_backend = "memory" | "postgres"` config knob
that pivots `AppState.request_queue` between the two impls.

## Tests

Five integration tests in `crates/quarry-runtime/src/postgres_queue.rs`:

- `enqueue_then_pop_roundtrips`
- `priority_ordering_pops_high_first`
- `reap_expired_returns_in_flight_to_queued`
- `tenant_isolation_pop_skips_other_orgs`
- `checkpoint_save_load_roundtrip`

All gracefully skip when `DATABASE_URL` is unset so default CI stays
green. Local dev can run `docker run -d -p 5432:5432 postgres:16-alpine`
and `DATABASE_URL=postgres://... cargo test -p quarry-runtime --features postgres-queue` for real coverage.

## What's still pending (cluster #1 Phase B)

- **Go control surface** — `LeaseNextURLs` / `AckURL` / `NackURL` REST
  endpoints on `quarry-control` so external Temporal workflows can drive
  the queue without owning a Postgres connection directly.
- **Temporal signal-back** — when an activity marks a URL as
  permanently-failed, signal the workflow so it can record the failure
  in `quarry_retry_events`.
- **Edge wiring** — pick `InMemoryRequestQueue` vs `PostgresRequestQueue`
  at boot based on a config flag.

These are tracked as cluster #1 Phase B in the next cycle.

# Profile Restore Probe (cycle 20 / cluster #13)

`POST /v1/profiles/:id/restore_probe { url }` validates that a saved
profile snapshot has the state needed to drive an authenticated
session at the given URL. Returns:

```jsonc
{
  "profile_id": "01H...",
  "url": "https://example.com/dashboard",
  "restorable": true,
  "cookies_count": 12,
  "local_storage_count": 4,
  "session_storage_count": 0,
  "indexed_db_count": 2,
  "has_user_agent": true,
  "has_viewport": true,
  "locale": "en-US",
  "timezone": "Europe/Stockholm"
}
```

The probe does NOT spin up a browser yet — that's a high-cost
operation deferred to cycle 21. For now the response is enough for the
UI to flag "profile likely expired, re-login" without paying for a
real navigation.

`SessionSnapshot.indexed_db: Vec<IndexedDbEntry>` is the new field
shipped this cycle; drivers that don't yet support IDB capture (Kernel,
Browserless static) leave it empty — the field round-trips cleanly
via `#[serde(default, skip_serializing_if = "Vec::is_empty")]`.
