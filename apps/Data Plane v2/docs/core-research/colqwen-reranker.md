# colqwen-reranker Research Dive

Generated: 2026-07-10 (new — no prior core-research doc existed for this service; it
was flagged as undocumented/unaudited during the 2026-07-10 Data Plane v2 pass and is
being baselined here for the first time)

Scope: `apps/Data Plane v2/services/colqwen-reranker` (+ its call site in
`services/retrieval-engine-rs/src/search/colqwen.rs` and
`services/retrieval-engine-rs/src/pipeline/orchestrator.rs`)

## Secure-MVP current state — 2026-07-10

- **Implemented:** optional ColQwen reranking remains feature-flagged and is not
  required for the secure MVP. Retrieval bypasses this optional egress path for
  restrictive ZDR requests in source.
- **Tested:** no new ColQwen service test was run in this production-readiness
  program. The historical Rust URL-parsing test below is too narrow to prove the
  Python runtime or reranking behavior.
- **Built/deployed/reachable/effective:** none proven. The service is absent from
  the Data Plane Compose file and was not built or contacted in this pass.
- **Coverage:** no Python or reranking-behavior coverage was measured.
- **Gate:** keep disabled for MVP. Enabling an external GPU service would require
  a verified scoped service identity, tenant/ZDR propagation, egress policy, and
  its own isolated build/runtime tests.

The live-environment observations below are historical and superseded for current
state; they remain useful dependency and design notes only.

## Historical snapshot (superseded for current state)

`colqwen-reranker` is a small, self-contained FastAPI microservice that runs
**ColQwen2.5** (a ColPali-family visual document model) to do late-interaction
(MaxSim) reranking over **Cohere Embed v4**'s already-retrieved top-K page-image
candidates. It is *not* a replacement for the visual embedding/storage arm — it is
an optional second pass that only reorders candidates Embed v4 already found. The
retrieval-engine calls it over plain HTTP; it is **off by default** and gated by two
env vars (`VISUAL_RERANK_ENABLED`, `COLQWEN_ENDPOINT_URL`).

Repo footprint is tiny and complete for what it claims to be:

- `Dockerfile` — CUDA 12.4.1 production image, GPU-only (no CPU/MPS Docker path)
- `README.md` — accurate, matches the code (a rarity worth calling out positively)
- `app.py` — 101 lines, the entire service
- `requirements.txt` — 18 lines, mostly a version-compatibility warning comment

Deployment status (live-verified 2026-07-10):

- **Not in `docker-compose.yml` as a service.** It only appears as three lines of
  env-var passthrough on `retrieval-engine-rs` (lines 284–290: `VISUAL_RERANK_ENABLED`,
  `COLQWEN_ENDPOINT_URL`, `VISUAL_RERANK_TOP_K`, all defaulting to off/empty/20).
  There is no `colqwen-reranker:` service block, no build context, no port mapping.
- **Not running.** `docker ps -a` shows no container matching `colqwen` anywhere in
  the stack (confirmed against the same 14-container baseline this plane's other
  docs use — `dpv2-*` — none of them is this service).
- **Not in CI.** `apps/Data Plane v2/.github/workflows/ci.yml` has zero references to
  `colqwen`.
- **Not in the plane `Makefile`.** No build/test/run target references it.

This is by design, not a discovered gap: the README explicitly documents it as a
GPU-host service (Hetzner GEX44/GEX130, or Azure NV/NC "once quota lands") that is
meant to be run and pointed to independently, or run natively on Apple Silicon/MPS
for local verification — never inside the plane's own Docker Compose network.

## Runtime Shape

`app.py` is the entire runtime:

- Loads `MODEL_ID` (`vidore/colqwen2.5-v0.2` default) via `colpali_engine`'s
  `ColQwen2_5` / `ColQwen2_5_Processor` **at import time**, before the FastAPI `app`
  object is even constructed. Device/precision selection: CUDA → bf16, Apple MPS →
  fp32, else CPU (eager attention only — flash-attn is CUDA-only).
- `POST /rerank` — body `{query, image_urls}`, fetches each image over HTTP
  (sequentially, one `httpx.Client` GET per URL, 30s timeout each), runs ColQwen's
  processor + model on the query and the images, and returns
  `{"scores": [...]}` — one MaxSim score per `image_urls` entry, in order.
- `GET /healthz` — `{"ok": true, "model": ..., "device": ...}`.
- `COLQWEN_URL_REWRITE` — an escape hatch (`"quarry-edge:8082=localhost:8082"`
  comma-separated pairs) so a server running *outside* the compose network (a local
  Mac, or an external GPU box) can resolve the Docker-internal image URLs the
  retrieval-engine emits. Well-commented, pragmatic.

## Wiring / Call Site — real and live-wired, just switched off

This is not a dangling stub the rest of the system ignores; the Rust side genuinely
calls it when enabled, and the plumbing is clean:

- `retrieval-engine-rs/src/search/colqwen.rs` — `ColqwenClient`: thin `reqwest`
  client, 30s timeout, `from_url()` returns `None` on an empty/blank endpoint
  (safe-disable by construction, not by a separate `if enabled` check at every call
  site). One unit test (`from_url_empty_is_none_and_trims_slash`).
- `config.rs` — `visual_rerank_enabled: bool` (`#[serde(default)]` → `false`),
  `colqwen_endpoint_url: String` (`#[serde(default)]` → empty),
  `visual_rerank_top_k: usize` (`default_visual_rerank_top_k()` → `20`).
- `main.rs` (lines ~234–250) — constructs the client only if
  `cfg.visual_rerank_enabled` is true **and** `ColqwenClient::from_url` succeeds;
  otherwise `None`, with a `tracing::warn!` if the flag is set but the URL is empty
  (misconfiguration is logged, not silently ignored).
- `pipeline/orchestrator.rs::visual_rerank()` (lines 82–147) — the real integration
  point. Confirmed behavior by direct source read:
  - No-ops (returns `fused` unchanged) when `self.colqwen` is `None` — the default,
    live-everywhere-today path is a single `Option` check, not a code fork.
  - **Explicitly skips reranking under ZDR** (`if embed_zdr { return fused; }`,
    line 97–100) — page images are not sent to the reranker for a ZDR query. This is
    one of the few places in the plane's visual-RAG pipeline that actively **honors**
    Zero Data Retention rather than needing a fix for it (contrast with this plane's
    other known ZDR gaps: bulk-ingest bypassing the single-create guard, retrieval
    still embedding query text in "ephemeral" mode, semantic cache persisting under
    `zdr=true`).
  - Non-fatal on any client error — logs a warning and returns the original Embed-v4
    order (`tracing::warn!(... "visual reranker failed; keeping Embed-v4 order")`).
  - Reorders **only the page-image candidates among themselves**, capped at
    `visual_rerank_top_k` (default 20): it reuses the score band the visual
    candidates already occupy in the fused list (`band` array, sorted descending)
    and reassigns ColQwen's best-to-worst ranking into that same band, so a
    ColQwen-favored image can't leapfrog ahead of unrelated text candidates it never
    competed against. This is a careful design choice, not an accident.
  - Requires ≥2 visual candidates to do anything (0 or 1 → nothing to reorder).

Net: the integration is correct and safety-conscious. The reason it doesn't affect
production today is purely the `VISUAL_RERANK_ENABLED=false` / empty
`COLQWEN_ENDPOINT_URL` defaults in `docker-compose.yml`, not a wiring gap.

## Live-Verification Findings (2026-07-10)

1. **Confirmed not deployed** (see Snapshot) — expected/by-design, not a finding
   against the code.

2. **Bug, live-reproduced — the checked-out local verification environment cannot
   currently import the app.** The README's "Local verification (Apple Silicon /
   MPS)" workflow has clearly been attempted here: `services/colqwen-reranker/.venv`
   exists (gitignored, so not part of the repo state itself) with PyTorch 2.11.0,
   torchvision 0.26.0, and `colpali-engine` 0.3.17 installed, and
   `~/.cache/huggingface/hub` already has both `vidore/colqwen2.5-base` and
   `vidore/colqwen2.5-v0.2` downloaded (dated 2026-06-23). But the installed
   `transformers` in that same venv is **4.57.6**. Running the app's own import line
   directly:
   ```
   $ .venv/bin/python3 -c "from colpali_engine.models import ColQwen2_5, ColQwen2_5_Processor"
   ImportError: cannot import name 'ModernVBertModel' from 'transformers'
   ```
   `colpali_engine/models/__init__.py` unconditionally imports the `ModernVBert`
   submodule (a different model family the app never uses — it only asks for
   `ColQwen2_5`), and that submodule needs `transformers>=5`. `requirements.txt`
   *already documents this exact hazard* in its own comment: colpali-engine 0.3.17
   only round-trips its LoRA weights correctly with one of two "blessed trios" —
   `(a) colpali-engine~=0.3.7 + transformers~=4.50` or
   `(b) colpali-engine>=0.3.17 + transformers>=5`. The installed venv is
   **`colpali-engine 0.3.17 + transformers 4.57.6`** — neither trio, and the
   specific combination that fails to import at all (not just "LoRA keys missing,"
   which the comment treats as the milder failure mode). Since the import happens
   at module scope before `app = FastAPI(...)` is even reached, `python app.py`
   cannot serve `GET /healthz` or anything else in this environment right now — it
   crashes before binding the port. Confirmed live that `transformers>=5` does
   resolve on PyPI today (checked up to `5.13.0`), so recreating the venv per the
   current `requirements.txt` would very likely fix this; the drift is in the
   already-installed environment, not necessarily in what a fresh `pip install`
   would produce. The production Docker path (`Dockerfile` → fresh
   `pip3 install -r requirements.txt` inside the CUDA image) was not built/run in
   this environment, so it is not proven to hit the same issue — but it has also
   never been proven *not* to, since no colqwen image has been built here.

3. **Zero test coverage on the actual reranking behavior.** The only test in the
   Rust client (`colqwen.rs`) checks URL parsing
   (`from_url_empty_is_none_and_trims_slash`). `orchestrator.rs::visual_rerank()` —
   the method containing the score-based reorder, the ZDR skip, the top-K cap, and
   the band-preserving reinsertion — has no unit or integration test anywhere in the
   crate (confirmed by grep across `orchestrator.rs`). `app.py` has no test file at
   all. Given the feature is off by default, this logic has never been exercised by
   the retrieval-engine's own test suite (the "41/41 lib tests" cited in
   `visual-rag-integration-plan.md`'s PR-C status do not touch this path).

4. **No health-check gating on the reranker leg** — a deliberate tradeoff, not a
   bug: `retrieval-engine-rs`'s own `/health`/`/readyz` do not probe
   `COLQWEN_ENDPOINT_URL` reachability. Consistent with the "non-fatal, degrade to
   Embed-v4 order" design, but it means an operator who flips
   `VISUAL_RERANK_ENABLED=true` with a stale/wrong endpoint would only discover it
   via a `tracing::warn!` log line and silently-unreranked results — no startup
   failure, no red health check. Worth knowing before anyone turns this on.

## Relationship to the visual-RAG and sovereign-RAG planning docs

This service sits at an interesting, slightly unreconciled point between two other
docs in this same `docs/` directory:

- `docs/visual-rag-integration-plan.md` (PR-A–E, 2026-06-22) chose **Cohere Embed v4
  single-vector** as the v1 visual arm specifically *to avoid* self-hosted ColPali/
  ColQwen2 — it lists "❌ Self-hosting ColPali for v1" under **Anti-patterns to
  avoid** and calls self-hosted ColQwen2 "a future flagged accuracy tier, not the
  v1." That plan is built and live (Embed v4 provider, page-image consumer, visual
  RRF fusion — all real per the plane's other core-research docs).
- `docs/sovereign-rag-phased-plan.md` and `docs/sovereign-rag-blueprint-reconciliation.md`
  (later planning docs, both still headed "planning artifact — no code yet, awaiting
  confirmation") describe a much bigger **Phase 3: swap the visual embedder itself,
  Embed-v4-images → ColQwen2 multivector**, requiring a new Qdrant multivector/MaxSim
  collection and GPU fleet, rated **HIGH risk/complexity**, and explicitly marked
  "ColQwen2 [decided, not yet built]" as of that doc. That full swap has **not**
  been built — no multivector Qdrant collection exists.

What actually exists in `services/colqwen-reranker` is neither of the above: it is a
**narrower, reranking-only** use of ColQwen2.5 that sits entirely downstream of
Embed v4's single-vector retrieval — no storage change, no multivector collection,
Embed v4's own arm untouched. It most closely resembles the reconciliation doc's
third option for Decision 1 ("Both behind a flag → Embed v4 default, ColQwen2 as a
sovereign opt-in"), scoped down to reranking rather than embedding. It is not named
or cross-referenced in either sovereign-rag doc, so it reads as a parallel
exploratory build rather than the tracked output of a recorded decision — worth
reconciling explicitly in a future planning pass so the next reader doesn't assume
"ColQwen2 [decided, not yet built]" means *nothing* ColQwen-shaped exists yet.

## Duplicates / Redundancies / Dead Surfaces

- No redundancy with the built Embed v4 visual arm — additive only, reranks on top
  of it rather than replacing anything.
- Its `.venv` carries a fully separate PyTorch/torchvision/colpali-engine stack,
  isolated from every other service's Go/Rust/TS toolchain in this plane. That's
  appropriate for a Python ML server; it is also the only Python *runtime* service
  under `services/` (the other Python entry, `retrieval-eval-py`, is an eval/support
  script, not a deployed service — see the plane README's "Support and scaffold"
  note).

## API Design And Performance Notes

- Contract is small and matches the Rust client exactly (field names, array order,
  `scores.len() == image_urls.len()` — the Rust side defensively
  `anyhow::bail!`s on a length mismatch rather than trusting it).
- `_fetch_image` fetches candidate images **sequentially** in a Python `for` loop
  (30s timeout each), not concurrently. At `visual_rerank_top_k=20` (the default),
  a worst-case rerank pass could serialize up to 20 image fetches before even
  running inference. This is currently moot (feature is off everywhere), but worth
  flagging before anyone enables it at that top-K value.
- The `COLQWEN_URL_REWRITE` mechanism is a sensible, well-scoped fix for the real
  problem of running this server outside the compose network while
  `retrieval-engine-rs` emits Docker-internal image URLs (`http://quarry-edge:8082/...`).

## Current Doc Cleanup Read

- No service-local docs beyond `README.md`, and it is accurate — matches the code
  precisely, including the exact env var names and defaults. No cleanup needed here.
- Recommend a follow-up note in `docs/sovereign-rag-phased-plan.md` (or a short
  addendum) cross-referencing this service, so "ColQwen2 [decided, not yet built]"
  in that doc doesn't read as contradicting the fact that a (narrower,
  reranking-only) ColQwen2 integration already exists and compiles.

## Historical bottom line (superseded)

`colqwen-reranker` is real, working-as-designed code for a narrow, currently-inert
feature: an optional GPU-backed ColQwen2.5 late-interaction reranker that
`retrieval-engine-rs` can call to reorder Embed v4's page-image results, gated
behind `VISUAL_RERANK_ENABLED` (default false) and never deployed via
`docker-compose.yml`, CI, or the plane `Makefile`. The Rust-side wiring (client,
config, orchestrator integration) is clean, fails soft, and — notably — is one of
the few points in the visual-RAG pipeline that actively **respects** ZDR by
skipping egress outright, rather than needing a fix for it. The Python side is
small and matches its documented contract exactly.

The one live-verified defect: the pre-existing local-verification `.venv` on this
checkout cannot currently import `app.py` at all — `colpali-engine==0.3.17` pulls in
a `ModernVBert` import path that needs `transformers>=5`, but the installed
`transformers` is `4.57.6`, exactly the mismatched combination
`requirements.txt`'s own comment warns against. Anyone following the README's
local-verification steps today, in this venv, would hit an immediate `ImportError`
before the server ever binds a port. There is also no test coverage anywhere for
the actual reranking logic (`orchestrator.rs::visual_rerank()` or `app.py`) — only
URL-parsing is unit-tested. None of this affects the live plane today (the feature
is off everywhere), but it does mean the feature is further from "ready to flip on"
than the code's overall polish suggests.
