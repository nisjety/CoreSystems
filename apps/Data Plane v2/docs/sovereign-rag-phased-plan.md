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
- **`retrieval-engine-rs` ✅ (2026-08-09)** — the service D-A actually needs, since an
  account-scoped read flows through it. **47 statements scoped, 19 deliberately left, zero
  unclassified.** Left unscoped, each commented in place: `gdpr/purge.rs` (5 — destructive
  cross-table erasure where a subtle scoping interaction would silently delete zero rows and
  *look* like success, i.e. a GDPR failure; deserves its own change with dedicated
  verification), `api/mod.rs::cleanup_orphans` (8) and `semantic_cache_prune` (3) (admin
  maintenance; the latter uses `pg_try_advisory_xact_lock`, whose lifetime semantics change
  inside an explicit transaction), `audit::record_admin` (1 — `admin_audit_log.org_id` is
  NULLABLE for non-org-scoped admin actions, so a scoped INSERT would be rejected by
  `WITH CHECK`), plus `warmup_pool` and `readyz` (no org exists).
  - **Two functions gained an org predicate they never had.**
    `orchestrator.rs::join_sources` and `pipeline/postprocess.rs` fetched
    `FROM documents WHERE document_id = ANY($1)` with **no org filter at all** — safe today
    only because candidates arrive pre-filtered from the org-scoped arms. Both now take the
    org explicitly and run scoped, so the database supplies the predicate structurally.
    Several gRPC handlers also join `documents` with no org predicate on the join side; RLS
    now covers that too.
- **⚠ A real defect in the first migration, found during this rollout and fixed:**
  `20260809120000` granted `dataplane_app` privileges only inside its 35-`org_id`-table loop,
  and `ALTER DEFAULT PRIVILEGES` covers only *future* tables — so a scoped transaction that
  merely *joined* a table without an `org_id` failed outright (`permission denied for table
  wiki_page_versions` / `retrieval_candidates`, both reproduced live). Scoping
  `search/wiki.rs` without noticing would have taken down the entire wiki retrieval arm at
  runtime while every non-DB test still passed. Fixed by
  `20260809180000_org_rls_child_tables.sql`, which grants **and** isolates the three
  transitively org-scoped children via parent-derived policies
  (`retrieval_candidates`→`retrieval_runs`, `wiki_page_versions`→`wiki_pages`,
  `chunk_lineage`→`documents`; all three FK columns indexed, so the `EXISTS` is an index
  probe). Granting without policies was rejected as the wrong fix — it would have left a
  scoped role able to read any org's children by guessing a key. Verified live: the child
  `WITH CHECK` correctly resolves a parent written earlier in the *same uncommitted*
  transaction (which is what makes the merged `persist_trace` transaction correct), and still
  rejects a foreign-org parent. `schema_migrations` is deliberately excluded — genuine global
  infrastructure, only ever touched by the superuser migrator.
- **⚠ Test-fixture regression introduced and fixed in the same pass.** The DB-gated
  (`#[ignore]`d) suites build their own schema on a bare database and never created the
  `dataplane_app` role, so once production code called `begin_org_scoped` they all failed with
  `role "dataplane_app" does not exist` — **invisible to `cargo test`**, which is exactly the
  silently-disabled-safety-net pattern this phase exists to prevent. Fixed with one shared
  `tests/common::grant_rls_runtime_role` helper wired into the 5 suites that exercise a scoped
  path; the 4 that don't were audited and left alone. Grants only, not policies — deliberately,
  since enabling policies in fixtures would change what each suite asserts. **Worth doing
  later:** making the fixtures install the real policies would upgrade them from "the SQL's own
  `org_id` predicate filters correctly" to "the database filters correctly even if that
  predicate is dropped", which is the actual guarantee this phase buys.
- **Two PRE-EXISTING DB-gated failures found, verified unrelated, and deliberately not fixed**
  (fixing them changes what the suites assert): `zdr_behavior::test_persist_trace_creates_row_
  when_not_ephemeral` fails because the fixture seeds `retrieval_traces` while production writes
  `retrieval_runs` — confirmed by direct grep, the fixture contains zero occurrences of the real
  table name; and `grpc_integration::test_document_index_status` fails because the fixture
  inserts `embedding_status = 'completed'` while production counts `'done'`, a literal that
  changed in `7d2f7b2f` without the fixture following. Also worth knowing for CI:
  `cross_org_isolation` and `grpc_integration` **require `--test-threads=1` against an empty
  database** — their fixtures race on concurrent `CREATE TABLE IF NOT EXISTS`. All three
  predate this work.
- **`graph-index-rs` ✅ (2026-08-09)** — the cleanest rollout so far: all 56 sites live in one
  file (`src/store.rs`), every production method already took `org_id`, and all 7 tables it
  touches carry an `org_id` and were already granted, so neither the threading nor the grant
  hazard applied. **33 statements scoped across 18 methods (19 transactions), 5 left** — the
  `purge_organization_data` erasure, for the identical reason as `retrieval-engine-rs`'s. Two
  methods already opened `self.pool.begin()` and were *converted* to `begin_org_scoped` rather
  than nested. One deliberate exception to one-transaction-per-method:
  `detect_claim_contradictions` keeps its read phase and write phase separate, so it does not
  hold a read-write transaction open across the whole detection pass or open a write
  transaction when there is nothing to record.
  - **Verified live with a real extraction**, not just a healthy boot: entities 768→769,
    relationships 238→241, claims 329→331, and 31 text-unit mappings written for the processed
    chunk — all through the scoped write path.
- **⚠⚠ An operational trap that cost real debugging time, worth reading before the next
  service.** The first post-deploy extraction returned `entities:0` in 64ms (vs ~4s for a real
  LLM call) and looked exactly like an RLS regression. It was not. Two compounding causes:
  1. **`docker compose up -d <service>` silently detaches DPv2 services from the real
     inter-plane bus.** The base compose file deliberately defaults its `inter-plane-bus`
     network to a *local* bridge (`dpv2-cross-plane`) so DPv2 can boot without Control/Model
     Plane; `docker-compose.cross-plane.yml` is the overlay that swaps in the real external
     `inter-plane-bus`. Recreating a service without that overlay moves it onto the dummy
     bridge, so every cross-plane call (auth-core token minting, Model Plane inference) starts
     failing while the container still reports **healthy**. Five services were affected before
     it was noticed. **Always deploy with
     `-f docker-compose.yml -f docker-compose.cross-plane.yml`.** Diagnose by comparing
     `docker inspect <c> --format '{{range $k,$v := .NetworkSettings.Networks}}{{$k}} {{end}}'`
     against a service you have not recreated.
  2. **`stream.rs` calls `load_org_visible_chunks(...).unwrap_or_default()`**, so any failure
     to load chunks is swallowed into an empty vector and reported as a successful extraction
     of zero entities. Pre-existing, not introduced here, but it is what turned an
     infrastructure outage into a silent no-op. The real error was only visible one line
     earlier (`extraction failed: inference service-token endpoint unavailable`) — worth
     hardening separately.
- **⚠ The DB-gated-fixture trap recurred, in a new hiding place.** `graph-index-rs` has no
  `tests/` directory, which is *not* sufficient evidence that it has no DB-gated fixtures — it
  keeps them inside `src/store.rs` as `mod visibility_tests`, seeded from
  `GRAPH_TEST_DATABASE_URL` into a per-test `graph_visibility_<uuid>` schema. Three of its
  `#[ignore]`d tests began failing with `role "dataplane_app" does not exist`; fixed the same
  way, with three corrections the `retrieval-engine-rs` version did not need: grants must
  target the **per-test schema** (not `public`), `ALTER DEFAULT PRIVILEGES` is required because
  one test creates a table *after* the fixture returns, and — the important one — the
  role-creation guard must catch **`duplicate_object` OR `unique_violation`**. Catching
  `duplicate_object` alone does not survive the race it exists for: with the role absent, the
  losing backend faults on `pg_authid_rolname_index` and raises `unique_violation` (23505)
  before the duplicate-object path is reached, reproduced verbatim against a real cluster. The
  `retrieval-engine-rs` helper had that same defect and has been corrected.
- **`index-engine-rs` + `quickwit-adapter-rs` ✅ (2026-08-09)** — deliberately small scoped
  surfaces, because most of both services is legitimately cross-org.
  - `index-engine-rs`: **10 statements under 2 scope entry points** (`builder::process_document`,
    which converts its existing `pool.begin()` and covers 8 in-function statements plus 2
    delegated to `outbox::enqueue_intent` on the same transaction; and `reembed-switch`'s
    operator query). Left unscoped: `outbox.rs` (7 — drains for all orgs), `reconcile.rs` (2 —
    the D19 reconciler claims stranded units plane-wide, and each row's `org_id` is an *output*
    of the claim, not an input), `gdpr.rs` (2 — the erasure precedent). Two scoped statements
    carry no `org_id` of their own and gain isolation purely from policy: the
    `parent_window_text` refresh (keyed only by `knowledge_id`) and the `chunk_lineage` insert
    (via its `documents` parent).
  - `quickwit-adapter-rs`: **3 statements** — the event-driven `index_knowledge_unit_by_id` /
    `index_document_knowledge_units` / `index_source_object_by_id`, name-for-name the functions
    already scoped in the sibling `meilisearch-adapter-rs`. Left unscoped: the 5 `rebuild_*`
    stages (their `org_id` is `Option`, predicate `($1::TEXT IS NULL OR ...)` — a whole-corpus
    rebuild passes `None`, which a scoped transaction cannot express), `jobs.rs` (13),
    `ingest_and_checkpoint_batch`, and `gdpr.rs` — the last three all touching
    `quickwit_admin_jobs`/`quickwit_admin_job_audit`, whose **NULLABLE `org_id`** a scoped
    write cannot satisfy.
  - Verified live: a real re-announce drove quickwit-adapter's newly-scoped path to return all
    **10** real chunks. Separately verified that the scoping is genuinely active rather than
    inert, by enabling `log_statement=all` on a disposable server and counting
    `SET LOCAL ROLE dataplane_app` occurrences against each test's known structure.
- **`wiki-store-go` ✅ (2026-08-09) — first Go adopter, proving the Go helper end to end.**
  **45 statements under 24 `WithOrgScope` callbacks**; three functions that already opened
  `pool.Begin` were converted so the scoped transaction *is* that transaction, not a nested
  second one. Left unscoped: the `wiki_event_outbox` drain (3 — one process serves every org;
  a short batch is indistinguishable from an idle queue, so scoping would silently stop
  publishing other tenants' events) and `HardPurgeByOrg` (5 — the erasure precedent, with a
  sharper articulation worth keeping: **under RLS a `DELETE` can only remove rows the policy
  lets the role *see*, so a row with drifted or NULL `org_id` would survive erasure while the
  summary still reported success**). **Structural hardening worth copying to the remaining Go
  services:** its eight private helpers were converted from `*WikiRepo` methods holding the
  pool into package-level functions taking `pgx.Tx`, so they can no longer be called unscoped —
  the compiler now enforces what a comment used to.
- **⚠ A latent crash surfaced by restarting `wiki-store` — pre-existing, unrelated to RLS, and
  now fixed.** `internal/events/publisher.go` demanded `DATAPLANE_WIKI` retention be
  **WorkQueue** and fatally refused to boot otherwise. But that stream is legitimately
  **Interest** (WorkQueue permits exactly one consumer per subject, and this subject fans out
  to more than one reader — the same Interest-over-WorkQueue correction already applied to
  `DATAPLANE_DOCUMENTS` and `DATAPLANE_KNOWLEDGE`). Retention is immutable, so the service could
  not have started since that correction — it had simply not been restarted in three days, and
  the RLS deploy is what surfaced it. Fixed by making the publisher create and require
  `Interest`, with the mismatch error now reporting what it wanted and what it found instead of
  a bare "contract mismatch". **The general lesson: a long-uptime container can be hiding a
  boot-time regression; a service that has not restarted is not evidence that it can.**
- **Go helper ergonomics — feedback from the first real adopter, worth acting on before the
  remaining 3 Go services:** (1) it hands back a `pgx.Tx` while repos are written against
  `*pgxpool.Pool`, so adoption is one atomic all-or-nothing refactor — exporting a `Queryer`
  interface both satisfy would allow incremental migration; (2) `fn func(pgx.Tx) error` forces
  every read path to declare results outside the closure and assign in, which invites a
  zero-value bug on early returns — a generic `InOrgScope[T]` variant would remove it; (3)
  **`pgx.Rows` must be fully drained inside the callback** (returning them out compiles, then
  fails at runtime because commit closes them) — currently undocumented; (4) nesting a scoped
  call inside a callback compiles and silently checks out a second pool connection.
- **Remaining rollout:** 3 Go services — `documents-api-go` (55 sites), `data-quality-go` (42),
  `data-orchestrator-go` (35). **⚠ Blocked on an infrastructure prerequisite**, not on the RLS
  work: unlike `wiki-store-go` (whose compose build context is already the repo root and which
  already carried a sibling-module `replace`), these three build from `context:
  services/<svc>`, so a `replace ... => ../../shared/go` would resolve for `go build` and then
  **fail only at image-build time**. Each needs its context widened to the repo root and its
  Dockerfile `COPY` paths adjusted, following `services/wiki-store-go/Dockerfile` as the
  working template (it copies the sibling module's `go.mod`/`go.sum` before `go mod download`
  and the full tree before `go build`). Their multi-org drains — `documents_outbox`,
  `data_orchestrator_jobs` — must stay unscoped.

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
