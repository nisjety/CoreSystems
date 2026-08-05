# autocomplete-core

Tenant-scoped typeahead for Verevon and Quarry search flows.

`autocomplete-core` is the input-assistance layer in front of Quarry. It
does not execute search and does not generate answers. It consumes Quarry
events, indexes short suggestion terms into Sonic, stores suggestion metadata
in SQLite, and serves hydrated suggestions to trusted backend callers.

## Current Status

Implemented:

- `GET /health`
- `GET /ready`
- `GET /v1/suggestions`
- `POST /v1/internal/push`
- Sonic ingest/search adapter
- SQLite metadata hydration store
- Durable NATS JetStream consumer for `quarry.events.*`
- Event handlers for `search_issued` and `host_discovered`

Still pending:

- Live Verevon suggestion E2E after the Docker recovery/redeploy
- Title ingestion once Quarry emits title-bearing metadata events
- Optional correction endpoint after enough query corpus exists

## Architecture

```
Verevon server proxy
        │ GET /v1/suggestions
        ▼
autocomplete-core
        ├─ Sonic: fast object lookup
        └─ SQLite: object ID -> display suggestion metadata
        ▲
        │ JetStream durable consumer
        │
Quarry NATS stream QUARRY_EVENTS
        ├─ quarry.events.search_issued
        └─ quarry.events.host_discovered
```

Sonic `QUERY` returns object IDs and `SUGGEST` returns words. Full UI
suggestions need display text, source, URL, and metadata, so SQLite is the
authoritative hydration store while Sonic stays the low-latency index.

## Configuration

| Variable | Default | Purpose |
|---|---:|---|
| `AUTOCOMPLETE_HTTP_ADDR` | `0.0.0.0:3219` | HTTP listen address |
| `AUTOCOMPLETE_METADATA_DB` | `./data/autocomplete-core.sqlite3` | SQLite metadata path |
| `AUTOCOMPLETE_INTERNAL_TOKEN` | required | Bearer token for non-health routes; unset only behind both isolated-E2E gates |
| `SONIC_ENABLED` | inferred from password | Enables Sonic adapter |
| `SONIC_ADDR` | `127.0.0.1:1491` | Sonic TCP channel address |
| `SONIC_PASSWORD` | unset | Sonic channel password |
| `NATS_ENABLED` | inferred from explicit URL | Enables Quarry event consumer |
| `NATS_URL` | `nats://127.0.0.1:4222` | NATS broker URL |
| `NATS_STREAM` | `QUARRY_EVENTS` | JetStream stream |
| `NATS_DURABLE` | `autocomplete-core` | Durable consumer name |
| `NATS_SUBJECT_FILTER` | `quarry.events.*` | Event subject filter |

## API

Trusted callers should pass the tenant with `x-org-id`. Direct browser calls
should go through a Verevon/backend proxy so auth and rate limits stay outside
this internal service.

```http
GET /v1/suggestions?q=Find%20me&scope=queries&limit=8
x-org-id: org_123
authorization: Bearer <AUTOCOMPLETE_INTERNAL_TOKEN>
```

Response:

```json
{
  "data": {
    "query": "Find me",
    "scope": "queries",
    "suggestions": [
      {
        "text": "Find me restaurants",
        "source": "query",
        "collection": "queries",
        "object": "query:..."
      }
    ],
    "sonic_enabled": true
  }
}
```

Manual/internal ingest:

```http
POST /v1/internal/push
x-org-id: org_123
authorization: Bearer <AUTOCOMPLETE_INTERNAL_TOKEN>
content-type: application/json

{"text":"Find me Grocery store","collection":"queries","source":"query"}
```

## Local Verification

```bash
cargo fmt
cargo test
cargo clippy --all-targets -- -D warnings
```

The canonical Ingestion Plane Compose includes `autocomplete-sonic` and
`autocomplete-core` on the private plane/inter-plane networks. Set
`SONIC_PASSWORD`, `AUTOCOMPLETE_INTERNAL_TOKEN`, and `INGESTION_NATS_TOKEN`
before starting them. The repository-root Compose is legacy and no longer owns
this capability.
