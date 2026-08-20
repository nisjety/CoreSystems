# Data Plane / Model Plane GPU continuation — from Windows

**Written:** 2026-08-19, on the Mac (no discrete GPU), for continuation on:

> Windows-PC (Aquatiq-Dev) — Windows 11 64-bit, Intel Core i7-13700H, 32 GB RAM,
> NVIDIA RTX A1000 Laptop GPU (6 GB VRAM), 1 TB SSD, Philips 45B1U6900CH 45"
> 5120×1440 ultrawide external display.

## Read this first: what this machine is actually for

This is a **real GPU**, but a small mobile one. 6 GB VRAM is not a rounding error away from
"enough" for either of the two workstreams below — for both, the currently-named model
**does not fit at all**, at any quantization level anyone actually uses. This machine's job is:

1. Prototype and unit-test the *code* (provider wiring, HTTP contracts, config plumbing) against
   a **much smaller substitute model**, proving the mechanism works.
2. It is explicitly **not** a stand-in for Phase 7's bare-metal EU GPU procurement decision. You
   already chose to wait for real bare-metal over renting cloud GPU for that — this laptop doesn't
   change that decision, doesn't size it, and shouldn't be used to justify skipping it. Keep those
   two things mentally separate the whole way through.

## Action item before you rely on git to carry this over

Branch `model-plane-harness` (current branch all session) has **never been pushed to origin**
(`github.com/nisjety/Coresystem-verevon.git` — no upstream configured). Today's Cohere/Command A
Plus commit (`1ec4639e`) is on it, along with whatever else this branch carries. If your plan is
`git pull`/`git fetch` on the Windows PC, **push this branch first**:
```
git push -u origin model-plane-harness
```
Then on Windows: `git fetch && git checkout model-plane-harness && git pull`. Also note: this Mac
still has other pre-existing uncommitted changes (Ingestion Plane Quarry-v2 files, a Model Plane
Go file, one doc) that were dirty before this session even started and were deliberately left
alone — those won't travel via git either, by design, since they aren't confirmed to be mine.

---

## Workstream 1 — ColQwen2 visual reranker (Data Plane v2, Phase 3)

**Real code exists, it's just never been run on real GPU hardware.** The original Phase 3 plan
(swap the visual embedder itself to a ColQwen2 multivector Qdrant collection) was superseded back
in 2026-07-10 by a much lighter design that's actually built:

- `apps/Data Plane v2/services/colqwen-reranker/app.py` — a FastAPI service loading
  `vidore/colqwen2.5-v0.2` via `colpali_engine`, exposing `POST /rerank {query, image_urls}`. It
  MaxSim-reranks Embed v4's already-retrieved top-K page images — it does not replace embedding or
  storage.
- `apps/Data Plane v2/services/retrieval-engine-rs/src/search/colqwen.rs` — a thin HTTP client,
  wired into the real pipeline (`config.rs`'s `visual_rerank_enabled`, defaulting to `true` as of
  commit `e9b5b71f`; `pipeline/postprocess.rs`'s `VisualRerank` stage; degrades safely to Embed-v4's
  original order on any error).
- It's a **safe no-op today**: `COLQWEN_ENDPOINT_URL` is empty in `docker-compose.yml`, and no
  `colqwen-reranker` container is defined anywhere in the compose stack — it's designed to run
  standalone against a GPU host, not as part of the plane's own `docker compose up`.

### The VRAM wall, specifically

`app.py`'s own inline comment: the model is **15.15 GB in fp32**, roughly **7.5 GB in fp16/bf16**
(what the CUDA and MPS code paths both use) — before any activation/KV overhead from image patches
during a rerank call. **The RTX A1000's 6 GB does not fit this model at its current pinned
precision.** There is no quantization path in the repo today: no `bitsandbytes` in
`requirements.txt`, `COLQWEN_DTYPE` only accepts `float16`/`bfloat16`/`float32`, the Dockerfile
pulls a plain CUDA 12.4 torch wheel with no int8/int4 tooling.

### Concrete next steps on the Windows PC

1. **Don't assume it'll load.** First thing to try: run `app.py` locally against the real GPU and
   see whether it OOMs — 6 GB vs. a 7.5 GB weight footprint plus activations means it very likely
   will, but confirm rather than assume either way.
2. If it OOMs (expected), you need one of:
   - A `bitsandbytes` 8-bit or 4-bit load path added to `app.py` (there's a real gap here — nobody
     has built this), or
   - A smaller/quantized checkpoint variant of ColQwen2.5, if one exists upstream.
3. Once it loads (quantized or otherwise), set `COLQWEN_ENDPOINT_URL` in
   `apps/Data Plane v2/.env` to point at it and confirm `retrieval-engine-rs`'s `VisualRerank` stage
   actually calls it end-to-end — **this has never been live-verified with a real HTTP round-trip**
   anywhere in the repo; only URL-parsing and the pure score-placement function are unit-tested.
4. Whatever you learn (does it fit quantized? at what latency?) is real, useful data for the
   still-nonexistent Phase 7 "GPU sizing" deep-dive — capture it, even though this card can't
   validate production scale.

---

## Workstream 2 — local Llama reasoning provider (Model Plane, Phase 6 / decision D-C)

**This is greenfield — zero code exists.** Confirmed by grep across the whole repo (`vllm`,
`ollama`, `llama.cpp`, "Llama") and by `git log --all --grep` on all three terms returning nothing,
ever, on any branch. No service directory, no provider module, nothing to extend.

The decision that authorized this (`D-C`, confirmed 2026-08-08, in
`apps/Data Plane v2/docs/sovereign-rag-phased-plan.md`): *"local Llama 3.3 (vLLM) becomes an
inference-core provider in Model Plane, fed by a sovereign EU GPU fleet... explicitly Model Plane's
build, not Data Plane v2's."*

### The model-size wall, specifically

**"Llama 3.3" only exists as a 70B-parameter model** — Meta never released a smaller 3.3 variant
(unlike 3.1's 8B/70B/405B or 3.2's 1B/3B/11B/90B lineups). A 70B model needs:
- ~140 GB at fp16/bf16
- ~70 GB at INT8
- ~35–40 GB even at aggressive 4-bit quantization (GPTQ/AWQ/GGUF Q4_K_M)
- ~20–26 GB at the most extreme practical 2–3 bit quantization

**6 GB is roughly 4× too small at the most extreme lossy quantization anyone actually uses, and
~23× too small at full precision.** This is a hard capacity wall, not a "runs slowly" situation —
the weights themselves cannot fit. Separately: vLLM (the serving engine the plan names) defaults to
preallocating ~90% of a GPU's memory for its KV-cache pool, making it a worse fit than
llama.cpp/ollama for a tight-VRAM card even for a model that *would* otherwise fit.

### Concrete next steps on the Windows PC

1. **This needs your decision, not an assumption** — pick one:
   - (a) Treat this laptop as pure integration-code development: build the provider against a
     small local model (below) purely to prove the serving/wiring mechanism, and separately
     validate the real target (Llama 3.3 70B) later on rented or bare-metal GPU capacity.
   - (b) Substitute a genuinely small model for hands-on testing — e.g. **Llama 3.2 3B** (~2 GB at
     Q4) or **Llama 3.1 8B** (~4.5–5 GB at Q4, tight but plausible with a small context window).
     Understand this validates the mechanism only, not the plan's actual named model.
2. Given vLLM's heavy default VRAM preallocation, consider **llama.cpp or ollama** as the local dev
   serving engine on this card instead — both handle low-VRAM CUDA setups more gracefully. Whatever
   you learn doesn't have to dictate the *production* fleet's serving engine, but it makes local
   testing on this hardware actually feasible.
3. **Build pattern to copy:** vLLM/llama.cpp/ollama all expose OpenAI-compatible
   `chat/completions` HTTP APIs — so the natural template is
   `apps/Model Plane/rust/services/inference-core/src/provider/openai.rs` (the same file this
   session just extended with a third `OpenAiFlavor` variant for Cohere). A new local-model
   provider is most likely another flavor on that same struct, or a new sibling module using the
   identical `ProviderRouter` trait shape.
4. **Wire it exactly like every other provider**, not as a special case: register it in
   `apps/Model Plane/rust/services/inference-core/src/provider/fallback.rs`'s `provider_order`
   match (the `"cohere" | "azure-cohere"` arm added today, around line 464, is the freshest example
   to copy), and let it flow through the existing `ZdrAttestation`/residency gating in `zdr.rs` —
   a self-hosted provider actually has the *strongest* legitimate claim to sovereignty of anything
   in this chain, so it's worth getting that wiring right, not skipping it because "it's local
   anyway."
5. Model Plane's own status docs (`MODEL_PLANE_STATUS.md`, `MODEL_PLANE_ROADMAP.md`) don't mention
   any of this yet — this whole thread currently lives only in Data Plane v2's docs. Worth updating
   Model Plane's own tracking once real code exists here, since Model Plane owns the build.

---

## Explicitly out of scope for this machine — Phase 7 (bare-metal EU GPU fleet)

Zero code, infra-as-code, vendor contract, or sizing document exists anywhere in the repo for this
(confirmed: no Terraform/Ansible, no GPU sizing worksheet — the "blueprint deep-dive #1" both
sovereign-rag docs call for has never been written). This is a real hardware procurement decision
for **you**, not something this session or that laptop resolves. The only "cloud GPU" mention
anywhere (`colqwen-reranker/README.md`'s "Hetzner GEX44 / Azure NV once quota lands" note) predates
your self-host decision by six weeks and was never acted on — it is not a silent pivot away from
bare-metal, just stale leftover prose. **Do not read "I now have a Windows PC with a GPU" as
"Phase 7 is resolved" or "we don't need bare-metal anymore" — they are unrelated questions.**

Two small, non-GPU documentation fixes worth doing whenever (not urgent, could even be done back on
the Mac): `sovereign-rag-phased-plan.md`'s Phase 7 header still says *"(if self-host confirmed)"*
even though decision D-C already confirmed it (2026-08-08) — the conditional is stale. And
`sovereign-rag-blueprint-reconciliation.md` still frames "Decision 1" (self-host ColQwen2 + Llama)
as an open fork rather than reflecting that D-C already resolved it.

---

## Also fixed this session, for context (not something to redo)

Cohere is now fully wired through Azure AI Foundry as `cohere-command-a-plus`
(Cohere-command-a-plus-05-2026, the current name — "Command R+" itself is retired), live-verified
end-to-end except for one gap: no authenticated gRPC round-trip through the real session/auth path
was exercised (would need a real signed auth-core JWT, which wasn't fabricated). If you want that
last mile closed, that's a good small task to pick up from a session with access to real auth-core
credentials, on either machine — not GPU-dependent at all.
