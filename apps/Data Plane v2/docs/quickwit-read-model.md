# Quickwit Sparse Search Read Model

Quickwit is a derived read model for corpus-scale sparse search. Postgres stays
canonical for documents, knowledge units, wiki content, source objects, and
retrieval/source logs.

## Runtime Shape

- `minio` stores Quickwit splits under the `quickwit` bucket.
- `quickwit` runs the local search/index node using `infra/quickwit/quickwit.yaml`.
- `quickwit-adapter-rs` creates `dataplane-corpus`, rebuilds it from Postgres,
  and applies best-effort live updates from core NATS subjects.
- `retrieval-engine-rs` selects sparse search through `SPARSE_SEARCH_BACKEND`.
  Use `postgres` for local/dev fallback and `quickwit` for the Quickwit-backed
  path. The Quickwit path falls back to Postgres if Quickwit fails.

The active retrieval sparse backend is exposed by:

```bash
curl -s http://localhost:8014/readyz
```

With Quickwit enabled, `sparse_backend` should be
`quickwit-with-postgres-fallback`.

## Rebuild Contract

Quickwit can be deleted and rebuilt from Postgres:

```bash
curl -XPOST http://localhost:9204/admin/rebuild \
  -H 'content-type: application/json' \
  -d '{"clear":true}'
```

Equivalent helper:

```bash
./scripts/rebuild-quickwit.sh
./scripts/rebuild-quickwit.sh --org org_123
```

For an org-scoped rebuild:

```bash
curl -XPOST http://localhost:9204/admin/rebuild \
  -H 'content-type: application/json' \
  -d '{"org_id":"org_123","clear":true}'
```

## Indexed Entities

- `knowledge_unit`: searchable retrieval chunks; these are the only entity type
  used by the retrieval sparse backend.
- `wiki_version`: published wiki content for corpus search and RAG browsing.
- `source_object`: canonical connector inventory with SharePoint/OneDrive hashes,
  ACL tags, paths, and metadata.
- `retrieval_log`: retrieval traces for observability/search.
- `wiki_source_log`: wiki provenance/source logs.

## Live Events

The adapter subscribes to core NATS subjects:

- `dataplane.knowledge.units.created`
- `dataplane.documents.indexed`
- `dataplane.documents.deleted`
- `dataplane.wiki.version.published`
- `dataplane.source_objects.changed`
- `dataplane.source_objects.deleted`
- `dataplane.search.rebuild.requested`

Core NATS updates are only acceleration. Durability comes from Postgres plus the
admin rebuild path.

## Delete Visibility

Quickwit delete tasks are asynchronous and are applied during Quickwit merge
work. Data Plane therefore treats Quickwit as a derived read model only:

- Retrieval filters final candidates through canonical Postgres live documents
  before returning sources.
- Source-object duplicate/search management should use the documents API
  canonical Postgres endpoints when deterministic delete visibility matters.
- Dev/test cleanup can call `./scripts/rebuild-quickwit.sh` to clear and replay
  from Postgres.

## Smoke Test

With the compose stack running and `SPARSE_SEARCH_BACKEND=quickwit`:

```bash
./scripts/smoke-quickwit-retrieval.sh
```

This runs the sparse fallback unit proof, verifies `retrieval-engine` selected
the Quickwit wrapper, pushes a smoke `source_object` through `documents-api`,
waits for Quickwit to return it, and rebuild-cleans the smoke row.

## Source-Object Duplicates

The canonical duplicate-management surface is Postgres-backed:

```bash
curl "http://localhost:8010/v1/source-objects/duplicates?min_count=2&min_size=1048576&source=sharepoint" \
  -H "X-Org-ID: org_123" \
  -H "X-Internal-Api-Key: $INTERNAL_API_KEY"
```

Groups are keyed by `content_hash`, falling back to `sha1_hash` and then
`quickxor_hash`. Deleted `source_objects` are excluded.

## Finspo Backfill

Live SharePoint deltas mirror into Data Plane source objects. Existing finspo
inventory can be replayed with:

```bash
cd "../Ingestion Plane/finspo-core"
FINSPO_DSN=postgres://... \
DATA_PLANE_DOCUMENTS_BASE_URL=http://localhost:8010 \
DATA_PLANE_INTERNAL_API_KEY="$INTERNAL_API_KEY" \
go run ./cmd/backfill-source-objects
```

Set `FINSPO_BACKFILL_ORG_ID` to replay one organization and
`FINSPO_BACKFILL_BATCH_SIZE` to tune batch size.
