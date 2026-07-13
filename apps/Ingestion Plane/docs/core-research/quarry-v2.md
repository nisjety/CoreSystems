# Quarry-v2 Research Dive

Generated: 2026-07-12 · re-verification pass for the 2026-07-11 Ingestion Plane audit cycle
(supersedes the 2026-06-07 dive and re-checks the 2026-07-02/07-10 plane audit findings).

Scope: `apps/Ingestion Plane/Quarry-v2` — the split web/search ingestion core:
`quarry-edge` (`127.0.0.1:8082`), `quarry-control` (`0.0.0.0:8081`),
`quarry-orchestrator` (no host port). Rust edge/runtime + Go control/orchestrator.

> **2026-07-12 source update:** production quarry-control now forces HMAC enforcement, production edge refuses a missing/invalid internal signer, and browser-agent Navigate actions are preflight/DNS guarded at both HTTP and WebSocket boundaries. `cargo test --workspace` is green. Running images remain stale; SearXNG effectiveness and redirect/rebinding browser interception remain release blockers.

Evidence grades used below: **[live-curl]** = observed over the published port right
now; **[source-only]** = read from disk (may differ from the running image);
**[logs]** = `docker logs`; **[inspect]** = `docker inspect` config/state.

> Environment caveat: Docker's containerd content store is corrupted, so
> `docker exec` and image rebuild are broken. Every Quarry container shows
> `(unhealthy)` because its healthcheck runs `wget` *inside* the container
> (exec path) which fails — **the processes themselves serve traffic fine**
> (proven by host curl below). All config/env facts here come from the
> compose file, `.env`, and `docker inspect`, not from `docker exec`.

## Snapshot

Quarry-v2 is real, central, and — for the fetch/scrape path — **live-verified end
to end today**. The runtime split is intact:

- `quarry-edge` — public ingest boundary: `/v1/scrape`, `/v1/crawl`, `/v1/batch`,
  `/v1/search*`, `/v1/answer`, auth guards, SSE, artifact + event fan-out.
- `quarry-runtime` — hot-path execution engine (drivers, browser, queues, answer,
  vector hooks, Data Plane ingest client).
- `quarry-control` (Go) — durable operator API: jobs, stores, snapshots, artifacts,
  profiles, schedules, webhooks, sources, blocklists, presets.
- `quarry-orchestrator` (Go) — Temporal worker polling posted jobs.

Headline: the **UNSAFE dev posture from the 2026-07-10 audit is still present**
(dev auth bypass on, HMAC not required, control published on all interfaces). The
**scrape/fetch pipeline is genuinely real** (not mocked). **Web search is wired but
returns zero results** because the bundled SearXNG's engines are all unresponsive
right now. The DataPlane ingest **contract is fixed and its tests pass (21/21)**.

## Live verification (host curl / inspect, 2026-07-12)

| Check | Result | Evidence |
|---|---|---|
| `quarry-edge` `/health` | 200 `ok` | [live-curl] |
| `quarry-edge` `/ready` | 200 `ready` | [live-curl] |
| `quarry-control` `/health` | 200 | [live-curl] |
| `quarry-edge` `POST /v1/scrape` example.com | **200, real markdown + links + blake3 fingerprint + stored artifacts** | [live-curl] |
| `quarry-edge` `POST /v1/search` (valid bearer) | 200 but `results:[]`, `count:0`, `provider:"hybrid"` | [live-curl] |
| `quarry-edge` `/v1/search` **no bearer** | 401 (dev bypass still needs a *non-empty* bearer) | [live-curl] |
| SearXNG direct JSON query | 200, **0 results**, all engines `HTTP connection error` | [live-curl] |
| `quarry-control` `GET /v1/jobs` **no auth** | **200** (unsigned request accepted) | [live-curl] |
| `quarry-edge` HTTP 500s last 12h | **0** (was 62/12h in the 2026-07-10 baseline) | [logs] |

## Dev-posture safety findings (the important ones — RE-VERIFIED)

All three unsafe findings from the prior audit **still stand**:

1. **`QUARRY_EDGE_AUTH_DEV_BYPASS=1` — STILL ON.** Set in
   `apps/Ingestion Plane/.env:7` and `.env.example:10`, and confirmed live in the
   running container env (`docker inspect quarry-edge`). The compose *default* was
   flipped to `0` (`docker-compose.yml:57`, "bypass OFF by default"), but `.env`
   overrides it back to `1`. Source `crates/quarry-edge/src/auth.rs` (`dev_bypass_enabled`,
   read once and latched at startup) accepts any **non-empty** bearer without JWKS
   verification and logs `WARN: accepting bearer without verification`. Empty/absent
   bearer → 401. [live-curl + inspect + source]

2. **`QUARRY_INTERNAL_HMAC_REQUIRED=0` — STILL 0.** Confirmed in the running
   `quarry-control` env. With a secret present but `require=false`, the Go HMAC
   middleware (`internal/httpx/hmac.go`) runs in **"rollout mode": unsigned requests
   pass, only present-but-bad signatures fail** (`verify()` returns nil when all three
   sig headers are absent and `require==false`). This is why every `/v1/*` control
   route answered **without any auth header** (jobs, schedules, sources, benchmarks —
   all 200/400/404, never 401). [live-curl + inspect + source]

3. **`quarry-control` is still host-published on `0.0.0.0:8081`** (`docker-compose.yml:199-200`
   `- "8081:8081"`; `docker ps` shows `0.0.0.0:8081->8081` and `[::]:8081`). Unlike
   `quarry-edge`, which was correctly narrowed to loopback (`127.0.0.1:8082:8082`,
   compose lines 140-145), control is reachable from the LAN. **Combined with finding
   #2, anyone who can reach the host on 8081 can read/enumerate/create durable Quarry
   resources with no credential.** This is the single highest-value hardening gap in
   the plane. [live-curl + inspect + source]

Improved since baseline: `QUARRY_INTERNAL_SECRET` and `QUARRY_EDGE__INTERNAL_SECRET`
now carry a real 64-hex value and the compose uses `${...:?must be set}` (no insecure
default) — the run_page org-binding HMAC path is properly keyed. [inspect + source]

## Search vs scrape — the real-vs-mock distinction

- **Scrape/fetch is REAL.** `POST /v1/scrape https://example.com` returned a full
  live result: `status:200`, extracted `markdown`/`html` artifacts, discovered links,
  a `blake3:` content fingerprint, `change.status:"new"`, and a real `run_id`
  (`run_01KX…`). No mock. The edge has working outbound egress. [live-curl]

- **Web search is WIRED but currently returns nothing.** `/v1/search` is a real route
  (`crates/quarry-edge/src/search_routes.rs` → runtime `SearchProvider`, SmartRouter
  over SearXNG primary + optional Brave). It answered 200 with `provider:"hybrid"` but
  `count:0`. Querying the bundled SearXNG (`127.0.0.1:8888`) directly returns 200 with
  **0 results and every engine in `unresponsive_engines` reporting
  `HTTP connection error`** (brave, duckduckgo, google, startpage, wikidata). Since the
  edge itself *can* reach the internet (scrape works), this is a **SearXNG-container
  networking/engine problem, not a Quarry code defect**. Web search will stay empty
  until SearXNG's egress/engine access is restored. This nuance corrects the prior
  baseline claim that "real SearXNG results come back". [live-curl]

- **Answer synthesis:** `crates/quarry-edge/src/answer_routes.rs` notes the
  model-gateway `/v1/invoke/stream` is a **Model-Plane-side stub**; the edge
  deliberately falls back to the working non-streaming `/v1/invoke` (`mp_client.rs`
  has both; the non-streaming path is used for the full answer). Graceful, not broken.
  [source-only]

## quarry-control stub inventory (re-checked)

`cmd/control/main.go` mounts every resource family behind the HMAC middleware. State
of the previously-flagged partials:

- **Schedules `trigger`/`backfill` — STILL Temporal stubs.**
  `internal/resources/cycle23.go` `scheduleTriggerStub`/`scheduleBackfillStub` validate
  the request and return **202 with `"note":"Temporal SDK not yet wired; … is a stub."`**
  (`TODO(D5): wire go.temporal.io/sdk`). Live: trigger/backfill on a non-existent id
  returns 404 (would return the stub-202 for a real schedule). [source + live-curl]
- **`/v1/benchmarks` and `/v1/request-queues` — STILL stub-shaped.** Both return
  `{items:[], total_estimated:0}` live; `main.go:167-171` comments them as
  "stub-shaped where the schema isn't ready". [source + live-curl]
- **`/v1/sources` — now REAL** (upgraded since the 2026-06 dive). Live it returns
  `400 "org_id required"` (org-scoped CRUD over `quarry_sources`, PR-7), no longer an
  empty stub. Note the org scope is taken from the **query param**, unauthenticated —
  fine only because control is meant to be private. [source + live-curl]
- **Temporal `SDKClient` — still stubbed** (`internal/temporal/client.go`: "concrete
  client wired in cycle 24"). [source-only]
- **`IdempotencyKeyHandler` — no-op placeholder** (`cycle23.go:492` returns false;
  pg-backed table deferred to cycle 24). [source-only]

## DataPlane ingest contract — RESOLVED (confirmed from the Ingestion side)

`cargo test -p quarry-core --test contracts` → **21 passed, 0 failed**. The
`DataPlaneIngestRequest` carries `initiator_user_id` and `visibility`
(`crates/quarry-core/src/contracts.rs:186-191`); the tests
`data_plane_ingest_request_serde_roundtrip` and
`…_omits_absent_ownership_fields` both pass (absent ownership fields are omitted from
the wire for system/connector ingest). Matches the Data Plane Phase 2 audit's 21/21.
[live cargo test]

## In-memory / durability paths (production-guard review)

- **`QUARRY_EDGE__ARTIFACT_BACKEND=memory` is LIVE** (compose default `memory`,
  confirmed in `docker inspect`). `crates/quarry-edge/src/main.rs:199-214` selects
  `InMemoryStore` for `memory` (filesystem/S3 are real alternatives). So scraped
  artifacts in the running edge are **not durable across restart**. Same for the local
  index (in-memory Tantivy unless `QUARRY_EDGE__LOCAL_INDEX_DIR` is set). [inspect + source]
- **`quarry-runtime` request queue**: `RedisRequestQueue`/`PostgresRequestQueue` are
  still `TODO`; `InMemoryRequestQueue` is the active generic path
  (`crates/quarry-runtime/src/request_queue.rs:22-26`). [source-only]
- **`quarry-control` in-memory store is a dev fallback only.** `main.go:122-124` uses
  `store.NewMemory()` **only when `QUARRY_CONTROL_DSN` is empty**; the running
  container sets the DSN to the `quarry_v2` Postgres DB, so it runs the pg store
  (`internal/store/pg`). The in-memory path is correctly guarded. [source + inspect]
- Profile store: compose sets `QUARRY_EDGE__PROFILE_STORE_KIND=postgres` against a
  dedicated `quarry_edge` DB, so named browser profiles are durable (falls back to
  in-memory with a warning if the DSN is unreachable). [source + inspect]

## Uncommitted WIP vs. running image (drift warning)

`git status` shows **~20 modified files / +632 −255 lines under Quarry-v2, uncommitted**
(edge `main.rs`/`config.rs`/`agent_routes.rs`; runtime `pipeline.rs` +189,
`data_plane_client.rs` +151, `ingest_client.rs` +167, `events.rs` +110, `agent_loop.rs`,
`http3.rs`, `observation.rs`, `page_renderer.rs`, `grpc/ingest_adapter.rs`; transform
`images.rs`/`attributes.rs`). Because **image rebuild is Docker-blocked, none of this is
in the running containers** — source findings below the fetch layer may not match live.

Notable WIP (source-only, NOT live):
- **ZDR defense-in-depth**: `ingest_client.rs`/`ingest_adapter.rs` now route both the
  HTTP and gRPC durable-ingest paths through a single `ensure_durable_ingest_allowed(zdr)`
  gate (previously the gRPC path restated deny semantics separately). New
  `validate_data_plane_bearer()` and a `data_plane_service_token` config field. This is
  the "quarry-edge ZDR gap in start_run" fix referenced in recent commits — a genuine
  security improvement that the running image does not yet have. The running edge still
  enforces ZDR on the HTTP path (older code did), so the live posture is safe-but-older.

## Findings (severity-ordered)

1. **HIGH — unauthenticated, LAN-reachable control plane.** `0.0.0.0:8081` +
   `QUARRY_INTERNAL_HMAC_REQUIRED=0` (rollout mode) ⇒ any host-reachable client can
   read/enumerate/create durable Quarry resources with no credential. Fix: bind control
   to `127.0.0.1` (mirror the edge fix) **and** set `QUARRY_INTERNAL_HMAC_REQUIRED=1`
   now that a real secret is provisioned. [live-curl + source]
2. **HIGH — edge dev auth bypass on in `.env`.** `QUARRY_EDGE_AUTH_DEV_BYPASS=1`
   accepts any non-empty bearer without JWKS verification. Acceptable only for an
   isolated edge with no reachable auth-core; must be `0` anywhere auth-core exists.
   [live-curl + source]
3. **MEDIUM — web search returns zero results.** SearXNG engines all
   `HTTP connection error`; the search feature is effectively down even though the
   Quarry route is healthy. Operational (SearXNG egress/engine config), not a Quarry
   bug. Restore SearXNG outbound access or configure reachable engines. [live-curl]
4. **MEDIUM — non-durable artifact/index backends live.** `ARTIFACT_BACKEND=memory`
   and in-memory Tantivy/request-queue mean scraped artifacts, local index, and queued
   work are lost on restart. Switch the edge to `filesystem`/`s3` artifacts and set a
   local-index dir for any environment expected to retain results. [inspect + source]
5. **LOW — Temporal-dependent control ops are stubs.** Schedule `trigger`/`backfill`
   return 202 "not yet wired"; `SDKClient` and idempotency table deferred to "cycle 24".
   Callers get typed responses but no durable scheduling. [source + live-curl]
6. **LOW — large uncommitted WIP not in running images.** Rebuild is blocked; commit +
   rebuild once the content store is repaired so the ZDR gRPC hardening and pipeline
   changes actually run. [source]

Resolved since baseline: the 62 HTTP-500s/12h on the edge are **gone (0 in 12h)**;
`INTERNAL_SECRET` is real and default-free; `/v1/sources` is a real CRUD; the
DataPlane ingest contract passes 21/21.

## Out of scope for this doc

The program's headline shipping/Visma goals — the **shipping-core Bring delivery-time
parsing defect** and **whether shipping-core is reachable as a Model Plane tool**, plus
the **Visma MCP** — live in `shipping-core` and `integration-corev2`, not Quarry-v2.
They are tracked in their own core-research docs and are not re-verified here.

## Bottom line

Quarry-v2's fetch/scrape/extract path is real and healthy today, and the durable
contract with Data Plane is solid (21/21). The blockers are **operational/posture, not
correctness**: an unauthenticated LAN-exposed control plane, an edge dev-auth bypass
left on, a dead SearXNG search backend, and non-durable in-memory storage defaults in
the running images. None of these are fixable by rebuild while the Docker content store
is corrupted; the highest-priority items (control bind + HMAC-required) are pure
compose/env changes that can land the moment the stack is next brought up cleanly.
