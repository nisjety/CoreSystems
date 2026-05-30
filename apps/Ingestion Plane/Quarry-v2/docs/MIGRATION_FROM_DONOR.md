# Migration from Donor Codebases

Cycle 29 / cluster #15.

Quarry v2 was built by donor-extracting + re-architecting code from
three places:

1. **Quarry v1 (Go)** — `apps/Ingestion Plane/Quarry/` — the
   first-generation crawl/scrape service.
2. **Cursor's scraping internals** — donated pre-cycle 14 (Sonic +
   embedded-extraction stack).
3. **Firecrawl OSS** — selected patterns for the per-host scheduler
   + retry classifier.

This doc maps each donor concept onto its v2 home so operators
familiar with the original codebases can find equivalents.

## Quarry v1 → v2 module map

| Quarry v1 (Go)                    | Quarry v2 (Rust)                                         | Notes |
| --------------------------------- | -------------------------------------------------------- | ----- |
| `internal/driver/`                | `crates/quarry-runtime/src/{fetch,tls_driver,browser_driver,fallback_driver}.rs` | Trait-based; selection via `DriverRegistry` |
| `internal/scraper/`               | `crates/quarry-runtime/src/pipeline.rs`                  | `PageRunner` end-to-end |
| `internal/pipeline/`              | Subsumed into `pipeline.rs`                              | |
| `internal/crawl/runner.go`        | `crates/quarry-runtime/src/{crawl_frontier,crawl_ranker,crawl_signals}.rs` | Frontier + ranker split for testability |
| `internal/agent/`                 | `crates/quarry-runtime/src/agent_loop.rs`                | Model-Plane-backed agent |
| `internal/orchestrator/`          | `services/quarry-control/internal/dispatcher/`           | Go-side dispatcher; Temporal SDK pending cycle 30 |
| `pkg/contracts/`                  | `crates/quarry-core/src/{contracts,resources,job_history,…}.rs` + `pkg/quarrycontracts/` | Shared by both planes |
| HTTP routes                       | `crates/quarry-edge/src/*_routes.rs`                     | Axum; JWT-gated |
| Postgres tables                   | `crates/quarry-runtime/migrations/*.sql` + `services/quarry-control/internal/store/pg/migrations/*.sql` | Two owners — runtime + control |
| Event log                         | `crates/quarry-core/src/job_history.rs` + `postgres_event_history.rs` | Canonical envelope; cycle 24 |

## v1 → v2 wire shapes

### Scrape request
Same JSON keys; v2 adds `policy` (RunPolicy), `preset`, `output_profile`.
Old callers continue to work — new fields default to legacy behavior.

### Scrape response (`NormalizedOutput`)
v2 adds `determinism: Option<DeterminismStamp>` (cycle 21) and stamps
content fingerprint via `blake3` instead of v1's MD5.

### Event envelope
v1 used ad-hoc per-transport shapes. v2 emits the canonical
`JobHistoryEvent` (cycle 24) on every transport.

## Cursor scraping internals → v2

| Cursor concept                 | v2 home                                              |
| ------------------------------ | ---------------------------------------------------- |
| Sonic-backed autocomplete       | Deferred — `apps/Ingestion Plane/autocomplete-core/` (cycle 14) |
| Embedded DOM cleanup            | `crates/quarry-transform/src/{readability,images,attributes,sitemap}.rs` |
| Search-stack consensus          | `crates/quarry-runtime/src/smart_router.rs` + `serp.rs` (cycle 18-19) |
| Tantivy local corpus            | `crates/quarry-runtime/src/local_index.rs` (cycle 19)  |

## Firecrawl OSS → v2

| Firecrawl concept              | v2 equivalent                                         |
| ------------------------------ | ----------------------------------------------------- |
| Per-host concurrency            | `crates/quarry-runtime/src/host_scheduler.rs` (cycle 21) |
| Retry classifier                | `crates/quarry-runtime/src/retry.rs` + `policy::RetryPolicy` |
| Output profile shape            | `crates/quarry-core/src/output_profile.rs` (cycle 25)  |
| Preset bundles                  | `crates/quarry-core/src/presets.rs` (cycle 25)        |

## Operational migration

If you operated Quarry v1 on the same Postgres:

1. **Tables**: v2 uses `quarry_*` prefixes. v1's tables stay
   untouched — drop them only after the v2 cutover is stable.
2. **API**: v1's `:8080` and v2's `:8082` can coexist; route a
   percentage of traffic via your load balancer.
3. **Profiles**: v1's S3 layout `profiles/<id>.json` migrates to
   v2's `profiles/<org_id>/<id>.json` (cycle 24). One-shot CLI
   pending cycle 30.
4. **Webhooks**: v1's webhook payloads remain backwards-compatible;
   v2 ADDS the canonical `JobHistoryEvent` shape under
   `Idempotency-Key` headers.

## What's NOT carried over

- v1's MD5 fingerprint → v2 uses blake3 (security + perf).
- v1's per-request ad-hoc event JSON → v2 uses `JobHistoryEvent`
  exclusively (cluster #7).
- v1's "trust the network" cross-plane auth → v2 enforces HMAC
  (cluster #14).
- v1's hand-rolled retry → v2 uses the typed `RetryPolicy`
  (cluster #2).
