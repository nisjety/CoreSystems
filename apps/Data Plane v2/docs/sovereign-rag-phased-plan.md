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
  ≠ the request body org (API-key path stays gateway-trusted by design). **RLS — deferred**
  (needs transaction-pinned connections; scoped follow-up). **gRPC `get_sources`/`get_chunks`/
  `pack_context` still take body org** (by-id, SQL-org-filtered) — extend the same helper next.

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
     prefetch→rerank); keep dense path on `text-embedding-3-large` **or** migrate text to
     **Embed v4** (multilingual) — *sub-decision: re-embed cost vs multilingual gain*.
- **Files:** `embedding-engine-rs/provider/*`, `qdrant_writer`, `image_consumer.rs`;
  `retrieval-engine-rs/embed/visual.rs`, `search/`, `pipeline/orchestrator.rs`.
- **Risk:** **HIGH** (GPU infra, multivector storage ~1000× single-vector, MaxSim serving).
  **Complexity:** HIGH. **Depends on:** Phase 2 + GPU sizing decision.

### Phase 4 — 4-arm retrieval parity (graph-in-fusion + Meilisearch)
- **Goal:** match the blueprint's arm set.
- **Steps:** fold **graph** into the fused RRF path (currently a separate endpoint — needs a
  shape adapter from entities/claims → candidates); add **Meilisearch** keyword arm
  (typo-tolerant IDs/codes) + a `w_keyword` RRF pass; keep dense.
- **Files:** `pipeline/orchestrator.rs`, `pipeline/types.rs`, `config.rs`, new
  `search/keyword.rs`, infra (Meilisearch).
- **Risk:** MEDIUM. **Complexity:** MEDIUM. **Depends on:** Phase 1 (new arm must be org-gated).

### Phase 5 — Erasure completeness for the new stores (GDPR Art. 17)
- **Goal:** the purge cascade covers every store the blueprint adds.
- **Steps:** extend the Art.17 cascade to **MinIO CAS** (raw + page images) and
  **Meilisearch**; verify Qdrant visual-multivector purge, Quickwit segment prune, and
  Dragonfly/semantic-cache eviction end-to-end. (Page-image Qdrant purge already wired in PR-D.)
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

## Open decisions (please confirm)
- **D-A (tenant):** Is "tenant" a real axis ABOVE org (e.g., MSP/reseller grouping many
  orgs), or is **org the tenant boundary**? If the latter, "tenant isolation" = org
  isolation (✅ today, hardened in Phase 1). If the former, it's a new schema axis (added to
  Phase 1).
- **D-B (text embedder):** Keep `text-embedding-3-large` for dense text, or migrate to
  **Embed v4** (multilingual) — re-embed cost vs multilingual recall.
- **D-C (sovereignty depth):** Self-host ColQwen2 only, or also reasoning (Llama) so the
  boundary isn't crossed by Command R+/Rerank?

## Suggested order
**Phase 1 (isolation) → Phase 2 (CAS/producer) → Phase 3 (ColQwen2) → Phase 4 (arms) →
Phase 5 (erasure) → 6/7 (Model Plane + infra).**
