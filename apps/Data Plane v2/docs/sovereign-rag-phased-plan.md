# Phased Plan: Data Plane v2 → Sovereign EU RAG (v3.0)

> Planning artifact. Builds on `visual-rag-integration-plan.md` (what's built+verified) and
> `sovereign-rag-blueprint-reconciliation.md` (blueprint↔reality map + the
> code-level isolation audit).

> **Verified 2026-07-10** (re-check of a 2026-07-10 audit pass; live containers +
> source read, not re-typed from the earlier pass): the "no code yet" framing above
> is stale — Phase 1 (GAP-1/GAP-2) and part of Phase 2 have shipped and were
> re-confirmed live today. Two corrections of substance vs. what's written below:
> 1. **Phase 2 "Remaining" list is out of date.** The `BrowserDriver` render hook
>    (`quarry-runtime/src/page_renderer.rs`), the `image/png` serve route
>    (`quarry-edge/src/resource_routes.rs`), and the `page_images.created` emission
>    (`quarry-runtime/src/page_image.rs`) all exist in Ingestion Plane source today.
>    Still genuinely open, confirmed by grep: no `page_images.deleted` **producer**
>    exists in DP2's document-erasure cascade (`embedding-engine-rs/src/stream/mod.rs`'s
>    `documents.deleted` handler only purges Qdrant vectors — it does not touch the CAS
>    or emit `page_images.deleted`), so the GDPR/CAS-erasure gap called out in Phase 2
>    is real and still open. `W_VISUAL` also still defaults to `0` (shadow) in
>    `docker-compose.yml` — the arm is wired but not live in fusion.
> 2. **Phase 3 shipped differently than planned, not "not started."** There is no
>    ColQwen2 multivector Qdrant collection and no swap of `provider/visual.rs` /
>    `embed/visual.rs` off Embed v4 — both still call Cohere Embed v4 for visual
>    embeddings exactly as before. Instead, a separate, additive
>    **`services/colqwen-reranker`** Python service and a
>    `retrieval-engine-rs/src/search/colqwen.rs` client were built: ColQwen reranks
>    Embed v4's visual top-K via MaxSim (`colqwen_endpoint_url` / `COLQWEN_ENDPOINT_URL`,
>    off by default, non-fatal on failure). This is a materially lighter architecture
>    than the "replace the visual embedder" plan below — treat Phase 3's steps as
>    superseded by this reranker-only shape unless/until the full multivector swap is
>    separately decided.
> Phase 1's own gRPC caveat also re-confirmed live: `get_sources`/`get_chunks`/
> `pack_context` in `retrieval_svc.rs` still bind `req.org_id` straight into SQL with no
> verified-context check, and no RLS migration (`ENABLE ROW LEVEL SECURITY`) exists yet.

## Requirements (restated)

1. Evolve the **current** Data Plane v2 toward the v3.0 sovereign-EU RAG blueprint.
2. **Visual arm = self-hosted ColQwen2** (multivector / MaxSim in Qdrant) for page
   layouts/blueprints/charts; **Cohere Embed v4 = dense text** (multilingual chunks,
   wiki, metadata). This *changes* the visual arm we built (Embed-v4-on-images,
   single-vector) → ColQwen2 multivector.
3. Audit & close **user / org / tenant isolation** gaps.
4. Reach the blueprint's retrieval shape: graph + visual + sparse + keyword (+ dense,
   which the blueprint omits but is our highest-recall arm) → RRF → Cohere Rerank.

## Current state (verified this session)

Visual RAG core built + passing real `cargo test`: authz-safe semantic cache (PR-A),
Embed v4 provider/consumer/visual-arm (PR-B/C/D), structural chunker (PR-E). Ships dark
(`W_VISUAL=0`, no producer). Stores: Qdrant (vectors), Postgres (canonical + graph +
ownership), Quickwit (BM25 on MinIO), Dragonfly (exact KV). Fusion = RRF + Cohere rerank.

## Isolation audit — summary

| Path | org | user | tenant |
|---|---|---|---|
| Dense / Sparse(PG+Quickwit) / Wiki-ANN / **Visual** | ✅ forced | ✅ fused gate | ❌ none |
| Graph, Wiki-kw, Contradictions, Timeline (aux HTTP) | ⚠️ **body-trusted org** | ❌/n/a | ❌ |
| Semantic cache (HTTP) | ⚠️ **body-trusted org** (scope ✅) | ✅ scope_key | ❌ |
| gRPC Retrieve/Stream | ⚠️ **no DP-side org binding** | ✅ + admin off | ❌ |

**Verdict:** strong org + per-user on the main `/v1/retrieve` path; **no tenant tier,
no RLS** (single-layer); **two HIGH body-org-trust gaps** (aux HTTP handlers + gRPC).

## Phases

### Phase 1 — Isolation hardening *(do first; live security + gates multi-tenant trust)*
- **Goal:** every org boundary enforced inside DP2, not delegated to the gateway; add
  defense-in-depth.
- **Steps:**
  1. **GAP-1:** make the aux HTTP handlers (`retrieve_graph`, `retrieve_wiki`,
     `retrieve_contradictions`, `retrieve_timeline`, `semantic_cache_search/store`) take
     `Extension<AuthContext>` and **pin `org_id` from the verified context** (mirror
     `apply_to_request` used by `/v1/retrieve`, `authz/context.rs:173`). This also
     completes PR-A (the cache's org becomes spoof-proof, not just its scope).
  2. **GAP-2:** bind org in the **gRPC** path — assert request `org_id` against the
     JWT claim (when JWT) and require an explicit org on the internal-key path; reject
     mismatch. Defense-in-depth so a valid credential can't read an arbitrary org.
  3. **RLS (defense-in-depth):** `ENABLE ROW LEVEL SECURITY` + `CREATE POLICY` on
     `documents`, `knowledge_units`, `wiki_pages`, graph tables, keyed on a per-request
     GUC (`SET LOCAL app.org_id`). Converts any forgotten `WHERE org_id` from a leak into
     empty results. (Additive migration; app already sets org everywhere.)
- **Files:** `api/mod.rs`, `authz/context.rs`, `grpc/interceptor.rs`,
  `grpc/retrieval_svc.rs`, `infra/postgres/migrations/*`.
- **Risk:** MEDIUM (RLS needs the GUC set on every pooled connection — test carefully).
  **Complexity:** MEDIUM. **Depends on:** nothing.
- **Status:** **GAP-1 ✅** — six aux/cache handlers (`retrieve_graph/wiki/contradictions/timeline`,
  `semantic_cache_search/store`) now pin org from the verified `AuthContext` via a tested
  `pin_org_from_ctx` helper (`authz/context.rs`); a JWT for org A can no longer read org B by
  putting B in the body. **GAP-2 ✅** — `verify_jwt_claims` + `verified_org_from_metadata`
  added; `retrieve` + `retrieve_stream` reject (`permission_denied`) when a JWT principal's org
  ≠ the request body org (API-key path stays gateway-trusted by design).
  **RLS — foundation ✅ (2026-08-09), rollout in progress.** No longer deferred: the
  "needs transaction-pinned connections" blocker is solved and proven. See the
  dedicated Phase 1-RLS entry below. **gRPC `get_sources`/`get_chunks`/
  `pack_context` still take body org** (by-id, SQL-org-filtered) — extend the same helper next.

#### Phase 1 RLS — org isolation enforced by the database (2026-08-09)

- **Migration:** `infra/postgres/migrations/20260809120000_org_rls_isolation.sql` —
  creates the `dataplane_app` role (NOLOGIN / NOSUPERUSER / **NOBYPASSRLS**) and enables
  RLS + a strict fail-closed policy
  (`org_id = current_setting('app.current_org', true)`, no fail-open branch) on **all 35
  tables in `public` carrying an `org_id`**, verified against the live schema rather than
  taken from this plan's older 4-category sketch. Ported from Control Plane org-core's
  audited design (its migrations 009 + 013), skipping org-core's intermediate fail-open
  phase because we already know the end state.
- **Why enabling all 35 at once was safe:** every DPv2 service connects as `dataplane`,
  which is SUPERUSER with BYPASSRLS, so policies are inert on a normal connection and no
  existing query changed behaviour. Enforcement is opt-in per transaction. The risk lives
  in **adoption** (which call sites get wrapped), which is per-service and reviewable one
  site at a time — not in enablement.
- **Helpers (both proven against the live database, not just unit-tested):**
  `services/pg-org-scope-rs` (Rust, `begin_org_scoped` returning a `Transaction` — a
  transaction handle rather than org-core's Go-style closure, because the HRTB async-closure
  translation is unreadable at every call site and sqlx already rolls back on drop) and
  `shared/go/orgscope` (Go, `WithOrgScope(ctx, pool, orgID, fn)` — a direct port of
  org-core's proven closure shape, for the 4 Go services' later rollout).
- **Live verification (real data, no fixtures, all rolled back):** scoped to a foreign org,
  a `SELECT` over `documents`/`knowledge_units`/`graph_entities` returned **0**, `UPDATE`
  affected **0**, `DELETE` affected **0**; scoped to the real org it correctly returned
  10 documents / 51 knowledge units; a cross-org `INSERT` was rejected by `WITH CHECK`
  (`new row violates row-level security policy`); and a scoped transaction that forgot to
  set the GUC saw **0** (fail-closed). The strongest evidence was incidental: `graph_entities`
  showed **748 scoped vs 750 unscoped** — 2 rows genuinely belong to a leftover
  `org-e2e-meili-test` org, so the filter was demonstrated discriminating on real multi-org
  data rather than trivially returning everything.
- **Pilot adoption ✅ — `meilisearch-adapter-rs` and `embedding-engine-rs`**, both rebuilt,
  redeployed, healthy, and proven end-to-end through the live pipeline: a real re-announce
  drove meilisearch-adapter's now-scoped read to return all **10** real chunks (a broken or
  over-restrictive scope would have returned 0), and a real 1-chunk re-embed drove
  embedding-engine's now-scoped chunk-text lookup to `batch embedded count=1` (rather than
  `chunk not found, skipping`).
- **⚠ Key finding that reshapes the remaining rollout estimate: much of DPv2 is multi-org
  by design, and must NOT be wrapped.** `embedding-engine-rs::process_batch` drains one
  unpartitioned JetStream buffer that deliberately mixes orgs — the code itself proves it
  (`embed_items_by_org`, and `cost_groups` keyed by `(org_id, user_id, zdr)`) — and
  `mark_units_done` updates by `knowledge_id = ANY($1)` with no org predicate at all.
  Wrapping those in a single-org transaction would silently drop every other org's rows
  from the update. So of embedding-engine's 9 query sites only the 2 genuinely per-message,
  single-org lookups were scoped; the batch writes stay on the unscoped pool, documented in
  place. Same for meilisearch-adapter: 2 of 3 scoped, the third being a `SELECT 1` liveness
  probe that touches no org table. **Adoption is therefore a per-call-site classification
  exercise, not a mechanical find-and-wrap** — the same judgement org-core's audit applied
  to its admin/GDPR exceptions.
- **Remaining rollout (not started, tracked here):** 8 services — `retrieval-engine-rs` (66
  sites), `graph-index-rs` (56), `index-engine-rs` (48), `quickwit-adapter-rs` (26), and the
  4 Go services `wiki-store-go` (105), `documents-api-go` (55), `data-quality-go` (42),
  `data-orchestrator-go` (35). Roughly 430 call sites, each needing the scoped-vs-legitimately-
  unscoped judgement above. Both helpers exist and are proven, so this is now bounded,
  incremental work rather than a design problem.

### Phase 2 — Light up the visual arm: MinIO CAS + page-image producer *(PR-F)*
- **Goal:** something actually emits `dataplane.page_images.created` so the (built)
  visual consumer receives pages. Identical work regardless of embedder.
- **Steps:** content-addressable MinIO bucket for raw binaries + rendered PNGs; an
  Ingestion-Plane (or `documents-api-go`) renderer that rasterizes pages → CAS → emits the
  event (`{document_id, org_id, page_no, image_url, content_hash, zdr}`); migrations for
  image-ref rows.
- **Files:** Ingestion Plane / `documents-api-go`, `infra/` (MinIO), migrations.
- **Risk:** MEDIUM (cross-plane, partly non-Rust, needs live stack to E2E). **Complexity:**
  MEDIUM. **Depends on:** Phase 1 (so new image rows are org/RLS-isolated from day one).
- **Investigation outcome (workflow, 2026-06-22) — NOT ready to build as one push.** Reusable
  primitives verified: Quarry already has a Chromium PNG screenshot path (`quarry-browser/
  chromiumoxide.rs`) + an `aws-sdk-s3` artifact store; the DP2 Quickwit MinIO can host a new
  `dataplane-cas` bucket; the consumer contract is pinned. Recommended producer = a **new
  Ingestion-Plane Rust worker** reusing those crates (NOT documents-api-go — text-only, no S3).
  **Four blockers found:**
  1. **No raw bytes exist.** DP2 stores only extracted TEXT — for PDF/office there is no
     original binary to rasterize. Where originals come from is a precondition, not a detail.
  2. **ZDR-before-CAS.** Raw bytes + PNGs would be written to the CAS *before* the consumer
     drops `zdr=true` → restricted content persisted in object storage (GDPR/ZDR boundary
     violation). MUST skip rasterization+CAS for `zdr=true` at the producer; and `zdr` is NOT
     on `documents.indexed`, so the worker must fetch the classification before writing.
  3. **No CAS erasure hook.** `.deleted` purges only Qdrant; nothing deletes raw+PNG objects
     from the CAS on delete/DSAR → GDPR gap. Must extend the erasure cascade.
  4. **Silent poison-drop.** No DLQ; any payload drift (e.g. the `image_cas_key` bug above)
     loses page images invisibly → contract-pinned producer tests + a live round-trip required.
  Also: "reuse Quarry S3Store" is overstated (its `object_key` is run-based, hardcoded → the
  doc/page CAS layout needs a NEW store impl); MinIO needs `force_path_style` on the SDK builder
  (not just env); cross-plane NATS/MinIO/serve reachability is unverified.
- **Recommended slice:** ship the **web/HTML path FIRST** (reuse Quarry's browser render),
  with ZDR-skip + CAS erasure built in — a real, non-mock, shippable vertical. Defer PDF/office
  until the raw-byte-source decision lands. Full blueprint + critique: workflow `wf_79b9f73e-56b`.
- **Design workflow (`wf_28aa1805-c11`) caught two would-be-broken premises** before any code:
  1. **Screenshot is NOT on `Driver`.** The scrape-hook capture (`self.driver.screenshot()`)
     does not compile — `Driver` (`driver.rs`) has only fetch/kind/tls; `screenshot(&self,
     &BrowserSession, full_page)` lives on the **`BrowserDriver`** trait (quarry_browser),
     reachable only with a `BrowserSession`. → **Decision: capture via a `BrowserDriver`
     session render** (acquire→goto→screenshot→release), the same pattern as the P7
     `agent_routes.rs` agent_driver. Not a call on `Arc<dyn Driver>`.
  2. **Erasure can't add a 2nd consumer.** `DATAPLANE_PAGE_IMAGES` is a **WorkQueue** stream —
     a competing `quarry-edge` deleter would *steal* deletes from the embedding-engine. →
     **Decision: extend the embedding-engine consumer's existing `page_images.deleted` handler
     to also purge the CAS** (single consumer, no stealing), and wire a real `page_images.deleted`
     **producer** into DP2's document-erasure cascade (none exists today).
- **Decided architecture:** capture = `BrowserDriver` session render; CAS = new `CasStore`
  (direct `aws_sdk_s3` `put_object` + **`force_path_style(true)`** — S3Store can't express the
  doc/page key and omits path-style); event contract = `image_url` (NOT `image_cas_key`),
  `page_no` integer, JetStream publish to **dpv2-nats** (separate broker — needs
  `QUARRY_EDGE__DATAPLANE_NATS_URL`); serve route returns **`image/png`** (consumer filters
  `image/*`); ZDR-before-CAS enforced structurally + explicit early-return.
- **Landed + verified this turn (offline, real `cargo test`):**
  - `quarry-runtime/src/page_image.rs` — wire-contract structs + emit fns + a byte-compatibility
    test round-tripping producer JSON through a mirror of the consumer struct (locks
    `image_url`/integer-`page_no`/no-`image_cas_key`; kills the silent poison-drop risk). 4/4.
  - `quarry-runtime/src/cas_store.rs` — `CasStore`: `put_page_png` (content-addressable key),
    `get_object` (serve), `delete_by_doc` (erasure), `force_path_style(true)` for MinIO. The
    `aws_sdk_s3` calls compile-verified; key/prefix layout unit-tested. 3/3.
- **Remaining — implementable but needs the live stack to E2E-verify:** the `BrowserDriver`
  render hook (acquire→goto→screenshot→release), the `image/png` serve route in quarry-edge,
  consumer-side CAS erasure + a `page_images.deleted` producer in DP2's cascade, the additive
  migration, compose env (`QUARRY_EDGE__DATAPLANE_NATS_URL`/`CAS_BUCKET`/AWS_*), the
  `dataplane-cas` bucket, and the cross-plane NATS/serve/MinIO reachability (all runtime-
  unverified per the critique). These are the live-stack finish line for PR-F.
  **(Verified 2026-07-10: the render hook, serve route, and `page_images.created` emission
  now exist in source — `page_renderer.rs`, `resource_routes.rs`, `page_image.rs`. Still
  open: no `page_images.deleted` producer in DP2's erasure cascade, so CAS objects are not
  purged on document delete/DSAR. See the verified note at the top of this file.)**
- **(b) live-stack progress (2026-06-22):** DP2 infra up; **compose fixed** — `dpv2-minio`
  joined `inter-plane-bus` + `minio-init` now creates `dataplane-cas` (verified created).
  **Cross-plane reachability PROVEN live** (the critique's #1 unverified risk): from
  `quarry-edge`, `dpv2-minio:9000` and `dpv2-nats:8222` are both reachable over
  `inter-plane-bus`. Remaining for full E2E: cold Rust builds of `embedding-engine` (consumer) +
  the producer wiring (render hook + `image/png` serve route) in quarry-edge + the migration;
  AND a deployed visual embedder (ColQwen2 [decided, not yet built] or Embed v4 w/ Azure-EU
  creds) for the final embed→Qdrant→retrieve hop. The producer→event→consumer→image-fetch
  chain is embedder-independent and provable first.

### Phase 3 — Swap visual embedder: Embed-v4-images → **ColQwen2 multivector**
> **Verified 2026-07-10: superseded by a lighter shape.** What actually shipped is an
> additive `services/colqwen-reranker` + `search/colqwen.rs` MaxSim **reranker** over
> Embed v4's visual top-K (off by default via `COLQWEN_ENDPOINT_URL`) — NOT the full
> multivector-Qdrant swap this section describes. `provider/visual.rs` and
> `embed/visual.rs` still call Cohere Embed v4 unchanged. Read the steps below as the
> not-yet-decided "full swap" option, distinct from the reranker that already exists.
> **Step 4's dense-text sub-decision is now resolved — see D-B above (2026-08-08):
> migrate to Embed v4.** That half is independent of the ColQwen2 multivector swap (separate
> call sites, `provider/text.rs`-equivalent vs `provider/visual.rs`) and does not need to wait
> on this phase's GPU/multivector work to proceed.
- **Goal:** the decided architecture — ColQwen2 (self-hosted) for visual, Embed v4 for text.
- **Steps:**
  1. Stand up **ColQwen2 serving** (vLLM/TGI on EU GPU) — infra/Model-Plane.
  2. **Qdrant multivector collection** for `dataplane_page_images` with `MAX_SIM`
     comparator + token-pooling + (optional) binary quantization + a mean-pooled vector for
     two-stage prefetch. *(This is the multivector work the Embed-v4 path let us skip.)*
  3. New ColQwen2 embedding path: index (page image → patch matrix) + query (text → token
     multivectors); replace `provider/visual.rs` (embedding-engine) and
     `embed/visual.rs` (retrieval) Embed-v4 calls for the visual arm.
  4. Generalize the orchestrator's single-vector assumption for the visual arm (MaxSim
     prefetch→rerank). Dense text: **migrate to Embed v4 (decided, D-B)** — tracked as its
     own slice, see the new phase below.
- **Files:** `embedding-engine-rs/provider/*`, `qdrant_writer`, `image_consumer.rs`;
  `retrieval-engine-rs/embed/visual.rs`, `search/`, `pipeline/orchestrator.rs`.
- **Risk:** **HIGH** (GPU infra, multivector storage ~1000× single-vector, MaxSim serving).
  **Complexity:** HIGH. **Depends on:** Phase 2 + GPU sizing decision.

### Phase 3b — Dense text embedder: migrate to Embed v4 *(new, split out of Phase 3 Step 4 — D-B decided 2026-08-08)*
- **Goal:** close the blueprint gap (line 40) for the arm that actually matters most today —
  dense text is the highest-recall arm on the real, live-Norwegian corpus this session just
  backfilled (see the durability plan's 2026-08-08 backfill entry). Deliberately split out of
  Phase 3 because it needs none of that phase's GPU/multivector work — Embed v4's text
  embedding is a plain single-vector call, same shape as the current OpenAI call it replaces.
- **Steps:**
  1. Add a text embedding path to the existing Cohere client (`embedding-engine-rs/provider/
     visual.rs` already holds Embed v4 credentials/client setup for images — add the sibling
     text-input call rather than a new provider from scratch).
  2. New Qdrant collection for text at Embed v4's output dimension (distinct from the current
     `text-embedding-3-large`-sized `dataplane_knowledge` collection — dimensions differ, so
     this is a new collection, not a resize).
  3. Re-embed the real corpus into it (the same `knowledge_units` rows already re-announced
     for the graph/keyword backfill can be replayed the same way once the new provider is
     live) and cut `retrieval-engine-rs`'s dense arm over.
  4. Decide and document the cutover: dual-run both collections behind a flag first, or a
     hard swap once the new collection's recall is spot-checked against real Norwegian
     queries.
- **Files:** `embedding-engine-rs/provider/*`, `qdrant_writer` (new collection config),
  `retrieval-engine-rs/embed/dense.rs`(-equivalent), `docker-compose.yml` (Qdrant collection
  env), `docs/actions-surface-operations.md` if the provider surface is contract-tracked.
- **Risk:** MEDIUM (re-embed cost + a cutover window; no GPU/infra blocker — Embed v4 access
  already exists in this stack). **Complexity:** LOW-MEDIUM. **Depends on:** nothing new;
  buildable now.
- **Status: ✅ done (2026-08-08).** All four steps shipped as a hard cutover (not a dual-run
  flag — the corpus was small enough that a flag would have been pure overhead): new
  `EmbeddingBackend::Cohere` variant added to both `embedding-engine-rs/provider/mod.rs` and
  its independently-duplicated twin `retrieval-engine-rs/embed/mod.rs` (`input_type: "document"`
  vs `"query"` — Embed v4 asymmetrically optimizes each side), new `dataplane_knowledge_embedv4`
  Qdrant collection (1536-dim, int8-quantized), both services' `EMBEDDING_PROVIDER` defaults
  flipped to `cohere`. Re-embedding the existing corpus needed a new tool
  (`index-engine-rs/src/bin/reembed_switch.rs`, binary `reembed-switch`) rather than the
  `backfill-reannounce` tool the durability plan's 2026-08-08 entry built — that tool
  deliberately bypasses the embedding pipeline (right for re-announcing already-correct
  vectors), which is exactly wrong here since a provider switch's entire point is a real
  re-embed into a new vector space; `reembed-switch` replays `dataplane.knowledge.units.created`
  instead, mirroring `reconcile.rs`'s signing identity but targeting already-`'done'` units
  without mutating status (a distinct tool, not a mode of the failed-only reconciler).
  **A real external constraint surfaced live, not assumed**: the Cohere Embed v4 Azure AI
  Foundry deployment is on a free tier capped at 10 requests/60s; publishing all 51 units'
  re-embed events back-to-back exceeded it after ~14 succeeded, producing a burst of `429
  RateLimitReached` errors. Confirmed self-healing, not manually forced: JetStream's existing
  redelivery (D18/D19 machinery from the durability plan) retried the failed batches once the
  window cleared, with zero intervention — all 51/51 landed. **Verified end-to-end with a real
  query**, not just a point count: embedded the Norwegian query "hva koster SuperOffice CRM?"
  via live Cohere Embed v4 (`input_type: query`) and searched the new collection directly —
  top 3 hits all correctly resolved to the real SuperOffice pricing document
  (`630a9cc9-847a-44f5-8f57-4dc63353dcf8`), with the #2 hit's literal chunk text reading
  "SUPEROFFICE CRM CLOUD SUBSCRIPTION PRICE LIST EFFECTIVE OCTO[BER...]" — a correct semantic
  match, not a coincidence. The old `dataplane_knowledge` (3072-dim, text-embedding-3-large)
  collection was left in place, untouched, as a rollback path — deleting it was out of scope.

### Phase 4 — 4-arm retrieval parity (graph-in-fusion + Meilisearch)
- **Goal:** match the blueprint's arm set.
- **Steps:** fold **graph** into the fused RRF path (currently a separate endpoint — needs a
  shape adapter from entities/claims → candidates); add **Meilisearch** keyword arm
  (typo-tolerant IDs/codes) + a `w_keyword` RRF pass; keep dense.
- **Files:** `pipeline/orchestrator.rs`, `pipeline/types.rs`, `config.rs`, new
  `search/keyword.rs`, infra (Meilisearch).
- **Risk:** MEDIUM. **Complexity:** MEDIUM. **Depends on:** Phase 1 (new arm must be org-gated).
- **Status:** **graph-in-fusion ✅** — the GraphRAG program folded the graph arm
  into the fused RRF path: `search/graph.rs::graph_arm_candidates` is the
  entities→candidates shape adapter, `arm_graph` joins the concurrent
  `tokio::join!` fan-out, and `fuse_arms` applies `w_graph` (closes gap-data
  §16.1.1). It also added a Neo4j-backed native multi-hop read-model
  (`POST /v1/graph/traverse`) behind `NEO4J_ENABLED`. See
  `docs/graphrag-neo4j-plan.md`. **Meilisearch keyword arm ✅ (2026-08-07)** —
  new `meilisearch-adapter-rs` (write side, durable JetStream consumers on the
  knowledge/document lifecycle) + `search/keyword.rs` (query side,
  org-filtered, typo-tolerant) fused last in `fuse_arms` via `w_keyword`
  (default 0.05, same calibration `w_visual` launched with). Live-verified:
  real signed events → real adapter → real Meilisearch, typo-tolerant match,
  cross-org filter proven empty, erasure proven via a real
  `dataplane.documents.deleted` event, and a real fused-RRF score through the
  unmodified `reciprocal_rank_fusion`. Full account:
  `docs/retrieval-quality-and-durability-plan-2026-08-05.md`'s 2026-08-07
  "Phase 4: Meilisearch keyword arm" entry (includes what was deliberately
  left out — no wiki/source-object indexing, no admin rebuild API, no
  separate GDPR-fanout consumer, no scoped search-only key).

### Phase 5 — Erasure completeness for the new stores (GDPR Art. 17)
- **Goal:** the purge cascade covers every store the blueprint adds.
- **Steps:** extend the Art.17 cascade to **MinIO CAS** (raw + page images) and
  **Meilisearch**; verify Qdrant visual-multivector purge, Quickwit segment prune, and
  Dragonfly/semantic-cache eviction end-to-end. (Page-image Qdrant purge already wired in PR-D.)
  **Meilisearch's slice of this ✅ (2026-08-07)** — `dataplane.documents.deleted`
  wired to purge the keyword index (live-verified with a real signed event);
  MinIO CAS, Qdrant visual-multivector, Quickwit segment prune, and
  Dragonfly/semantic-cache eviction remain as stated (MinIO CAS closed
  separately the same day — see the durability plan's adjacent entry).
- **Risk:** MEDIUM. **Complexity:** LOW-MEDIUM. **Depends on:** Phases 2-4.

### Phase 6 — Model Plane track *(separate plane — coordinate, don't build in DP)*
- Local **router classifier** (replaces "Not Diamond"; sovereign <10ms) + routing policy.
- **Command R+** and **local Llama 3.3 (vLLM)** as **inference-core providers**.
- Multi-turn session stays in **session-core** (no second Redis).
- **Risk:** MEDIUM. **Depends on:** Phase 3 GPU (shares the Llama fleet).

### Phase 7 — Sovereignty/infra *(if self-host confirmed)*
- Bare-metal EU GPU provisioning (ColQwen2 + Llama), Azure-EU pinning (Sweden/Germany)
  + ZDR contractual for Cohere Command R+/Rerank/Embed v4. GPU sizing = blueprint deep-dive #1.

## Cross-cutting risks
- **Sovereignty consistency:** Command R+/Rerank/Embed v4 on Azure-EU still egress text +
  answers; self-hosting only ColQwen2 narrows sovereignty to "images never leave." If full
  sovereignty is required, Phase 6 must also move *reasoning* to local Llama.
- **Multivector cost** (Phase 3): storage + MaxSim compute; mitigate with pooling + quant.
- **RLS rollout** (Phase 1): the GUC must be set per pooled connection or queries return
  empty — needs a connection-acquire hook + tests.

## Decisions (confirmed 2026-08-08)

All three were open questions as of the last pass through this plan; the user has now
decided all three. Recorded here with rationale so the "why" survives independent of who
implements each — none of the three are built yet as of this entry.

- **D-A (account, formerly "tenant") — DECIDED and scoped 2026-08-08: a grouping axis
  ABOVE org, with per-org opt-in grants, not a structural takeover.** Genuinely greenfield —
  no precedent anywhere: `organizations` (org-core) has no parent/grouping field, and every
  existing "tenant" reference in this codebase means something else entirely
  (`org_tenant_links.microsoft_tenant_id` is a *Microsoft 365 tenant per org*, for SSO domain
  verification — an unrelated, already-shipped concept; org-core's RLS migrations use
  "tenant isolation" as loose phrasing for org isolation, same pattern found everywhere else
  this session). **Naming: call it "account," not "tenant"** — avoids colliding with
  `microsoft_tenant_id` right next to it in the same table.
  **Scope, as decided (not yet built):**
  1. An org joining an account is **opt-in per grant, decided at org-creation (or later)**,
     not an all-or-nothing parent-child relationship. Two independent grants per org:
     - **Data-access grant**: if granted, the account (its admin principal) can read this
       org's data in Data Plane v2 directly. If NOT granted, the account has no standing
       access — it must go through a **request/approval flow**, the same shape as this
       codebase's existing private-until-shared `resource_grants` authorization pattern
       (grant rows checked as a retrieval post-filter, never a blanket bypass). This is the
       actual reason D-A is NOT "org is the tenant boundary, full stop": an org's isolation
       is still the default, but it is no longer unconditional — an org can widen it by
       choice.
     - **Billing-consolidation grant**: if granted, this org's billing rolls into the
       account's consolidated invoice/plan (`billing-core` — `billing_accounts.org_id` is
       currently a PRIMARY KEY with no grouping concept, so this is real new billing-core
       work, not a label). If NOT granted, the org stays billed exactly as today.
  2. **Sequencing: org-level RLS must be validated live FIRST.**
     **✅ Control Plane side validated 2026-08-09 — solid, one known structural caveat.**
     What's actually live today is NOT migration 008 (that one really is inert — superseded,
     never the thing enforcing anything) but **009 + 013**, confirmed applied
     (`schema_migrations` ledger) and confirmed enforcing on the real running database: all
     14 org-scoped tables show `relrowsecurity=true`, the `org_core_app` role exists exactly
     as the migration describes (NOLOGIN), and `internal/database/database.go`'s
     `WithOrgScope` drops every scoped transaction to that role via `SET LOCAL ROLE` before
     the caller's query runs. A full agent-driven code audit of every exported DB-touching
     function in `internal/org/repository.go` (1400 lines), `internal/rbac/repository.go`,
     and `internal/org/deletion_ledger.go` found **zero unexplained unscoped paths** — every
     function is either routed through `WithOrgScope`, or is one of the migration's own
     documented exceptions (admin list-all, GDPR erasure, lookup-by-secondary-key) *and* is
     independently gated by real, code-verified authorization (HMAC-signed service
     delegation with nonce+timestamp replay protection, owner/platform-admin membership
     checks) — not just a comment claiming it's fine. Then proven empirically, not just read:
     opened the exact transaction sequence `WithOrgScope` runs, scoped as the one real org in
     the database, and confirmed live — a cross-org `SELECT` returns zero rows, a cross-org
     `UPDATE` affects zero rows, the org can still see itself, and dropping the `SET LOCAL
     ROLE` (the unscoped/admin path) correctly sees across both. **One real gap found, not
     glossed over**: `gdpr_hard_delete_organization`/`soft_delete_organization`/
     `purge_old_deleted_organizations` (`migrations/003_gdpr_hard_delete.up.sql`) are
     `SECURITY DEFINER` — their bodies always run as the *definer's* privileges, so `SET
     LOCAL ROLE` has zero effect inside them regardless of how the caller reached them. RLS
     provides no backstop on this path; it relies entirely on the proc's own parameterized
     `WHERE org_id = ...` (verified correct on every statement, today) with no second layer
     behind it. Also noted: `org_quotas`/`org_billing`/`org_compliance` have zero Go
     references anywhere in org-core (dead schema from migration 002, not a live risk —
     confirmed no other Control Plane service holds a connection string to the `org_core`
     database; org-core is the only consumer).
     **Data Plane v2 side — foundation ✅ built and proven 2026-08-09, rollout in progress.**
     When first checked this was not merely unvalidated but entirely absent: zero RLS
     migrations, zero transaction-pinning pattern, zero `WithOrgScope`-equivalent anywhere.
     That is the actual blocker for D-A's data-access-grant feature specifically — the
     org-core validation above covers Control Plane's own identity/admin data
     (organizations, members, entitlements) and says nothing about the *documents and
     knowledge* an account-level grant would expose, which live entirely in DPv2. It has
     since been built: migration + both language helpers + 2 pilot services, all
     live-verified (see the Phase 1 RLS entry above). **What remains before D-A can start**
     is the 8-service rollout listed there — in particular `retrieval-engine-rs`, since that
     is the service an account-scoped read would actually flow through, and it is not yet
     adopted. D-A's grant check is then a second predicate layered on a working org-level
     one, which is the order this sequencing decision was asking for.
  3. Data Plane v2's current JWT `Claims`/`AuthContext` (`retrieval-engine-rs/src/authz/
     context.rs`) carry only `org_id` — no account concept anywhere in the propagation
     chain yet. An account-scoped read needs a new claim (e.g. `account_id` +
     `account_data_access: bool`) minted by auth-core once org-core can resolve it, which
     depends on org-core having the account/grant schema in the first place.
  - **Not started**: no schema, no migration, no claim, no grant-check code. This entry is
    the scope, not the implementation — org-level RLS validation is the concrete next
    unblocking step, tracked as its own work above.
- **D-B (text embedder) — DECIDED: migrate to Cohere Embed v4 for dense text.** This
  resolves Phase 3 Step 4's sub-decision below in favor of the original blueprint
  requirement (line 40: "Cohere Embed v4 = dense text (multilingual chunks...)"). Driven by
  a live, confirmed fact rather than the blueprint's abstract multilingual framing: the
  real production corpus backfilled into this stack on 2026-08-07/08 is Norwegian-first
  (`Serviceavtale`, `Leievtale`, `Tilbud på Migrering...`), and D2 (durability plan)
  independently already flags English-tuned assumptions as a live P0 gap. Embed v4 is
  already an integrated provider in this exact codebase for the visual arm
  (`provider/visual.rs`, `embed/visual.rs`), so wiring it for text reuses an existing
  credential/client path rather than adding a new one. This sub-decision does NOT, by
  itself, require the full ColQwen2 multivector swap Phase 3 also describes — the dense
  text embedder and the visual embedder are independent call sites
  (`embedding-engine-rs/provider/*`) and can move on separate timelines.
- **D-C (sovereignty depth) — DECIDED: also self-host reasoning (Llama).** The narrower
  "ColQwen2 only" scope isn't even live yet (D4, durability plan: the ColQwen2 reranker
  endpoint is currently unreachable) — this decision commits to the deeper Phase 6/7 scope
  on top of that still-unfinished narrower one. Concretely this means: Command R+ stops
  being the reasoning provider for anything that must stay sovereign; local Llama 3.3
  (vLLM) becomes an `inference-core` provider in Model Plane, fed by a sovereign EU GPU
  fleet (bare-metal, Azure-EU Sweden/Germany pinning per Phase 7). This is explicitly
  **Model Plane's build, not Data Plane v2's** — Phase 6 says so directly ("separate
  plane — coordinate, don't build in DP"). DP2's own obligation is limited to routing
  policy / provider-selection contracts that let a ZDR-classified request actually reach
  the sovereign path once it exists, not to running the GPU fleet.

## Suggested order
**Phase 1 (isolation) → Phase 2 (CAS/producer) → Phase 3 (ColQwen2) → Phase 4 (arms) →
Phase 5 (erasure) → 6/7 (Model Plane + infra).**
