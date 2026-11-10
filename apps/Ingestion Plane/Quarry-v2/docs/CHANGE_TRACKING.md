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

### `GET /v1/change/diff` (INNH-4, behind `QUARRY_EDGE__CHANGE_TEXT_DIFF`)

The two versions of a tracked page and the paragraph diff between them: `?url=` (latest two), `?url=&from=&to=` (two named baselines), `?diff_id=` (the diff a `quarry.change.detected` event announced), `&include=markdown` (the stored text of both). The contract, caps, the unavailable reasons and the ZDR rule are in `docs/CONTRACTS.md`, "Change text diff". Off by default (501); only change monitors keep text, and nothing is kept under ZDR.

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

## Text-fingerprint switch (E3.5-4)

`QUARRY_EDGE__CHANGE_TEXT_FP` (edge, default off) compares the normalised text fingerprint (`text_fp` of `run_page`) instead of the raw-byte hash, so a rotating nonce or a reflowed attribute is not a change. The baseline records its scheme with a `text:` mark. A scheme flip is detected and re-baselined silently: the first check after the flip saves a new baseline, diffs nothing, sends no change event and no webhook, and answers `status: unchanged` with `rebaselined: true`. The raw-byte hash comes back as `raw_fingerprint` when the text one was compared. The internal record call carries the text fingerprint as `fresh_text_fingerprint`. The rollout order (orchestrator first, never with mixed orchestrator versions) is in `RUNBOOKS.md`, "Change text fingerprint".

## Tenant isolation

Every wire shape carries `org_id`. The persistence layer (cycle 27+)
keys on `(org_id, baseline_id)` so cross-tenant queries are
statically impossible.

## Webhook emission

`status=Changed` events emit a webhook with subject
`quarry.change.detected` (mirrors the cluster #7 subject taxonomy).
Payload mirrors the HTTP response from `/v1/change/check`.

The edge posts it to control (`POST /v1/webhooks/change?org_id=<org>`, internal HMAC), where it
becomes a `change_detected` event and is delivered to the **webhook subscriptions of that
organization** (INNH-5): an organization registers an HTTPS endpoint with
`POST /v1/webhook-subscriptions` (dark behind `QUARRY_EDGE__WEBHOOK_SUBSCRIPTIONS` and
`QUARRY_CONTROL__WEBHOOK_SUBSCRIPTIONS`). What the endpoint receives, how it is signed and how
failures are retried is `docs/CONTRACTS.md` section 4; the API is "Webhook subscriptions" there.
Under Zero Data Retention nothing is emitted, and a ZDR organization cannot create a subscription.

**The notifyOnChange filter (E3.5-7).** A subscription may carry a `notify_on_change` filter,
`{"types": [...], "min_paragraph_delta": N}`. It is set on create and changed on PATCH: an absent key
keeps the filter and `null` removes it. The subscription returns it. A subscription without one takes
every change, as before. `types` is a non-empty list from the closed set `new`, `changed` and
`removed`. Every change webhook carries `change_type`, with no switch, so a `changed` filter works as
soon as the subscription routes are on. Nothing sends `new` or `removed` yet: the edge sends no webhook
for a first capture, and a page that is gone is not told apart from a failed fetch. A filter that names
only those two matches nothing until a producer exists. `min_paragraph_delta` is 0 (no threshold) to
1000: the fewest paragraphs, added plus removed, that a `changed` page must have. Moves do not count,
and a markup-only change counts 0. The delta is measured from the stored text under
`QUARRY_EDGE__CHANGE_WEBHOOK_DELTA`. With that switch off the delta is unknown, and an unknown delta is
never dropped, so the threshold passes every change. The filter applies to `change_detected` events
only. It holds on the change webhook path only: the run-event batch path still fans out unfiltered
until E3.5-7b lands, so a filtered subscription can still receive a change by that path. A refused
filter is a 400 that names the field `notify_on_change` and a reason: `notify_types_empty`,
`notify_type_unsupported` or `notify_min_delta_out_of_range`.

## Tests

`crates/quarry-core/src/change_history.rs`:
- `change_status_json_is_snake_case`
- `baseline_omits_optional_fields_when_none`
- `change_record_roundtrips_through_json`

## Wiring status (updated)

Wired on `agent/quarry-parity-rust`:

- **Persistence layer** — `PostgresBaselineStore` in
  `quarry-runtime/src/postgres_baseline_store.rs` implements
  save/load/compare/diff-record plus snapshot promotion and refresh
  scheduling, with migrations.
- **Edge routes** — `/v1/change/check`, `/v1/change/latest`, and
  `/v1/change/history` live in `quarry-edge/src/change_routes.rs`.
- **Webhook emission** — signed change webhooks are emitted from the
  edge (`quarry-edge/src/change_webhook.rs`): `POST
  /v1/webhooks/change?org_id=<org>` with `org_id` required in both the
  body and query param, matching the Go receiver in quarry-control's
  `internal/resources/change_webhook.go`.

Still pending:

- **Diff computation** — dedicated markdown / HTML / JSON-patch
generators invoked by `compare_snapshot` remain future work; diffs are
currently stored as raw artifact bytes.

## Update 2026-08-28 (F4 follow-up)

The `/v1/team/*` wire shape is **deliberately a bare JSON object** for
single-object endpoints and `Page<T>` for list endpoints, NOT the
`quarrycontracts.RESTEnvelope { data, meta, error }` shape. The
`cycle24.go:16-25` doc comment in `services/quarry-control/internal/resources/`
records the contract. The Rust edge's `forward_one::<T>` and
`forward_list::<T>` deserializers are typed; switching to envelopes
would be a breaking change to every cycle-24 cross-language test.
The full contract, the test files that pin it, and the rationale live
at [`CROSS_LANGUAGE_WIRE_CONTRACT.md`](CROSS_LANGUAGE_WIRE_CONTRACT.md).
