# Wiki event schema (canonical)

> **Source of truth** for `dataplane.wiki.*` NATS events. wiki-store-go is
> the producer; embedding-engine-rs is the consumer. Both sides must
> keep their struct in lockstep with this doc. The Rust round-trip
> test (`tests/wiki_event_schema.rs`) catches drift at CI time.

## Subjects

| Subject | Direction | Trigger |
|---|---|---|
| `dataplane.wiki.version.published` | wiki-store-go → embedding-engine-rs | `CreatePage` / `CreateVersion` commit |
| `dataplane.wiki.page.deleted` | wiki-store-go → embedding-engine-rs | (reserved — not emitted yet) |

## Signed envelope

The NATS message is an RS256-signed Data Plane event envelope, not raw domain
JSON. The JWT contract is fixed to issuer/subject `service:wiki-store-go`, key
ID `wiki-events-v1`, audience `dataplane-events`, and scope
`events:wiki:publish`. It binds the subject, tenant, optional verified caller,
`zdr=false`, payload SHA-256, issued/expiry times, and a replay identifier.
Consumers must verify that contract before decoding `data` and must reject
plain JSON, claim/payload conflicts, tampering, replay, and ZDR content.

## Domain payload — `dataplane.wiki.version.published`

```json
{
  "page_id":      "string",
  "version_id":   "string",
  "org_id":       "string",
  "workspace_id": "string",
  "title":        "string",
  "path":         "string",
  "content":      "string",
  "user_id":     "string (optional for service jobs)",
  "zdr":          false
}
```

### Field semantics

| Field | Required | Notes |
|---|---|---|
| `page_id` | yes | Stable identifier; survives version churn |
| `version_id` | yes | Used as the Qdrant point ID in `wiki_block_embeddings` — re-publishing the same version overwrites the embedding |
| `org_id` | yes | Tenancy scope. Embeddings carry this in the Qdrant payload for filter-at-search-time |
| `workspace_id` | yes | Sub-org scope; carried in payload |
| `title` | yes | Display label; carried in payload for retrieval result rendering |
| `path` | yes | URL-routable path; carried in payload |
| `content` | yes | Full text to embed. May be Markdown or sanitized HTML — the embedder does not distinguish |
| `user_id` | no | Verified caller identity when the mutation originated from a user |
| `zdr` | yes | Always `false`; wiki publication is durable and therefore incompatible with ZDR |

### Unknown fields

Consumers may accept additive unknown domain fields for forward compatibility,
but the signature covers the exact encoded payload and the security fields must
remain claim-bound.

### Versioning

This is schema **v1**. Breaking changes (renaming a required field,
making a required field optional) require a new subject:
`dataplane.wiki.version.published.v2`. Additive changes (new optional
field) stay on `v1`.

## Cross-language test

[`services/embedding-engine-rs/tests/wiki_event_schema.rs`](../../services/embedding-engine-rs/tests/wiki_event_schema.rs)
freezes a canonical JSON literal against the consumer struct. CI runs
it on every Rust workspace test. Go-side counterpart in
[`services/wiki-store-go/internal/events/publisher_schema_test.go`](../../services/wiki-store-go/internal/events/publisher_schema_test.go)
freezes the same literal against the producer struct.
