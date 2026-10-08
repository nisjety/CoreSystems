# Qdrant `source` stamp and its backfill runbook

Status: **built, not deployed** (task QB-M1b of the Quarry option B build, written 2026-10-07). Nothing in sections 4 and 5 has been run against the stack yet. The numbers in section 3 are read-only reads of the developer stack on 2026-10-07.

## 1. What this is

Every point in the text collection (`dataplane_knowledge_embedv4`) now carries the payload key `source`: the `documents.source` of the document the chunk was cut from (`sharepoint`, `quarry`, `chat-upload`, ...).

Why: the retrieval engine's dense arm can only filter on what the point itself says. `filters.sources` is already turned into a keyword match on the payload key `source` (`retrieval-engine-rs/src/search/filters.rs`, `RetrievalFilters::to_qdrant_conditions`), and until a point carries the key that match finds nothing. The server-enforced Quarry corpus scope (QB-M1a) needs the key: Quarry may only read documents whose source is `quarry`, and the vector search cannot join Postgres. Quickwit (the sparse arm) is not changed by this task.

Rules, all of which the code and its tests hold to:

- The value is `documents.source` byte for byte (no trimming, no case folding), so the filter and the SQL predicate behind it compare the same string.
- A blank source or an unknown document writes **no key**. A point with no `source` is outside every corpus for a scoped search (it fails closed), and the backfill stamps it once its document can be found.
- The document is found by `(document_id, org_id)` together, with the organisation predicate written out. The services connect as a superuser, where row-level security does not apply, so the predicate is the only tenant check. A document id that exists only under another organisation yields no stamp.
- Free-form payload metadata can no longer replace a key the writer owns (`knowledge_id`, `document_id`, `org_id`, `chunk_index`, `text`, `source`). Before this task a `metadata` entry named `org_id` replaced the point's own tenant.
- The batch pipeline reads the source from the database when it writes the point (before anything is embedded, so a database that cannot answer fails the batch while nothing has been spent), not from the event that queued the unit. The reconciler of CR-T9a builds batches through the same pipeline and gets the stamp without extra work.

Where the stamp is written: `batch::build_points` (text chunks), the video-caption path in `media_consumer.rs` (a caption's text point), and `qdrant-backfill-source` (existing points). The visual (page image), audio, video segment and wiki collections are different collections, written by other consumers, and are not stamped by the engine; the tool can stamp any collection that has `document_id` and `org_id` in its payload (`--collection`), but its `--report` is only meaningful for the text collection.

## 2. The honest limit: a stamp is a copy

`documents.source` can change when the same document key is re-ingested from another source (`documents-api-go`, `UPDATE documents SET source=...`). A chunk that did not change is not re-embedded (index-engine's exact-reuse chunking), so its point is not rewritten and keeps the old stamp. Nothing rewrites a stamp that exists, and the backfill never overwrites one.

What contains it: the retrieval engine keeps its own check against `documents.source` after fusion (QB-M1a, defence in depth), and `qdrant-backfill-source --report` shows a stale stamp as a count that stops agreeing with Postgres. A tool that re-stamps mismatches is deliberately not part of this task: it needs the same bounded-run design and a decision on how often to run it. If `--report` ever shows a mismatch, raise it as a follow-up task.

## 3. State on 2026-10-07 (read-only reads)

| Read | Value |
|---|---|
| Qdrant `dataplane_knowledge_embedv4` points | 207 |
| Points with a `source` key | 0 (`is_empty(source)` counts 207; `source = quarry` counts 0) |
| Payload keys on a point | `chunk_index`, `document_id`, `knowledge_id`, `org_id`, plus `document_date` on 205 of them |
| Distinct documents among the points, distinct organisations | 185, 1 |
| Postgres `documents` by source (live, not deleted) | `sharepoint` 183, `quarry` 2 (one more `chat-upload` document is soft-deleted and has no units) |
| Postgres finished units of live documents by source | `sharepoint` 205, `quarry` 2 (207 in all, equal to the point count) |

After the backfill the expected report is `sharepoint` 205 = 205, `quarry` 2 = 2, 0 points without a source.

## 4. Deploy order

1. CR-T8 is deployed and green (gate `G-T8-GREEN`): this task changes the Qdrant payload that CR-T8 asserted unchanged, and updates that assertion (`the_qdrant_payload_carries_the_documents_source` replaces `the_qdrant_payload_is_unchanged`).
2. Deploy the `embedding-engine` image (stage S4, through the launcher stage copies, never a hand-built `docker compose`). It contains the engine that stamps new points and the tool, at `/usr/local/bin/qdrant-backfill-source`. No new environment variable, no switch: the stamp is always on, and a rollback of the image is the rollback.
3. Run the backfill once (section 5).
4. Only then is the vector leg of the Quarry corpus scope (QB-M1a, gate `G-M1B-BACKFILL`) allowed on. Until the backfill has run, a scoped dense search returns nothing for the 207 existing points.

Effect on callers: a request that sets `filters.sources` was matching nothing on the dense arm, because no point carried the key; after the stamp it filters as written. This was not checked for callers outside this repository's Data Plane tests: a caller that set `filters.sources` and got empty dense results will start getting the matching ones.

## 5. Running the backfill

The tool runs inside the engine's container, which already has `DATABASE_URL` (the superuser, as the service uses), `QDRANT_URL` (gRPC) and `QDRANT_COLLECTION`. It prints one JSON line to stdout; logs go to stderr. No provider is called and nothing is re-embedded.

```bash
E=data-plane-v2-embedding-engine-1

# 1. Before: read-only counts. Exit 3 is expected here (the numbers do not agree yet).
docker exec $E qdrant-backfill-source --report

# 2. Dry run: reads the points and Postgres, writes nothing.
docker exec $E qdrant-backfill-source --dry-run

# 3. The run. Bounded: 10000 points by default, --max-points N to change it.
docker exec $E qdrant-backfill-source

# 4. Idempotence: examines 0 points and writes nothing.
docker exec $E qdrant-backfill-source

# 5. After: read-only counts. Exit 0 when every number agrees.
docker exec $E qdrant-backfill-source --report
```

Pass criteria (state of section 3):

| Step | Pass |
|---|---|
| 1 | `total_points` 207, `unstamped_points` 207, `consistent` false, exit 3 |
| 2 | `would_stamp` 207, `by_source` `{quarry: 2, sharepoint: 205}`, all `skipped` counts 0, `truncated` false |
| 3 | `stamped` 207, the same `by_source`, `truncated` false |
| 4 | `examined` 0, `stamped` 0 |
| 5 | `unstamped_points` 0, `stamped_with_unknown_source` 0, each source `agree` true, `consistent` true, exit 0 |

Independent check from the host, Qdrant REST (read-only; no key is needed on the developer stack), and Postgres:

```bash
Q=http://127.0.0.1:6345/collections/dataplane_knowledge_embedv4/points/count
curl -s -X POST $Q -H 'content-type: application/json' -d '{"exact":true,"filter":{"must":[{"key":"source","match":{"value":"quarry"}}]}}'      # 2
curl -s -X POST $Q -H 'content-type: application/json' -d '{"exact":true,"filter":{"must":[{"key":"source","match":{"value":"sharepoint"}}]}}'  # 205
curl -s -X POST $Q -H 'content-type: application/json' -d '{"exact":true,"filter":{"must":[{"is_empty":{"key":"source"}}]}}'                    # 0
docker exec data-plane-v2-postgres-1 psql -U dataplane -d dataplane -At -F ' | ' -c \
  "select d.source, count(*) from knowledge_units ku join documents d on d.document_id=ku.document_id and d.org_id=ku.org_id where ku.embedding_status='done' and d.deleted_at is null group by d.source order by 1"
```

What the tool does and does not do (so a surprising number can be read):

- It scrolls only the points whose payload has no `source`, so an already stamped point is never examined and never overwritten.
- It writes exactly one key, `source`, to exactly the points it scrolled, by id, with `wait=true`. It cannot change `org_id`, a vector or any other key.
- A point is left alone and counted when it names no document or organisation (`without_identity`), when its document is not in that organisation (`document_not_found`) or when the source is blank (`blank_source`). It never invents a source. After a run, `skipped` should be 0 on this stack; anything else is an orphaned point to investigate (the erasure path should have removed it).
- `--max-points N` is the most points one run examines. A run that stops at the bound prints `truncated: true` and `resume_offset`; pass it back as `--offset` to continue past points already looked at. `--org-id <ORG>` (repeatable) restricts a run to some organisations, which is how a large corpus is stamped one tenant at a time.
- A failed run can simply be repeated: nothing it did needs undoing.

## 6. Rollback

Stamping is additive, and nothing reads the key until a request sets `filters.sources` (QB-M1a). Rolling the engine image back stops stamping new points and leaves the stamps that exist, which is harmless. To remove the key from a collection, use Qdrant's delete-payload on the points that have it (the state before this task: a scoped search then returns nothing for them):

```bash
curl -s -X POST http://127.0.0.1:6345/collections/dataplane_knowledge_embedv4/points/payload/delete \
  -H 'content-type: application/json' \
  -d '{"keys":["source"],"filter":{"must_not":[{"is_empty":{"key":"source"}}]}}'
```

That request has not been exercised: try it on one organisation first (add an `org_id` condition to the filter), and do not run it on a stack where Quarry's corpus scope is on.

## 7. Tests

| Where | What | Run |
|---|---|---|
| `qdrant_writer/mod.rs`, `payload_tests` | a new point carries the source; a blank or unknown source writes no key; metadata cannot replace an owned key or forge the source; the value is verbatim | `cargo test -p embedding-engine-rs --bin embedding-engine payload_tests` |
| `batch/mod.rs`, `source_payload_tests` | each unit is stamped from its own document under its own organisation; an unknown document gives no stamp; the stamp changes nothing else about the point | `cargo test -p embedding-engine-rs --bin embedding-engine source_payload_tests` |
| `source_stamp.rs` | the key is the one the retrieval filters read; blank is no source | `cargo test -p embedding-engine-rs source_stamp` |
| `bin/qdrant_backfill_source.rs` | the run against a fake that only records calls: stamps from the document, writes only `source`, never touches a stamped point, idempotent, dry run writes nothing, bounded and resumable, org filter, no stamp for a document that is not in the point's organisation, no blank source, the report | `cargo test -p embedding-engine-rs --bin qdrant-backfill-source` |
| `batch/tests.rs` (Postgres 16, real migrated schema) | the real pipeline stamps the payload from the `documents` table; a changed source reaches the point at the next embedding; a mixed-organisation batch stamps each unit from its own document; the lookup under the application role in the real org-scope helper and on the superuser connection | `bash tests/scripts/run-postgres-contract-tests.sh embedding-engine-rs` |
| `bin/qdrant_backfill_source.rs` (real Qdrant) | the real adapter: filter, scroll offsets, `set_payload` by id and counts against a scratch collection | `TEST_QDRANT_URL=http://<disposable qdrant gRPC> cargo test -p embedding-engine-rs --bin qdrant-backfill-source -- --ignored` |

The last two rows need infrastructure that is not on a developer host by default (a disposable Postgres 16, a disposable Qdrant). They are written to FAIL, not skip, when run without it.
