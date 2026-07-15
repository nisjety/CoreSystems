# retrieval-engine-rs Research Dive

Generated: 2026-06-07
Updated: 2026-07-10 (live re-verification pass; supersedes the 2026-06-07 snapshot below where the two disagree — see "2026-07-10 Live Re-Verification" for what actually changed vs what is still stale)

Scope: `apps/Data Plane v2/services/retrieval-engine-rs`

## 2026-07-15 final isolated acceptance delta

The real Auth/User/Control fixture proves an authorized user can use Knowledge,
GraphRAG, and navbar retrieval while a second tenant remains absent from
serialized browser/API payloads. The final HTTP matrix passed 401/401/200/403,
and all Retrieval, Document, and Knowledge gRPC families passed exact outcomes
in the 31-method/124-shape matrix.

Control policy token issuance has a seven-second budget because Auth Core waits
for a durable audit PubAck with a five-second downstream budget. Retrieval retries
only connect/timeout/503 once after 150ms; it never retries 401/403, malformed
contracts, or explicit denial. Exhaustion makes exactly two attempts, no decision
call, is uncached, and maps to gRPC `Unavailable`; verified non-membership remains
`PermissionDenied`. Policy and interceptor line coverage measure 93.76% and
88.14%. The final all-target suite passed **212 tests with 10 explicit
infrastructure ignores**. The shared deployment is unchanged.

## 2026-07-15 Velion and inference-auth delta

- HTTP now requires a cryptographically verified bearer and rejects the
  fleet-shared API-key identity path. The verifier requires RS256 plus configured
  issuer/audience and registered `exp`, `nbf`, `aud`, `iss`, `sub`; missing,
  expired, wrong-algorithm, wrong-issuer/audience, and non-canonical identity
  shapes return 401.
- Query embeddings no longer call Inference Core with a shared key. Retrieval
  mints one short-lived, audited, org-bound `aud=inference-core` service bearer
  per batch for exact `inference:invoke`, forwards only `Authorization: Bearer`,
  and retains caller ZDR. Auth Core's new `scopesByAudience` policy prevents the
  retrieval service's `control-policy` decision scope from being reused for
  inference (and vice versa).
- Velion's gateway mints its Data bearer from the verified session for search,
  chunks, trace/source expansion, and navbar search; failure is a sanitized 503
  with no internal-key fallback. The gateway maps SPA `limit`/`kinds` to
  retrieval `top_k`/filters and maps `candidates`/`sources` back to the UI
  contract. Navbar search now calls the implemented `/v1/knowledge/search`.

Verification dated 2026-07-15: `cargo test --all-targets` passed **175 unit/bin
tests plus 18 non-ignored integration tests** (**193 non-ignored total**); 10
infrastructure-dependent cases remain explicitly ignored. Minimal retrieval
requests now default an omitted `filters` member to empty filters, with a focused
regression test. `cargo fmt --all -- --check` and
`cargo clippy --all-targets -- -D warnings` passed.
The five focused HTTP JWT regressions and 19 focused embedding/service-auth
tests passed. Auth Core's focused registry suite passed **12/12**, build passed,
and changed-module coverage measured **90.19% statements, 93.84% branches, 100%
functions, 89.79% lines**. The current-source isolated retrieval image
`5a9bd5f96a14` carries revision
`eeebd0bc98c66434936460020958891066eb05fd`; its HTTP four-shape matrix returned
401/401/200/403. Shared running images still predate this source.

## Secure-MVP current state — 2026-07-10

- **Implemented:** strict RS256/JWKS auth covers HTTP and gRPC; tenant/user are
  claim-pinned. Auxiliary sources, freshness, chunks, compare, timeline, graph,
  wiki, contradictions, pack, and trace paths use verified identity and the common
  ownership/visibility contract. Trace tenant lookup and chunk-schema drift are
  fixed in source. Embedding and semantic-cache writes bypass under ephemeral ZDR.
- **Contained writes:** deprecated gRPC Create/Delete/Bulk permanently return
  `FailedPrecondition` after auth/scope/tenant checks and before storage. The
  historical one-variable write escape hatch no longer enables direct SQL writes.
- **Control contract:** retrieval mints a <=300-second org-bound
  `aud=control-policy` service JWT, validates its exact profile, and calls the
  versioned Control decision endpoint with Bearer only. Positive membership is
  cached for 30 seconds; denials/errors are not cached. The matching Control route
  independently verifies signature/caller/scope/org/reason.
- **Tested:** 64/64 library, 7/7 policy-token, and 1/1 offline transport
  containment tests pass after the bearer
  migration; auxiliary-handler, isolated-Postgres visibility, search, and trace
  security suites also pass. Strict all-target clippy passes.
- **Built/reachable/effective in isolation:** the revised image built locally
  with verified revision/build labels. The disposable stack accepted a
  cryptographically signed authorized-tenant bearer and rejected a spoofed
  tenant. This used an isolated authority fixture; the real Auth Core/User Core
  user journey and shared deployment have not completed.
- **Key isolation:** Compose mounts only Control's public verification file;
  retrieval no longer receives the directory containing the signing-key file.
- **Blockers:** Model Gateway, Execution Core, and Inference Core gRPC are
  contained off by default pending verified principal/tenant contracts. Disabled unsigned
  index/embedding/graph/Quickwit consumers also prevent a complete ingestion-to-
  retrieval flow. Rust coverage and dependency-audit tooling were unavailable.

The remainder is a superseded, sanitized pre-fix audit. Its earlier 403/404/cache
observations remain historical root-cause evidence, not current runtime proof.

Container: `dpv2-retrieval-engine`, HTTP `:8014->8004`, gRPC `:50062->50052`. Confirmed `Up 14 hours (healthy)` at time of this pass, alongside all other Data Plane v2 containers (documents-api, graph-index, embedding-engine, index-engine, quickwit-adapter, data-quality, data-orchestrator, wiki-store, quickwit, postgres, qdrant, nats, minio, dragonfly) — all reported healthy, matching the prior pass from earlier today.

## Historical pre-fix live re-verification (superseded)

This section reports what was actually checked live today (container exec, curl against the running HTTP surface, direct Postgres/Dragonfly inspection, `cargo build`/`clippy`/`test`) — not just re-reading source. Everything below is either CONFIRMED (reproduced live), REFINED (the shape of the bug is different from the one-line summary in the prior pass), or FIXED (no longer reproduces).

### 1. `CONTROL_PLANE_ENFORCEMENT=strict` — confirmed live, but it is a no-op on the traffic that actually matters

Live config, read directly off the running container:

```
CONTROL_PLANE_ENFORCEMENT=strict
JWT_PUBLIC_KEY_FILE=/app/keys/convex-auth.pub
JWT_REQUIRED_AUDIENCE=data-plane
JWT_REQUIRED_ISSUER=http://localhost:3011/api/convex-auth
INTERNAL_API_KEY=<redacted>
```

`strict` is real at the config layer (`EnforcementMode::from_env()` in `src/authz/policy.rs`), and it does construct `HttpPolicyClient` instead of `NoopPolicyClient` (`src/main.rs` ~154-167). But tracing what that client actually does live surfaces a chain of three separate problems that combine into "strict mode currently enforces nothing on the coarse org axis for the traffic that exists in production":

1. **The membership/permissions routes it calls don't exist as it expects them to.** `HttpPolicyClient::fetch_decision` (`src/authz/policy.rs:116-173`) calls:
   - `GET {USER_SERVICE_HTTP_URL:-http://user-service:3012}/internal/v1/users/{user_id}/memberships/{org_id}`
   - `GET {ORG_CORE_HTTP_URL:-http://org-core-service:8080}/internal/v1/orgs/{org_id}/users/{user_id}/permissions`

   Both historical endpoints resolved but returned HTTP 401, with response bodies
   redacted. The current source uses the versioned decision contract described in
   the authoritative section above.
   - `fetch_decision` only distinguishes `Strict` vs `Permissive` semantics on a **transport-level `Err`** (connection refused/timeout). A non-2xx HTTP response (which is what actually happens here) is *unconditionally* treated as `PolicyDecision::deny("denied:no_membership")` regardless of `EnforcementMode` (`policy.rs:127-130`). So today, `strict` and `permissive` would behave identically for this specific failure mode — both deny, both for the wrong reason (looks like "not a member" in the audit log, not "policy backend is unreachable/misconfigured").

2. **On the API-key path (the one the gateway actually uses), that deny is silently absorbed and never denies anything.** `velionv3`'s gateway (`apps/gateway/src/domains/knowledge/shared.rs::internal_request`) always attaches `x-internal-api-key` + `x-user-id` (+ optionally a `data-plane` audience Bearer token) on every Data Plane v2 call. In `auth_middleware` (`src/api/mod.rs:110-144`), the API-key branch is checked *before* the Bearer branch, so a request carrying both always takes the API-key path. On that path:
   ```rust
   let decision = state.policy.resolve(&uid, &org_id).await;
   if decision.is_member {
       ctx.acl = decision.acl;
   }
   ```
   `ctx.acl` starts life as `EffectiveAcl::allow_all()` and is *only overwritten* when `decision.is_member == true`. Since `decision.is_member` is always `false` today (problem #1), `ctx.acl` simply never gets overwritten and stays `allow_all()` — identical to what it would be with `CONTROL_PLANE_ENFORCEMENT=off`. The code comment even says explicitly: *"We do NOT deny on the api-key path (trusted internal caller) — per-user ownership is enforced at the step-6 post-filter regardless."* So this is by design a non-denying path; combined with problem #1 it means the coarse org-axis PolicyClient currently contributes **nothing** to the live gateway traffic pattern, in either `strict` or `permissive` or `off`.

3. **A pure Bearer-JWT caller (no internal key attached) would be hard-denied on every request.** The JWT branch (`api/mod.rs:177-193`) has no such escape hatch: `if !decision.is_member { ...; return Err(StatusCode::FORBIDDEN); }` unconditionally. Combined with problem #1, any caller that authenticates with *only* a verified Bearer token (no internal key) would get a `403 FORBIDDEN` on every single request while `CONTROL_PLANE_ENFORCEMENT` is `strict` or `permissive` — not because they lack permission, but because the policy backend call 401s. This path is not currently exercised (the gateway always also sends the internal key, so it never reaches this branch), so the bug is latent rather than live-breaking anything today, but it means "switch off the internal-key fallback" (a stated goal in several other DPv2/CP docs once `AUTHCTX_ENFORCE`/audience tokens fully roll out) would immediately turn this into a 100%-denial outage for every Bearer-only caller.

**Answer to "does strict mode reject requests without a valid org/session, or just log a warning?"**: neither, in practice. It does perform a real HTTP round-trip and does compute a real (if wrong) `PolicyDecision`, and that decision unconditionally denies — but that denial is a no-op on the code path Data Plane v2 actually receives traffic on (API-key + x-user-id), because the API-key branch was deliberately written to never act on a policy deny. The org axis is, right now, live-enforced by nothing except the separately-always-on document ownership/visibility post-filter (`authz::visibility`, unaffected by this flag) and the auth-middleware's own JWT-audience/org-header cross-check (`header_org != claims_org` mismatch → 403, which is real and unrelated to `HttpPolicyClient`).

JWT signature verification itself (separate concern from the org-membership PolicyClient above) **is real** in this service — `src/api/mod.rs::verify_jwt` does genuine RS256 verification via a JWKS cache or a static PEM loaded from `JWT_PUBLIC_KEY_FILE` at startup (`main.rs:50-64`). This is a materially different (and better) implementation than documents-api's `pkg/authctx`, which the baseline correctly flagged as having unimplemented signature verification. Do not conflate the two services' auth stacks going forward — retrieval-engine-rs's JWT path is real; its org-membership *policy* path is the broken piece.

### 2. Trace endpoint's empty-organization bug — CONFIRMED STILL PRESENT, live-reproduced

`trace::get_trace(pool, trace_id, org_id)` (`src/trace/mod.rs:142-181`) runs `WHERE trace_id = $1 AND org_id = $2`. Two call sites hardcode `org_id = ""` instead of threading the caller's real org through:

- `src/api/mod.rs:430-434` — the HTTP handler for `GET /v1/retrieval/{trace_id}` doesn't even accept an `AuthContext` extension parameter; it calls `trace::get_trace(&pipeline.pool, &trace_id, "")` directly.
- `src/search/timeline.rs:65-70` — `replay_trace(pool, trace_id)` (used by timeline/replay callers) does the same: `crate::trace::get_trace(pool, trace_id, "").await`.

Live reproduction historically sampled a valid tenant trace, then called the HTTP
endpoint with the matching tenant and an internal credential. IDs and row counts
are redacted:

```
curl -H 'x-internal-api-key: <key>' -H 'x-org-id: [redacted-org]' \
  http://localhost:8004/v1/retrieval/[redacted-id]
→ HTTP 404 (response body redacted)
```

So this is not a stale doc note — it reproduces exactly as described, on the current build, right now. Because no row in `retrieval_runs` currently has an empty `org_id`, the practical effect today is that `GET /v1/retrieval/{trace_id}` and `replay_trace`/timeline-replay are **100% non-functional** for every caller (always 404), rather than an active cross-tenant leak — but it is a broken-by-construction org filter, not a security control that happens to be strict. If any row is ever inserted with an empty/null `org_id` (local/dev seeding, a future migration, a bug elsewhere), this becomes a straightforward cross-org read: any caller could then fetch that trace under any org header, since the query would spuriously match. The gRPC equivalent (`src/grpc/retrieval_svc.rs:266-272`) is not affected — it correctly threads `req.org_id` through.

**Fix shape** (not applied — this pass is audit-only): give `get_trace` the HTTP handler's `AuthContext`, thread `ctx.org_id` through instead of `""`, and do the same in `timeline::replay_trace` by taking an `org_id: &str` parameter from its caller.

### 3. `RerankClient::new` — confirmed genuinely dead, not just "possibly"

`cargo build` and `cargo clippy` both now emit the compiler's own dead-code warning:

```
warning: associated function `new` is never used
  --> services/retrieval-engine-rs/src/search/rerank.rs:46:12
```

Grepped the entire Data Plane v2 workspace (`grep -rn "RerankClient::new(" .`) — zero matches anywhere. The only constructor actually used is `RerankClient::with_endpoint`, wired from `main.rs` off `cfg.rerank_endpoint` / `cfg.rerank_use_api_key` (so the public-Cohere-vs-Azure-Foundry endpoint switch is live and working; `::new` is just a leftover convenience wrapper that was superseded by `::with_endpoint` and never deleted). Safe to remove `RerankClient::new` (and its now-implicit default endpoint/bearer behavior, which `with_endpoint("", ..., true)` already reproduces) whenever someone next touches this file.

### 4. Clippy doc-comment lint at `orchestrator.rs:58` — confirmed still present, and it's the *only* thing standing between this crate and a clean `-D warnings` clippy run

`cargo clippy --all-targets -- -D warnings` fails to compile the crate with exactly one error:

```
error: doc quote line without `>` marker
  --> services/retrieval-engine-rs/src/pipeline/orchestrator.rs:58:9
   = note: `-D clippy::doc-lazy-continuation` implied by `-D warnings`
```

Root cause, precisely: the doc comment on the `visual_embedder` field (`orchestrator.rs:55-58`) reads

```rust
/// Visual RAG arm query embedder (Cohere Embed v4). `None` when the visual
/// arm is not configured (text-only deployment); when present and `w_visual`
/// > 0 the orchestrator embeds the query into Embed v4's multimodal space and
/// fuses page-image hits from `qdrant_visual_collection`.
```

The line wrap happened to put `"> 0 the orchestrator..."` at the *start* of a doc-comment line. rustdoc's Markdown parser reads a line starting with `>` as a blockquote; the following line (`/// fuses page-image hits...`) then reads as a lazy continuation of that blockquote without its own `>` marker, which `clippy::doc_lazy_continuation` flags. This is not a real documentation defect (the prose reads fine when rendered; there was never an intent to write a blockquote) — it is an accidental trigger from wrapping "greater than 0" as "`> 0`" right at a line boundary. One-line fix (not applied — audit-only pass): reword to avoid a bare `>` at the start of a wrapped line, e.g. "...when present and `w_visual` is greater than 0 the orchestrator embeds..." or move the whole clause so `>` never lands first on its line.

Aside from this single lint, `cargo clippy --all-targets` (no `-D warnings`) reports exactly two warnings total for the whole crate: this one and the `RerankClient::new` dead-code warning above. No other lint debt was found in this crate today.

### 5. ZDR — mixed: trace persistence is correctly ZDR-safe; the embedding cache is not, and this was live-reproduced

Live-tested by calling `/v1/retrieve` directly against the running container with a fresh, unique marker query and `zdr_mode` toggled, then diffing Postgres and Dragonfly state before/after.

**Correctly ZDR-safe (verified live):**
- **Trace persistence.** `orchestrator.rs:648-649` — when `zdr_mode == "ephemeral"`, `persist_trace` is never called at all; a synthetic `ephemeral-<uuid>` trace_id is returned instead. Confirmed: an ephemeral test call returned `"zdr_actions_applied":["ephemeral_no_trace_persist"]` and no new row appeared in `retrieval_runs`.
- **Visual RAG arm egress guard.** Confirmed via live server log on the same ephemeral call: `"visual query embed skipped; skipping visual arm","error":"ZDR query must not egress to the Cohere Embed v4 visual path"`.
- **Direct-Azure embedding egress guard.** `AzureOpenAiEmbeddingClient::embed_batch` hard-fails (`anyhow::bail!`) before any network call when `zdr=true` (unit-tested: `azure_openai_egress_guard_rejects_zdr`). Not the active backend in this deployment (see below), but correct if it ever is.
- **ZDR flag propagation to Model Plane.** The active backend here is `EmbeddingBackend::ModelPlane` (`EMBEDDING_PROVIDER=model_plane`, `MODEL_PLANE_EMBEDDING_PROVIDER=azure_openai` as a provider *hint* forwarded inside the gRPC request, not a local direct-Azure call). `build_request` faithfully carries `zdr` onto the wire (unit-tested: `model_plane_request_carries_zdr`), so retrieval-engine-rs does uphold its side of the contract — whether inference-core/Model Plane actually honors that flag is Model Plane's responsibility, out of scope for this pass.

**NOT ZDR-safe — confirmed live, still present, this is the substance behind the prior pass's "retrieval writes query embeddings in ephemeral mode anyway":**

The query-embedding cache (Dragonfly, key prefix `dpv2:embed:*`, `EMBED_TTL=3600` seconds, `src/cache/mod.rs`) is written unconditionally on every dense-retrieval cache miss —

```rust
// orchestrator.rs:239-251, inside `if route.dense { ... }`
let vec = if let Some(ref cache) = self.cache {
    if let Some(cached) = cache.get_embedding(&model_ver, &query_hash).await {
        cached
    } else {
        let vec = self.embedder.embed_query(&req.org_id, &req.query, embed_zdr).await?;
        cache.set_embedding(&model_ver, &query_hash, &vec).await;   // <-- unconditional, no zdr check
        vec
    }
} else { ... };
```

`embed_zdr`/`zdr_mode` is threaded into `embed_query` (so the *provider call* correctly signals ZDR), but nothing gates the *cache write* that follows it. Live reproduction, clean before/after diff:

```
before: dpv2:embed:* keys = 19 (dbsize 19)
POST /v1/retrieve  { "zdr_mode": "ephemeral", "query": "zdr-ephemeral-retest-<unique>" }
→ 200 OK, zdr_mode: "ephemeral", zdr_actions_applied: ["ephemeral_no_trace_persist"]
after:  dpv2:embed:* keys = 20 (dbsize 20)
diff: + dpv2:embed:model_plane:azure_openai:text-embedding-3-large:194b77cf...  (new key, blake3 hash of the marker query)
```

So a caller that explicitly requests zero data retention has that query's text-hash and resulting embedding vector persisted for up to an hour in a shared Dragonfly instance (the same instance backs `velion:gw:*` gateway keys — this is not an isolated ZDR-only store). This is a genuine, live-confirmed violation of CoreSystem's architecture rule that "Zero Data Retention must propagate through any content-persisting boundary" — the embedding cache is exactly such a boundary and today it does not honor `zdr_mode`. Note this is a narrower, more specific bug than the prior pass's one-line phrasing might suggest: trace persistence (the other obvious content-persisting boundary in this service) is correctly ZDR-gated; the embedding cache specifically is not.

**Fix shape** (not applied): gate the `cache.get_embedding`/`cache.set_embedding` calls on `!embed_zdr` (or equivalently skip the whole cache branch and always call `embed_query` directly when `embed_zdr` is true, same as the code already does when `self.cache` is `None`).

### 6. `cargo fmt` / `cargo clippy` / `cargo test` — run live today

- **`cargo fmt -- --check`**: drift present in 4 files — `src/embed/visual.rs:96`, `src/pipeline/orchestrator.rs:476`, `src/search/rerank.rs:128`, `src/search/timeline.rs:29,36`. All are long-line-wrap formatting differences (rustfmt wants a multi-line `format!(...)` call where the code has it on one line); no semantic difference. Confirms the baseline's "Rust fmt drift in retrieval-engine-rs" for this crate specifically.
- **`cargo clippy --all-targets -- -D warnings`**: fails to compile — exactly the one `doc_lazy_continuation` error above (item 4). Without `-D warnings`, exactly 2 warnings total (item 4 + item 3's dead code).
- **`cargo build --all-targets`**: succeeds, exit 0, 1 warning (dead code, item 3).
- **`cargo test --all-targets`**: **67 passed, 0 failed** across `src/lib.rs` unit tests (44), `tests/audit_log.rs` (2), `tests/cache_dragonfly.rs` (1), `tests/cross_org_isolation.rs` (5), `tests/grpc_integration.rs` (10), `tests/ownership_filter.rs` (1), `tests/zdr_behavior.rs` (4). `tests/pipeline_e2e.rs` has 3 tests marked `#[ignore = "requires docker-compose stack"]` (`happy_path`, `cache_invalidation_via_org_version`, `zdr_reject_filters_restricted`).

  **New finding**: the docker-compose stack *is* up and healthy, so these were forced on (`cargo test --test pipeline_e2e -- --ignored`, with `DATAPLANE_E2E_BASE=http://localhost:8014` / `DATAPLANE_DOCS_BASE=http://localhost:8010` corrected for the host-side port mapping). All 3 **fail live** with `401 Unauthorized` on the bulk-ingest step — the test file sends no auth headers at all (`grep` for `Authorization`/`x-api-key`/`x-internal` in `tests/pipeline_e2e.rs` returns nothing), so it predates documents-api's current auth hardening and has not been updated since. This refines the very old (2026-06-07) note that these cases were "still scaffolded... ZDR rejection and cache invalidation cases remain TODO" — that's now stale in a specific way: `zdr_reject_filters_restricted` and `cache_invalidation_via_org_version` are no longer TODO stubs, they exist as real test bodies today — but they cannot currently pass against the live, secured stack because nobody threaded credentials through them when the stack was hardened. Net effect is the same as the old note (no working e2e coverage for ZDR-reject or cache-invalidation), but for a different, more specific and easier-to-fix reason (missing auth headers in the test harness, not missing test logic).

### Net changes vs the prior (earlier-today) pass

- CONFIRMED, and materially sharpened: the strict-mode PolicyClient issue is not "does it fail closed or just warn" — it fails closed against a Control Plane route that doesn't exist, and that failure is then silently absorbed on the one auth path (API-key) that actual production traffic uses, making `CONTROL_PLANE_ENFORCEMENT` a no-op for the org axis today regardless of its value. It is NOT a no-op for a hypothetical Bearer-only caller, which would be hard-403'd.
- CONFIRMED unchanged: trace-endpoint empty-org bug, live-reproduced end to end (DB row → HTTP 404).
- CONFIRMED, and upgraded from "possibly-dead" to compiler-verified dead: `RerankClient::new`.
- CONFIRMED unchanged: the clippy doc-lazy-continuation issue at `orchestrator.rs:58`, with exact root cause identified (an accidentally-line-wrapped `> 0`).
- REFINED: the ZDR gap is real and live-reproduced, but narrower than "retrieval writes query embeddings in ephemeral mode anyway" might suggest — trace persistence and the visual-arm/direct-Azure egress guards are all correctly ZDR-gated; specifically the Dragonfly *embedding cache* is not, and that is the live-confirmed gap.
- NEW: the `pipeline_e2e.rs` ZDR/cache-invalidation tests exist (not stubs) but fail live against the current hardened stack for a mundane reason (no auth headers in the test).
- Out of scope for this service specifically: the Quarry-v2 `DataPlaneIngestRequest` cross-plane contract drift noted in the baseline concerns documents-api's bulk-ingest contract, not retrieval-engine-rs — this crate has no `DataPlaneIngestRequest` type at all (confirmed via grep). Re-verify that one against documents-api / Ingestion Plane directly.
- Not re-verified in this pass (out of the requested scope — see the Control Plane and cross-plane graph-index/data-quality/data-orchestrator findings in `plane-audit-2026-07-10.md`'s 2026-07-10 addendum instead): the `X-Org-ID`-only trust gap on `graph-index`/`data-quality`/`data-orchestrator`. For what it's worth, retrieval-engine-rs's own HTTP surface does NOT have this shape — every route under `authed` (`src/api/mod.rs:346-386`, including `get_trace`) sits behind `auth_middleware`, which requires either a matching internal API key or a verified JWT; a bare `X-Org-ID` header with no credential is rejected. The service's live bug is the wrong-org-string bug above (item 2), not a missing-credential bug.

## Historical 2026-06-07 snapshot (superseded)

`retrieval-engine-rs` is the main application-facing query surface in Data Plane v2. It owns hybrid retrieval, context packing, trace logging, graph and wiki search fusion, cache invalidation, and multiple transport wires.

- largest Data Plane v2 runtime service in this plane
- HTTP and gRPC surfaces are both live
- reads Postgres, Qdrant, Dragonfly cache, optional NATS, and optional Quickwit
- policy and JWKS support are present but still transitional in deployment shape (confirmed still true today — see item 1 above for exactly how transitional)
- current non-generated file count: 45 files under `src/` (44 `.rs` files + `src/bin/dlq_replay.rs`), plus 7 integration-test files under `tests/`

## Runtime Shape

- `src/main.rs` — Postgres, Qdrant, embedder, reranker, Dragonfly cache, optional NATS, JWKS init, policy client, visibility client, visual embedder (Cohere Embed v4), ColQwen visual reranker, HTTP server, gRPC server
- `src/api/mod.rs` — HTTP retrieval and diagnostic surface, `auth_middleware`, semantic-cache endpoints
- `src/grpc/*` — retrieval (`retrieval_svc.rs`), knowledge (`knowledge_svc.rs`), and document (`document_svc.rs`) gRPC services, plus `interceptor.rs` for gRPC-side JWT verification
- `src/pipeline/orchestrator.rs` — retrieval pipeline assembly: mode-mix resolution, engine routing, dense/sparse/wiki/visual fusion, rerank, ZDR enforcement, trace persistence
- `src/search/*` — dense, sparse, graph, wiki, rerank, timeline, colqwen (visual rerank), contradiction support
- `src/cache/*` — Redis-compatible (Dragonfly) cache, invalidation (org-version + NATS-driven), semantic cache
- `src/authz/*` — JWT verification + JWKS, org-membership policy client, per-user document visibility/ownership, taxonomy

Primary runtime surfaces:

- HTTP on `8014` (container `8004`)
- gRPC on `50062` (container `50052`)
- tool-style retrieval and knowledge search surfaces for upper planes

## API And Relationship Map

- `retrieval-engine-rs` -> Postgres — canonical metadata, retrieval traces, sparse search fallback, agent retrieval configs
- `retrieval-engine-rs` -> Qdrant — dense vector retrieval (main, wiki, and visual collections)
- `retrieval-engine-rs` -> Dragonfly — embedding cache, retrieval cache, semantic response cache (`SEMANTIC_CACHE_ENABLED=false` live today), cache-invalidation subscriber
- `retrieval-engine-rs` -> Quickwit — optional sparse backend (falls back to Postgres on failure)
- `retrieval-engine-rs` -> Model Plane (inference-core) — query embedding via gRPC (`ModelPlaneEmbeddingClient`), the active embedding path in this deployment
- `retrieval-engine-rs` -> Control Plane (user-service / org-core-service) — org-membership policy lookups; **currently broken** (item 1) — both endpoints reachable but 401, and the deny they produce is a no-op on the live gateway traffic path
- `retrieval-engine-rs` -> Control Plane (user-core :3012) — per-user document visibility/ownership grants (`HttpVisibilityClient`); always-on, decoupled from `CONTROL_PLANE_ENFORCEMENT`, working correctly (fails open to empty grants, not fail-open to full access)
- Frontend/Application/Model Plane -> `retrieval-engine-rs` — main retrieval and context assembly surface; velionv3 gateway always attaches `x-internal-api-key` + `x-user-id` (+ optional `data-plane` audience Bearer)

## Duplicates, Redundancies, And Inactive Surfaces

- sparse retrieval can run through Postgres or Quickwit, with fallback logic kept live for compatibility (`FallbackSparseBackend`, unit-tested)
- query-time embedding goes through Model Plane gRPC (`EMBEDDING_PROVIDER=model_plane` live today) with a direct-Azure fallback path (`AzureOpenAiEmbeddingClient`) that is fully implemented (including its own ZDR egress guard) but not the active backend
- gRPC `DocumentService` still exists for reads while write methods are deprecated and blocked by default
- `RerankClient::new` (item 3) is dead code superseded by `RerankClient::with_endpoint`

## Stubs, Placeholders, And Missing Connections

- `tests/pipeline_e2e.rs` is no longer scaffolded in the sense of missing test bodies — `happy_path`, `cache_invalidation_via_org_version`, and `zdr_reject_filters_restricted` all have real assertions — but all 3 currently fail live against the hardened stack because the test harness sends no auth headers (item 6)
- the org-membership `PolicyClient` HTTP contract (item 1) appears to have been written against a Control Plane route shape that Control Plane never shipped (or shipped differently) — this is a live cross-service contract drift, not a stub
- deeper Data Plane -> Model Plane embedding-path issue is worked around rather than fully solved (per repo-wide notes elsewhere); this pass did not re-litigate that beyond confirming the ModelPlane backend is the active one and correctly carries `zdr`

## API Design And Performance Notes

- combining HTTP and gRPC here is justified because this service is the shared retrieval facade for multiple planes
- keeping deprecated document writes blocked by default is correct; write ownership belongs in `documents-api-go`
- this is the most performance-sensitive Data Plane v2 runtime; cache invalidation via optional NATS means degraded modes are possible and should be watched closely
- reranker, direct embedding, sparse backend choice, and graph/wiki fusion all affect latency and cost
- the embedding cache (item 5) trades a real compliance gap for real latency wins (1-hour TTL, keyed by blake3 hash of query text + model version) — worth keeping the caching *mechanism*, but it must learn to skip itself for `zdr_mode=ephemeral`

## Current Doc Cleanup Read

Keep:

- `DATA_PLANE_DEEP_DIVE.md`
- `tests/e2e/README.md`
- this file (now current as of 2026-07-10)

Update or archive, not delete:

- `docs/gap-data.md` — useful history, but it still mixes closed gaps with live transition debt

## Historical bottom line (superseded)

`retrieval-engine-rs` remains powerful and real, and several things genuinely got fixed or were already correct on re-check (ephemeral-mode trace persistence, the visual-arm and direct-Azure ZDR egress guards, real RS256 JWT verification, a clean `cargo build`/67-passing-test baseline). But three concrete, live-verified problems remain the actual follow-up work, in priority order:

1. **Fix the org-membership `PolicyClient` HTTP contract** (item 1) — either point it at the Control Plane routes that actually exist, or add real internal-service auth to the calls it makes, and change `fetch_decision` to distinguish "backend said no" from "backend URL/auth is wrong" so `CONTROL_PLANE_ENFORCEMENT=strict` starts meaning something again. Today it is inert on the dominant traffic path and would hard-lock-out any Bearer-only caller the moment the internal-key fallback is retired.
2. **Fix the two hardcoded-empty-org-id call sites in the trace path** (item 2) — one-line-ish fix in `api/mod.rs::get_trace` and `timeline::replay_trace`, currently a 100%-broken read path masquerading as a security control.
3. **Gate the embedding cache on ZDR** (item 5) — `cache.get_embedding`/`cache.set_embedding` need to be skipped whenever `embed_zdr` is true, the same way the `self.cache.is_none()` branch already does it.

Smaller items worth a five-minute pass whenever someone is in this crate anyway: delete `RerankClient::new` (item 3), reword the `orchestrator.rs:58` doc comment to unblock a clean `-D warnings` clippy run (item 4), run `cargo fmt` on the 4 drifted files (item 6), and add an internal API key to `tests/pipeline_e2e.rs` so its 3 real (non-stub) e2e tests can actually run against the live stack again.

## 2026-07-11 secure-MVP delta (current)

The historical blockers above are superseded in source. Retrieval uses the
versioned Control decision contract, exact fail-closed ZDR enums, restrictive
embedding/cache/rerank egress guards, canonical auxiliary visibility, and
tenant+actor trace scope. Explicit-grant reads require the original verified
user bearer and are intentionally uncached, removing the stale-revocation/shared-
bus dependency. Provider errors retain status only. The library suite passes
75/75; current all-target/workspace verification is recorded centrally. The
isolated cryptographic authority fixture now proves the 200 authorized and 403
cross-tenant HTTP path; a real Auth Core/User Core authorized-user journey is
still required.
