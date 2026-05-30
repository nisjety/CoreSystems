# Versioned Change History

Cycle 26 / cluster #9.

## Why

Tracked sources (policy pages, pricing pages, doc-site indexes) need
versioned snapshots so consumers can answer "what changed since last
week" without re-fetching. Pre-cycle 26 the runtime captured pages
but didn't preserve a baseline chain — every fresh fetch overwrote
the previous one.

## API

```
POST /v1/change/check       { url }
GET  /v1/change/latest?url=<...>
GET  /v1/change/history?url=<...>&limit=<n>
```

### `POST /v1/change/check`

Performs a fresh fetch, compares to the last baseline, returns:

```jsonc
{
  "source_url": "https://example.com/pricing",
  "org_id": "org_a",
  "status": "changed",           // new | unchanged | changed | unreachable
  "new_baseline": { ... },        // BaselineSnapshot, present when reachable
  "prev_baseline": { ... },        // present unless status=new
  "diff_id": "diff_01H...",        // present only when status=changed
  "checked_at": "2026-05-19T..."
}
```

### `GET /v1/change/latest`

Returns the most-recent `BaselineSnapshot` for the URL, or 404.

### `GET /v1/change/history`

Paginated list of baselines, newest-first, walking the `prev_baseline_id`
chain.

## Data model

```rust
pub struct BaselineSnapshot {
    pub baseline_id: String,                // "bln_<ulid>"
    pub org_id: String,
    pub source_url: String,
    pub fingerprint: String,                 // blake3 of canonical content
    pub artifact_id: Option<ArtifactKind>,
    pub prev_baseline_id: Option<String>,    // chain pointer
    pub captured_at: DateTime<Utc>,
    pub run_id: Option<RunKind>,
}

pub struct DiffRecord {
    pub diff_id: String,
    pub org_id: String,
    pub from_baseline_id: String,
    pub to_baseline_id: String,
    pub source_url: String,
    pub format: String,         // "text" | "markdown" | "html" | "json-patch"
    pub artifact_id: ArtifactKind,
    pub summary: Option<String>, // ≤ 1KB human-readable
    pub created_at: DateTime<Utc>,
}
```

## Identity model

- `baseline_id`: `bln_<ulid>`. Cross-org globally unique.
- Natural key for "this URL's latest baseline":
  `(org_id, source_url)` — find via `ORDER BY captured_at DESC LIMIT 1`.
- Baseline chain: `prev_baseline_id` forms a singly-linked list so
  consumers can walk history without a separate LIST query.

## Change classification

- `New` — first time we've seen this URL.
- `Unchanged` — fresh fingerprint matches `prev_baseline.fingerprint`.
- `Changed` — fingerprints differ; `diff_id` points to a computed
  diff in the artifact store.
- `Unreachable` — fetch failed (4xx/5xx/timeout). Baseline chain not
  advanced; the previous baseline remains "latest".

## Tenant isolation

Every wire shape carries `org_id`. The persistence layer (cycle 27+)
keys on `(org_id, baseline_id)` so cross-tenant queries are
statically impossible.

## Webhook emission

`status=Changed` events emit a webhook with subject
`quarry.change.detected` (mirrors the cluster #7 subject taxonomy).
Payload mirrors the HTTP response from `/v1/change/check`.

## Tests

`crates/quarry-core/src/change_history.rs`:
- `change_status_json_is_snake_case`
- `baseline_omits_optional_fields_when_none`
- `change_record_roundtrips_through_json`

## Pending wiring

- **Persistence layer** — `PostgresBaselineStore` with
  `SaveBaseline / LoadBaseline / CompareSnapshot / CreateDiffRecord /
  ScheduleRefreshRun / PromoteTrackedResultToSnapshot` methods. Cycle
  27.
- **Edge routes** — `/v1/change/check`, `/v1/change/latest`,
  `/v1/change/history`. Cycle 27.
- **Webhook emission** wiring through the existing webhook delivery
  pipeline in Go control. Cycle 27.
- **Diff computation** — markdown / HTML / JSON-patch generators
  invoked by `compare_snapshot`. Cycle 28.
