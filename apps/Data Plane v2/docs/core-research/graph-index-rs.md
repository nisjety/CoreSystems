# graph-index-rs Research Dive

Live verification date: 2026-07-10 (this pass)
Prior baseline: 2026-06-07 (superseded, kept as an appendix below)
Prior flagged-but-unverified pass: 2026-07-10 earlier same-day plane audit (`apps/Data Plane v2/docs/core-research/plane-audit-2026-07-10.md`, 2026-07-10 addendum)

Scope: `apps/Data Plane v2/services/graph-index-rs` (Rust, container `dpv2-graph-index`, HTTP `:9203`, gRPC `:50053`)

## 2026-07-17 Neo4j GraphRAG read-model + graph-in-fusion delta

Landed the GraphRAG program (design: `docs/graphrag-neo4j-plan.md`). Decision:
**extend `graph-index-rs`** rather than add a new service — Neo4j is a
rebuildable, org-scoped **graph read-model** (same role as Qdrant/Quickwit),
Postgres stays canonical.

- **Infra/client:** `neo4j` compose service (org-scoped auth, healthcheck,
  `dpv2_neo4j` volume) + `src/neo4j.rs` (`neo4rs` Bolt). Behind `NEO4J_ENABLED`
  (default off), fail-closed on missing secret, boot-retry then degrade to the
  Postgres path — the core service has **no** `depends_on: neo4j`.
- **Dual-write:** `merge_extraction` (batched `UNWIND`/`MERGE`, idempotent on the
  Postgres PKs) runs downstream of `persist_extraction`, inheriting the
  org-visibility + restrictive-ZDR gates; non-fatal.
- **Traverse:** `POST /v1/graph/traverse` — native `*1..N` Cypher (org_id on
  seed, every edge, reached node), org-pinned via the verified `Principal`,
  `store::get_subgraph_visible` Postgres provenance re-join (Neo4j is never an
  authz source), transparent Postgres-BFS fallback.
- **Fusion:** retrieval-engine `arm_graph` folds `w_graph` into the fused RRF
  path (`search/graph.rs::graph_arm_candidates` adapter) — closes gap-data
  §16.1.1 and the Sovereign-plan Phase-4 "graph-in-fusion" step.
- **Tests:** unit (client, Cypher org-scoping, clamp, mirror shaping, fusion
  order) + a gated `#[ignore]` live-Neo4j roundtrip. Workspace clippy
  `-D warnings` + lib tests green. Not yet run against a live Neo4j deployment.

## 2026-07-15 final isolated acceptance delta

The final rebuilt service passed HTTP 401/401/200/403 and all six GraphService
families in the strict gRPC matrix. Authenticated cross-tenant calls returned the
exact graph tenant-mismatch guard. The real-authority Velion journey surfaced
only the authorized fixture's GraphRAG nodes, and supported signed graph events
passed the isolated broker matrix. The shared deployment remains unchanged.

## 2026-07-15 GraphRAG and Velion integration delta

- Signed document events now require exact equality between the verified
  envelope tenant and payload tenant. The verified claim supplies the tenant
  used for extraction and persistence; mismatches are acknowledged and dropped
  without graph mutation. Production posture cannot enable unsigned events even
  when both development gates are set.
- The Model Plane extraction hop no longer forwards a shared `x-api-key`.
  Graph-index mints a short-lived, audited, org-bound service token through Auth
  Core for exact `aud=inference-core` / `inference:invoke`, forwards only
  `Authorization: Bearer` to gRPC, and fails closed on missing/malformed
  issuance. The Control registry entry is audience-specific and cannot reuse a
  Control-policy scope.
- ZDR is explicit through extraction. Signed restrictive events never persist;
  direct-Azure extraction rejects ZDR before network egress. Model requests keep
  the restrictive flag.
- HTTP store failures now return sanitized 500 responses instead of successful
  payloads containing database errors; a missing entity is 404.
- Velion's Knowledge and onboarding graph routes derive tenant from the verified
  session and forward a session-minted Data bearer. The historical onboarding
  query-parameter IDOR and shared-key graph call are removed.

Verification dated 2026-07-15: `cargo test --no-default-features` passed
**28 tests** with one explicitly ignored disposable-PostgreSQL test; the focused
extractor suite passed **9/9**; `cargo clippy --all-targets --no-default-features
-- -D warnings` and Compose config validation passed. The isolated image
`fc138c8a6b23` carries revision
`eeebd0bc98c66434936460020958891066eb05fd`; its HTTP four-shape matrix returned
401/401/200/403. The current shared image predates this source, so this is
isolated effectiveness rather than a shared deployed/effective claim.

## Secure-MVP current state — 2026-07-10

- **Implemented:** HTTP and gRPC require verified RS256/JWKS user or scoped service
  claims and pin tenant identity. Cross-tenant path/query/header values are denied.
  Graph reads now filter through visible canonical documents before extraction;
  private or grant-only content is not treated as organization-wide graph input.
- **Tested:** `cargo test -p graph-index-rs` passed 20/20 tests after the auth,
  visibility, and event-containment changes.
- **Built/reachable/effective in isolation:** the revised image built locally
  with verified revision/build labels and passed the disposable HTTP tenant
  matrix. The shared deployment and the equivalent gRPC/broker matrices have
  not run on this source.
- **Containment/blockers:** unsigned mapping/extraction/cleanup consumers are
  disabled by default behind two insecure-development gates. Graph progression is
  ineffective until signed tenant-scoped events/NATS permissions exist. Outbound
  Model inference still requires a fully verified scoped service identity.
- **Coverage/audit:** Rust coverage and Rust dependency-audit tools were unavailable;
  no percentage/audit result was measured.

The remainder is a superseded, sanitized pre-fix audit retained to explain the
original failure. Its customer-specific response evidence has been removed.

## Historical pre-fix bottom line (superseded)

**This is a real, live, unauthenticated cross-tenant data exposure, and it is worse than the earlier same-day pass described.** The earlier pass characterized it as "returns real graph nodes/edges/claims with only an `X-Org-ID` header, no credential." That undersells it: `graph-index-rs` does not read `X-Org-ID`, `Authorization`, or any other header at all, for any route, on either the HTTP or the gRPC wire. Tenant scope is a bare `org_id` string taken from the URL path or query string and handed straight to the Postgres query layer. Anyone who can reach `:9203` — no header, no token, no cookie, nothing — can dump another org's entire knowledge graph (entities, relationships, claims, contradictions) by guessing, brute-forcing, or otherwise learning its `org_id`. A real, freshly-issued, verified Control Plane session for an unrelated user makes zero difference to the response.

This was reproduced live in the pre-fix pass against a populated tenant and with
an unrelated valid session. Customer identifiers, corpus sizes, response bodies,
and credentials have been removed.

## Live reproduction — 2026-07-10

Container confirmed healthy and reachable at the time of testing:

```
dpv2-graph-index   Up 14 hours (healthy)   0.0.0.0:9203->9203/tcp
```

Note the bind is `0.0.0.0:9203`, i.e. the host's Docker port mapping, not an internal-only address — this is reachable from anywhere that can reach the host on that port, not just from other containers on the compose network.

The sanitized historical matrix was:

| Shape | Result before the fix |
|---|---|
| No credential | 200 with populated tenant graph body (redacted) |
| Forged organization header | 200 with the same body |
| Forged bearer | 200 |
| Valid unrelated bearer | 200 |
| Valid bearer plus a different tenant selector | 200 |

The claims and export route families behaved the same way. This established a
complete absence of inbound authorization rather than a single spoofable-header
bug. No customer response content or credential is retained in this document.

## Root cause — exact file:line

`services/graph-index-rs/src/api.rs`, function `pub fn router()` (lines 14–33):

```rust
pub fn router(store: Arc<GraphStore>) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/readyz", get(readyz))
        .route("/v1/graphs/{org_id}", get(get_org_graph))
        .route("/v1/graph/entities/{entity_id}", get(get_entity))
        .route("/v1/graph/entities", get(list_entities))
        .route("/v1/graph/relationships/{entity_id}", get(get_relationships))
        .route("/v1/graph/claims", get(get_claims))
        .route("/v1/graph/contradictions", get(get_contradictions))
        .route("/v1/graph/expand", post(expand_graph))
        .route("/v1/graph/exports", post(create_export))
        .with_state(store)
}
```

There is no `.layer(...)` of any kind — no auth middleware, no tenant-context extractor, nothing between the socket and the handlers. Every data-bearing handler pulls `org_id` straight from a client-supplied value:

- `get_org_graph` — `Path(org_id): Path<String>` (api.rs:190)
- `list_entities`, `get_entity`, `get_relationships`, `get_claims`, `get_contradictions`, `expand_graph`, `create_export` — a `#[derive(Deserialize)]` query/body struct with a plain `org_id: String` field (api.rs:221-223, 226-233, 240-243, 246-250, 252-259, 261-269, 35-44)

and forwards it unmodified into `GraphStore` (`src/store.rs`), which does bind it as a parameterized SQL argument (no SQL-injection issue — the queries are all `$1`/`$2` bound), but with no check that the caller is entitled to that `org_id` at all.

`src/main.rs` confirms there is no wrapping at the server level either:

```rust
let app = api::router(store.clone());
...
res = axum::serve(listener, app) => { ... }
```

The gRPC wire has the identical gap: `src/grpc.rs`'s `GraphGrpc` service is registered in `main.rs` (`tonic::transport::Server::builder().add_service(grpc_service)`) with no `tonic` interceptor, and every RPC handler (`get_entity`, `list_entities_by_type`, `get_relationships`, `get_claims`, `get_graph_expansion`, `get_contradictions` — `src/grpc.rs:96,112,133,158,179,213`) takes `req.org_id` from the wire request message with the same no-check pattern.

Corroborating evidence that this was never wired, not regressed:
- `Cargo.toml` has no JWT/JWKS/auth crate at all (`axum`, `tower-http` with only `cors`+`trace` features, `tonic`, `sqlx`, `async-nats`, `reqwest`, `envy` — nothing auth-shaped).
- The one config field that looks auth-related, `internal_api_key` (`src/config.rs:33`), is consumed only outbound — as an `api-key` header graph-index-rs itself attaches when calling Model Plane's inference-core for extraction (`src/extractor.rs:41,71,158,197`) — never to validate inbound requests.
- `git log --oneline -- services/graph-index-rs/src/api.rs` shows exactly one commit touching this file (`1eecf8d0`, the original feature commit) — there is no prior version with auth that got dropped; it has been this way since the file was created.
- There is no `tests/` integration-test directory for this crate at all, and none of the 7 existing unit tests (`extractor::tests::*`, `grpc::tests::*`) touch the HTTP/gRPC surface or authorization — nothing in CI would have caught this.

## Same-class findings elsewhere in Data Plane v2 (re-verified live, for context)

The earlier same-day pass said data-quality's cost summaries and data-orchestrator's stale-embedding endpoints "similarly accept caller-selected organization scope without a credential." Re-verified live — true, but meaningfully less severe than graph-index-rs:

The sanitized control check returned 400 without an organization header and 200
with a caller-selected populated tenant. Tenant ID, model/cost counts, and response
body are redacted.

`data-quality-go`'s `OrgIDMiddleware` (`services/data-quality-go/internal/handler/quality.go:23-33`, wired at `services/data-quality-go/cmd/main.go:74,81,88`) and `data-orchestrator-go`'s identical copy (`services/data-orchestrator-go/internal/handler/orchestrator.go:16-23`) both **require** an `X-Org-ID` header to be present (400 without it) but never verify it against any signed credential — any caller-chosen value is accepted at face value. That is a real IDOR-class bug and belongs on the same remediation track, but it is one step better than graph-index-rs, which requires no header at all. Treat graph-index-rs as the most severe instance of this pattern in Data Plane v2 and fix it first or in the same change as a shared solution (see Recommended fix below), since a per-service patch here risks leaving the gRPC wire or a sibling HTTP route unfixed.

## Recommended fix

1. Add a `tower`/`axum` middleware layer to `api::router()` (and a matching `tonic` interceptor for `grpc.rs`) that requires and verifies a Control Plane-issued, signed audience token (the same `data-plane` JWT the v3 gateway already sends as a bearer alongside `x-org-id` — see `apps/Frontend Plane/velionv3/apps/gateway/src/domains/knowledge/shared.rs:60-91`; graph-index-rs currently receives this token on every gateway-originated call and ignores it), and derive `org_id` from the verified token claim rather than trusting the caller-supplied path/query/body value. Cross-check any caller-supplied `org_id` against the token's claim and reject on mismatch, matching the pattern already planned for documents-api's `pkg/authctx` (which is itself not yet implemented — see the plane audit's P0 finding on JWT/JWKS verification).
2. Apply the same layer to every route in the router, not just `/v1/graphs/{org_id}` — `entities`, `relationships`, `claims`, `contradictions`, `expand`, and `exports` are all currently open.
3. Add an integration test (`tests/authz.rs`) asserting 401/403 for: no credential, forged credential, and a valid credential for a different org than the one requested — this class of bug has zero test coverage today and a per-route manual review will not catch a regression.
4. Because the compose port mapping is `0.0.0.0:9203`, also confirm (outside this service's code) whether that host-wide bind is intentional; if graph-index-rs is meant to be reached only from other containers on the compose network, scope the port mapping to loopback or remove the host mapping entirely as defense in depth once the application-layer fix lands. This does not replace the application-layer fix — Docker's internal network already exposes the same gap to every other container regardless of host binding.

## Runtime shape (updated)

Key runtime entrypoints (all still accurate):

- `src/main.rs` — Postgres pool, extractor, JetStream consumer, HTTP server (`:9203`), gRPC server (`:50053`), orphan-cleanup subscriber, run concurrently via `tokio::select!`.
- `src/api.rs` — the HTTP surface audited above; also contains `graphml`/`markdown`/`json` graph export rendering (`render_graphml`, `render_markdown`).
- `src/extractor.rs` — graph extraction; routes through Model Plane's inference-core gRPC by default (`Backend::ModelPlane`), with a legacy direct-Azure-OpenAI fallback (`Backend::AzureOpenAi`) selected by `GRAPH_EXTRACTION_PROVIDER`.
- `src/store.rs` — `GraphStore`, all Postgres persistence and query backing (entities, relationships, claims, communities, text-unit mappings). All queries are correctly parameterized (no SQL injection), but none check caller entitlement to the bound `org_id`.
- `src/grpc.rs` — `GraphGrpc`, the gRPC mirror of the HTTP read surface, same no-auth gap.
- `src/community.rs` — community detection over the entity/relationship graph.
- `src/stream.rs` — NATS JetStream consumer for extraction jobs plus the orphan-cleanup subscriber that purges graph mappings for chunks orphaned by re-chunking.
- `src/config.rs` — env-driven config (`envy`); no auth-related settings besides the outbound-only `internal_api_key`.

Surface:

- HTTP admin/API on `9203` — confirmed live, healthy, and (per above) unauthenticated for every data-bearing route.
- gRPC graph surface on `50053` — confirmed same gap, no interceptor registered.
- Image: `data-plane-v2-graph-index`, built 2026-07-07T21:13:04Z per `docker inspect`; no source-revision label present in image metadata (only `com.docker.compose.*` labels) — health checks prove process liveness only, not which source revision is actually running.

## API and relationship map

- `index-engine-rs` → `graph-index-rs`: indexed content progression drives graph extraction (via JetStream, `src/stream.rs`).
- `graph-index-rs` → Postgres: persists entities, relationships, claims, communities, mappings.
- `graph-index-rs` → Model Plane inference-core (gRPC): extraction backend, using `internal_api_key` outbound.
- `retrieval-engine-rs` → `graph-index-rs`: graph retrieval and graph-aware search (`retrieval-engine-rs/src/search/graph.rs`) — this means the unauthenticated read surface also feeds into retrieval-engine's results; retrieval-engine has its own `src/authz/context.rs`, but that only governs retrieval-engine's own entry points, it cannot retroactively make graph-index-rs's direct surface safe.
- v3 gateway (`apps/Frontend Plane/velionv3/apps/gateway/src/domains/knowledge/workspace.rs:407-429`) → `graph-index-rs`: calls `GET {graph_index_url}/v1/graphs/{org}` directly, attaching `x-org-id` and a `data-plane` bearer token via `shared::fetch_json_bearer` / `internal_request` (`apps/gateway/src/domains/knowledge/shared.rs:60-91`) — both are sent on every call and both are currently discarded by graph-index-rs, so the gateway's defense-in-depth intent is silently ineffective today.

## Quality gate (this service specifically)

| Check | Result | Notes |
|---|---|---|
| `cargo fmt -p graph-index-rs -- --check` | Pass | No formatting drift in this crate (the previously reported drift is in embedding-engine-rs, index-engine-rs, retrieval-engine-rs, not here). |
| `cargo clippy -p graph-index-rs --all-targets -- -D warnings` | Pass | Clean; the previously reported clippy failure is at `retrieval-engine-rs/src/pipeline/orchestrator.rs:58`, a different crate. |
| `cargo test -p graph-index-rs` | Pass | 7 unit tests, all green; none exercise the HTTP/gRPC surface or authorization — zero coverage of the finding above. |
| Live HTTP auth probe | **Fail (P0)** | Every data-bearing route returns 200 with real tenant data regardless of headers/credentials presented; see reproduction above. |
| Live gRPC auth probe (code review) | **Fail (P0)** | Same gap confirmed by inspection of `src/grpc.rs`; no interceptor registered in `main.rs`. |
| Docker health | Pass | `dpv2-graph-index` reports healthy; proves process liveness only, not tenant-safe readiness. |

## Duplicates, redundancies, and inactive surfaces

No explicit inactive source residue in the active service tree. Older GraphRAG planning language describes future-facing AST-heavy graph work broader than the current runtime — unchanged from the 2026-06-07 baseline, still just a documentation-scope note, not a functional gap.

## Historical bottom line (superseded)

`graph-index-rs` is live, structurally coherent, and functionally correct for its happy path (extraction, storage, query, export all work, and the SQL layer is properly parameterized). Its single, critical defect is that the entire HTTP and gRPC surface trusts the caller's own claim of `org_id` with literally no verification — not a spoofable-header check, not even that; there is no auth code in this crate at all. This was live-reproduced against real tenant data ([customer redacted]) using no credential, a forged credential, and a genuine but unrelated Control Plane session, all with identical (successful, full-data) results. This is the most severe finding in Data Plane v2 and should be fixed before any multi-tenant exposure of this service, ahead of the related-but-lesser data-quality/data-orchestrator header-trust gaps.

---

## Appendix: 2026-06-07 baseline (superseded by the above, kept for history)

Generated: 2026-06-07

Non-generated file count from the tree at that time: about `12`.

### Original snapshot

`graph-index-rs` is the graph extraction and graph-query core for Data Plane v2. It persists entities, claims, relationships, communities, and graph mappings derived from indexed content. Current evidence highlighted:

- Rust service with HTTP admin/API plus gRPC.
- Event-driven extraction from indexed content.
- Orphan-cleanup subscriber exists to prevent stale graph residue after re-chunking.
- Older GraphRAG planning language appears broader than the currently visible runtime.

### Original assessment (no longer sufficient — see live verification above)

The original pass reported "no explicit code stubs or backup files were found in the active service tree" and closed with "the main risk is documentation drift around how much graph ambition is already runtime reality." That assessment did not include a live authentication/authorization probe and materially understated the service's actual risk profile. The 2026-07-10 live verification above supersedes it for anything related to trust boundaries; the architectural/structural notes (runtime shape, API map, duplicates) remain accurate and have been folded into the updated sections above.
# 2026-07-16 independent-startup addendum

Graph no longer performs a synchronous Auth Core JWKS fetch during process
startup in Compose. It loads the read-only Auth Core public verification key
mounted at `/app/keys/convex-auth.pub`; protected HTTP/gRPC requests still
require a verified RS256 bearer with the configured issuer, audience, time,
identity, and tenant claims. Model Plane inference remains a lazy, scoped
runtime call and fails closed while that authority is offline.

The base Data Plane network is local. Use `make standalone-up` for a disposable
local boot and `make cross-plane-up` only after the deployment-owned shared
network and service principals are provisioned. No production credential or
customer graph was copied into this evidence.
