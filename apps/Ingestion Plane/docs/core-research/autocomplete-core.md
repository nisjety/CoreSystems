# autocomplete-core Research Dive

Generated: 2026-07-11 (supersedes 2026-06-07)

Scope: `apps/Ingestion Plane/autocomplete-core`
> **2026-07-12 source update:** an unset internal token now fails startup outside double-gated isolated E2E, bearer comparison is constant-time, and `/ready` probes Sonic when enabled. Seven Rust tests pass. The running capability remains absent until secrets/Sonic/NATS are provisioned and a v3 UI E2E proves suggestions or typed unavailable behavior.

Evidence grades used below: **[live-curl]** = verified against a running port,
**[state]** = `docker ps`/`docker inspect`/filesystem state, **[source-only]** =
read from source/config/compose, **[host-build]** = compiled/tested on the host.
Docker is in a degraded mode this session (containerd content store corrupted:
`docker exec` and image rebuilds fail), so live behavior was checked via host
`curl` only and all DB/env/schema facts were read from disk.

## Bottom line

`autocomplete-core` is a **real, well-built, fully-tested** Rust/Axum typeahead
sidecar — no stubs, no mocks, no placeholders. But it is **completely DOWN**:
neither it nor its Sonic backend is running, and it cannot start with the current
configuration. It is **irrelevant to the shipping/Visma Model Plane headline
goals** (it is a frontend searchbar typeahead helper, not an agent tool).

Three things changed materially since the 2026-06-07 note, which called it
"straightforward and peripheral":
1. Its deployment lives in the **root monorepo `docker-compose.yml`**, not the
   Ingestion Plane compose. The plane compose has no `autocomplete` service.
2. It is **not running** (root cause: required env vars unset — see below).
3. Even if started, its **NATS consumer is mis-wired across networks** and would
   never receive Quarry events.

## What it is

- **Language/framework**: Rust 2021 (rust-version 1.85), Axum 0.7, Tokio.
  ~1,687 LoC across `src/{main,lib,app,config,routes,store,sonic,ingest,events,normalization,error}.rs`.
- **Role**: tenant-scoped search-as-you-type suggestions for the Verevon search
  box. It does **not** run search or generate answers — it is input-assistance
  in front of Quarry.
- **HTTP surface** (`src/routes.rs`): `GET /health`, `GET /ready`,
  `GET /v1/suggestions`, `POST /v1/internal/push`. Envelope-shaped
  (`{ "data": ... }`) responses; typed error envelope `{ error: { code, message } }`.
- **Two-tier index** (real):
  - **Sonic** (`valeriansaliou/sonic:v1.4.9`) is the low-latency term index
    (`collection / bucket / object`), via the `sonic-channel` crate (`src/sonic.rs`).
  - **SQLite** (`rusqlite`, bundled, WAL) is the authoritative hydration store
    that maps Sonic object IDs back to display text/source/target_url/metadata,
    plus `hit_count`/`last_seen_at` for ranking (`src/store.rs`).
  - **Graceful degradation**: if Sonic is disabled or errors, suggestions fall
    back to a bounded SQLite prefix `LIKE` scan (`src/app.rs::suggest_collection`).
- **Data source** (`src/ingest.rs`, `src/events.rs`): a durable NATS JetStream
  pull-consumer on stream `QUARRY_EVENTS`, subject filter `quarry.events.*`,
  handling `search_issued` → `queries` collection and `host_discovered` →
  `hosts` collection. Malformed/unsupported events are ack'd to avoid a poison
  loop. There is also a `POST /v1/internal/push` for manual/backfill ingest.
- **Tenant isolation** (`src/normalization.rs`): the Sonic bucket is
  `org_<blake3(org_id)[..16]>` — the raw org id never enters the index. Object
  IDs are deterministic `blake3` hashes, so re-ingest is idempotent. Good
  privacy posture.

## Deployment reality

- **Declared in the ROOT compose** `/docker-compose.yml`, not the plane compose:
  - `autocomplete-core` — build `./apps/Ingestion Plane/autocomplete-core`,
    `container_name: autocomplete-core`, **`ports: "3219:3219"`**, network
    `coresystem-local` (external → `verevon-net`), `depends_on: [quarry-nats, quarry-sonic]`,
    healthcheck `wget http://localhost:3219/health`. [source-only]
  - `quarry-sonic` — the Sonic index, same network, config from
    `autocomplete-core/deploy/sonic.template.cfg` with `SONIC_PASSWORD` injected. [source-only]
- **Port**: 3219 (env `AUTOCOMPLETE_HTTP_ADDR`, default `0.0.0.0:3219`). Sonic on 1491.
- The Ingestion Plane `docker-compose.yml` contains **no** autocomplete/sonic
  service (grep clean). [source-only]

### It is DOWN — verified

- **[live-curl]** `curl http://127.0.0.1:3219/health` → connection refused
  (port not listening). `GET /v1/suggestions?q=osl` → connection refused.
- **[state]** Of 92 running containers, **neither `autocomplete-core` nor
  `quarry-sonic` exists** (not running, not even stopped). `quarry-edge`,
  `quarry-control`, `quarry-orchestrator` are up; the typeahead pair is not.
- The verevonv3 gateway consequently returns an **empty** suggestions list for
  every searchbar keystroke today (it degrades silently — see below).

### Root cause it can't start (CRITICAL)

**[source-only + state]** The root compose gates both services on
required-variable interpolation:
- `AUTOCOMPLETE_INTERNAL_TOKEN=${AUTOCOMPLETE_INTERNAL_TOKEN:?... is required}`
- `SONIC_PASSWORD=${SONIC_PASSWORD:?... is required}` (on both `autocomplete-core`
  and `quarry-sonic`).

The root `.env` (exists, 2121 bytes) defines **none** of
`AUTOCOMPLETE_INTERNAL_TOKEN`, `SONIC_PASSWORD`, `SONIC_ENABLED`,
`AUTOCOMPLETE_NATS_ENABLED`, or `QUARRY_EDGE_NATS_URL` (grep exit 1). With `:?`
set and the vars absent, `docker compose up` **errors out before creating these
two services** — which is exactly why they never come up. Fix: add
`SONIC_PASSWORD` and `AUTOCOMPLETE_INTERNAL_TOKEN` to the root `.env` (README §Local
Verification already calls this out).

### NATS wiring mismatch (would still fail after env fix)

**[state + source-only]** Even with the env set, the event pipeline is broken:
- `autocomplete-core` is on network **`verevon-net`** and its `NATS_URL`
  defaults to `${QUARRY_EDGE_NATS_URL:-nats://quarry-nats:4222}`. There is **no
  `quarry-nats` container** (running NATS: `ingestion-nats`, `verevon-nats`,
  `controlplane-nats`, `app-nats`, `model-plane-nats-1`, `data-plane-v2-nats-1`).
- The **publisher** `quarry-edge` runs on networks **`ingestion-net` +
  `inter-plane-bus`** and connects to alias `nats` (`nats://nats:4222`) inside
  its own compose — i.e. `ingestion-nats`. `autocomplete-core` is on **neither**
  of those networks.
- So the consumer would target a non-existent host and, regardless, is not on
  the same network/JetStream as the publisher. To work, `QUARRY_EDGE_NATS_URL`
  must point at the NATS that actually holds `QUARRY_EVENTS`, and
  `autocomplete-core` must share that network (e.g. join `inter-plane-bus`).
- The **Sonic** wiring, by contrast, is internally consistent:
  `SONIC_ADDR=quarry-sonic:1491`, both declared on `verevon-net`.

## Is the upstream event contract real? Yes.

**[source-only]** Quarry genuinely emits the events this service consumes.
`Quarry-v2/crates/quarry-runtime/src/nats_event_bus.rs` maps
`EventType::SearchIssued → "search_issued"` and
`EventType::HostDiscovered → "host_discovered"`, publishes to stream
`QUARRY_EVENTS` and an aggregate subject `quarry.events.<event_type>`;
`quarry-edge/src/routes.rs` emits `host_discovered` per discovered host. The
payload field names (`org_id`, `user_id`, `query`, `provider`, `result_count` /
`host`, `seed_url`) line up with `events.rs`. The `titles` collection is
**intentionally inactive** (Quarry's `page_fetched` fires before title metadata
exists) — a documented future gap, not a defect.

## Gateway consumer (verevonv3)

**[source-only]** `apps/Frontend Plane/verevonv3/apps/gateway/src/domains/search.rs::search_suggestions`
implements `GET /api/v1/search/suggestions` → `{AUTOCOMPLETE_CORE_URL}/v1/suggestions`
with `bearer_auth(AUTOCOMPLETE_INTERNAL_TOKEN)` and `x-org-id` derived from the
**validated session** (never a client header), 1.5s timeout. It **always
degrades to `{ "suggestions": [] }`** (never an error) when `q` < 2 chars, the
token/URL is empty, or the upstream is non-2xx. `AUTOCOMPLETE_CORE_URL` defaults
to `http://autocomplete-core:3219`. Net effect today: the searchbar dropdown is
silently empty, which is resilient but masks the outage. Note this also means
the gateway must be on `verevon-net` to resolve `autocomplete-core`, and needs
`AUTOCOMPLETE_INTERNAL_TOKEN` in its own env or it short-circuits to empty.
(README still lists "Verevon server-side proxy route" as pending — that is now
implemented; the README is stale on that one line.)

## Security / auth notes

- `authorize()` (`routes.rs`) enforces the bearer token **only when
  `AUTOCOMPLETE_INTERNAL_TOKEN` is set**; if unset it returns `Ok(())` (open).
  In the root compose the token is a required var so this is closed there, but
  any deploy that omits the token leaves `/v1/suggestions` and
  `/v1/internal/push` unauthenticated. Low severity (internal-only service,
  hashed buckets), but worth a fail-closed default.
- `x-org-id` is trusted as provided; cross-tenant reads would require both the
  internal token and knowledge of another org's raw id. Acceptable for an
  internal service, and the gateway correctly sources org from the session.

## Build & test

**[host-build]** `cargo 1.94.1`, `cargo test` on host: compiles clean
(async-nats, rusqlite bundled, sonic-channel, axum) and **7/7 unit tests pass**:
`normalization` (whitespace collapse, host/URL normalization, stable non-raw
org buckets), `events::parses_search_issued_payload`,
`store::stores_and_searches_suggestions`,
`ingest::indexes_search_issued_into_metadata`,
`routes::push_then_suggest_returns_hydrated_suggestion`. No integration test
requires Sonic/NATS (they are feature-gated behind `enabled`). Coverage is unit
only — no live Sonic/NATS end-to-end test exists.

## Stubs / mocks / placeholders

**None.** Grep for `todo|fixme|mock|stub|fake|placeholder|unimplemented|dummy|hardcod`
across `src/` returns a single hit: a **descriptive comment** in `config.rs`
("Previously hard-coded to false…") documenting a real bug fix (NATS consumer now
enabled by default when `NATS_URL` is set — commit `8712fc0d`). The "pending"
items in README/DESIGN (titles, corrections) are honestly-scoped future work.

## Git / WIP state

**[state]** `autocomplete-core/` and the root `docker-compose.yml` are both
**clean** (fully committed) — no uncommitted WIP in this service. The NATS-default
fix is committed (`8712fc0d`). This service was untouched by the large
uncommitted shipping-core / integration-corev2 changes described in the plane
baseline.

## Recommendations (priority order)

1. **Populate root `.env`** with `SONIC_PASSWORD` and
   `AUTOCOMPLETE_INTERNAL_TOKEN` so the pair can start at all. (Blocking.)
2. **Fix NATS wiring**: set `QUARRY_EDGE_NATS_URL` to the NATS instance that
   actually holds `QUARRY_EVENTS` and put `autocomplete-core` on a shared
   network with the Quarry publisher (`inter-plane-bus`), or point it at
   `ingestion-nats`. Without this the Sonic index stays permanently empty.
3. Ensure the verevonv3 gateway carries `AUTOCOMPLETE_INTERNAL_TOKEN` and sits on
   `verevon-net`, else suggestions silently stay empty even once the service is up.
4. Make `authorize()` fail-closed (reject when no token configured) as
   defense-in-depth.
5. Correct the README line that lists the Verevon proxy route as "pending".
6. Add a live smoke test (push → suggestion round-trip against a real Sonic) to
   the plane's test-endpoints so the outage would be caught, not masked by the
   gateway's empty-list degradation.
