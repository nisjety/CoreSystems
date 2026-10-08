# index-parity-v1: the keyword-index parity report

Authored by CR-T11 (data-quality-go, the consumer). The producer is CR-T9a
(quickwit-adapter-rs, `GET /admin/index-parity`), which is built after this file exists
and implements exactly what it says.

data-quality-go has no access to Quickwit. The adapter owns the index, so the adapter
measures the index and data-quality-go judges the measurement in the release gate
`sparse_index_in_sync` (internal/gates/parity.go).

## The endpoint (CR-T9a implements it)

    GET /admin/index-parity

* Read-only. No query parameters: the organization is the `org_id` of the verified bearer,
  never a caller-supplied value.
* Auth: `Authorization: Bearer <data-plane token>` verified like the adapter's other admin
  routes (RS256, audience and issuer pinned). Required scope: `data:quality:admin`.
  data-quality-go forwards its own caller's bearer, which carries that scope; no principal
  needs a new grant.
* 200 with the JSON of `response.*.golden.json`. 401/403 for a bad token or scope.
  Any other status, a timeout or a body that is not this contract makes the gate report
  `unavailable`, which fails the gate report. It is never a pass.

## Fields (all integers are counts; none is negative)

| field | meaning |
|---|---|
| `contract` | the literal `index-parity-v1`. A different value makes the gate `unavailable`. |
| `org_id` | the organization measured. Must equal the caller's, or the gate fails. |
| `checked_at` | RFC 3339 UTC, informational. |
| `postgres.eligible_units` | units with `embedding_status = 'done'` whose document is not deleted and has `zdr_classification` internal, public or sensitive (`lower(btrim(...))`): exactly the population the adapter indexes (rebuild.rs). |
| `postgres.eligible_with_context` | of those, units whose stored `chunk_context` is not blank (`btrim(...) <> ''`). |
| `quickwit.documents` | `knowledge_unit` documents of the organization in the index as the index counts them. A delete is eventual, so this can exceed `distinct_units` until a merge. |
| `quickwit.distinct_units` | the same, de-duplicated by `knowledge_id` (this Quickwit refuses the `cardinality` aggregation). Read after merges. |
| `quickwit.with_context_body` | distinct indexed units whose `context_body` is not blank. |
| `difference.missing_units` | eligible in Postgres, absent from the index (set difference over knowledge ids). |
| `difference.unexpected_units` | in the index, not eligible in Postgres (stale). |
| `difference.context_body_missing` | indexed units whose Postgres context is not blank and whose `context_body` is blank or absent. |
| `difference.context_body_unexpected` | indexed units with a `context_body` while their Postgres context is blank. |

## What the gate does with it

It also counts the Postgres side itself, with the same predicate, and requires that:

1. the report is for the caller's organization and contract `index-parity-v1`;
2. it adds up: `eligible_units - missing_units + unexpected_units == distinct_units`;
3. the adapter's `eligible_units` and `eligible_with_context` equal the quality service's own
   counts (a drifted predicate would make parity meaningless);
4. all four `difference` counts are zero.

`response.in-sync.golden.json` passes. `response.drift.golden.json` is the live state of
2026-10-06 (52 of 207 units indexed) and fails.

## Pinning

`SHA256SUMS` pins the files of this directory. The adapter vendors this directory byte for
byte under `services/quickwit-adapter-rs/tests/fixtures/contracts/index-parity-v1/` (CR-T9a),
with the same `SHA256SUMS`, and its test must produce a report of the golden's shape for a
seeded fixture. `scripts/check-parity-contract.sh` is the repo-level drift check: it verifies
the pins and, once the adapter serves the route, that the vendored copy is byte-identical.
