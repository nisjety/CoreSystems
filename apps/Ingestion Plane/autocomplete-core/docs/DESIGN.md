# autocomplete-core Design

## Role

`autocomplete-core` makes text input feel instant. It is deliberately separate
from Quarry search and answer generation:

- Quarry `/v1/search` returns real search results.
- Quarry `/v1/answer` summarizes sources.
- `autocomplete-core` returns low-latency suggestions while the user types.

## Data Model

Sonic uses `collection / bucket / object`.

| Collection | Bucket | Object | Display text |
|---|---|---|---|
| `queries` | hash of `org_id` | stable query hash | normalized user query |
| `hosts` | hash of `org_id` | normalized host | host/domain |
| `titles` | hash of `org_id` | future artifact id | page title |

Buckets are hashes of org IDs instead of raw IDs. That keeps Sonic tenant
isolation strict without putting direct tenant identifiers into the index.

SQLite stores the metadata Sonic cannot return:

- collection
- bucket
- object
- display text
- source
- target URL
- metadata JSON
- hit count
- last seen time

## Event Ingestion

The service consumes the existing Quarry JetStream stream through a durable
consumer:

| Subject | Action |
|---|---|
| `quarry.events.search_issued` | index normalized query into `queries` |
| `quarry.events.host_discovered` | index normalized host into `hosts` |

The consumer acknowledges malformed or unsupported events so a bad event does
not poison the durable cursor. Valid events are idempotent because object IDs
are deterministic.

## HTTP Contract

The public contract is resource-shaped and envelope-based:

- `GET /v1/suggestions`
- `POST /v1/internal/push`

`/v1/suggestions` supports:

| Query | Purpose |
|---|---|
| `q` | required typed input |
| `scope` | `all`, `queries`, `hosts`, or `titles` |
| `limit` | 1-20 |

Tenant context comes from `x-org-id`; browser access should be proxied through
Verevon/backend.

## Reliability

Sonic is a fast derived index. SQLite is the local hydration source. If Sonic
is unavailable, suggestions degrade to a bounded SQLite prefix scan. If the
service restarts, the durable NATS consumer resumes from its acknowledged
position.

## Security

- No direct browser access is required.
- `AUTOCOMPLETE_INTERNAL_TOKEN` is required and gates non-health routes. An
  unset token is accepted only with both `ALLOW_INSECURE_DEV_DEFAULTS=1` and
  `ISOLATED_E2E=1`.
- Tenant isolation uses explicit org buckets.
- The Docker Compose service requires `SONIC_PASSWORD` and
  `AUTOCOMPLETE_INTERNAL_TOKEN`.

## Current Gaps

Title suggestions require Quarry to emit a title-bearing event after metadata
extraction. The current `page_fetched` event is emitted before title metadata
exists, so title indexing is intentionally left inactive for now.

Correction is also deferred. Sonic word suggestions are not a full spell
corrector; adding `/v1/corrections` should happen after enough query history
exists to validate behavior.
