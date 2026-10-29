# Embedding modality + RAG paradigm audit, and Azure Foundry vs. local model research

**Date:** 2026-08-19. **Method:** 9-agent research workflow — 2 codebase audits (read actual code,
not just grep) + 7 web-grounded research tracks (Azure AI Foundry catalog as of Aug 2026 + local
OSS alternatives + real cost numbers), including the 8 URLs given directly. One contradiction
between two agents' findings (on Adaptive RAG) was caught and resolved by direct verification
before writing this up — noted inline.

---

## 1. Do we have embedding capability for every modality?

| Modality | Status | What's actually there |
|---|---|---|
| **Text** | ✅ Real, live | 3-way pluggable cloud backend (`model_plane` / `azure_openai` / `cohere`) in `embedding-engine-rs/src/provider/mod.rs`, unit-tested including explicit ZDR-egress-guard tests. No local backend. |
| **Image** | ⚠️ Real, but narrower than it sounds | Cohere Embed v4 embeds **web-page screenshots only** — the only real producer of `page_images.created` is Quarry-v2's browser renderer ("MVP web slice: one PNG per document"). **Nothing rasterizes an uploaded PDF's pages anywhere in the repo.** ZDR-flagged images are silently dropped (`image_consumer.rs:302-311`, acked without embedding, "no compliant visual embedding path"). |
| **Video** | ❌ Zero code | Confirmed by reading, not just grepping. `inference-core/provider/video.rs` is Sora-style *generation*, unrelated. |
| **Audio/Sound** | ❌ Zero code | Whisper transcription exists (speech→text) but that's not an embedding vector, and nothing bridges a transcript into the text-embedding pipeline either. |
| **PDF (as distinct path)** | ✅ Text-only, real | `pypdf` extracts text per-page (`imports-core/app/parsers.py`) and routes through the same cloud text backends. The ZDR attestation mechanism (`provider/zdr.rs`) is real — SHA-256-bound, not a checkbox — but **every `AZURE_OPENAI_ZDR_*` var defaults empty/false** in this checkout, so nothing currently qualifies as ZDR-compliant. A ZDR-flagged PDF chunk today finds every candidate skipped. |
| **General documents** (docx/txt/md/csv/json/html, + pptx/xlsx/odt via optional Tika) | ✅ Real | Native parsing, routes through the same cloud text embedding. "Best quality" for this modality is already a config choice (`EMBEDDING_PROVIDER=cohere`), not a build gap. |

**Bottom line:** text and general-documents are genuinely done. Image is half-done and mis-scoped (screenshots, not PDFs) with a live ZDR data-loss gap. Video and audio are both greenfield.

---

## 2. RAG paradigms — do we have temporal RAG and "the other versions"?

There's no single canonical "5 versions" list — it's two axes: a **maturity ladder** (Naive →
Advanced → **Modular** RAG — CoreSystem is already Modular, that's not a gap) and a set of **named
paradigms** layered on top. Checked against the actual retrieval-engine-rs code:

| Paradigm | Status | Evidence |
|---|---|---|
| Naive/dense retrieval | ✅ Real | Base Qdrant ANN arm everything else fuses onto |
| **Hybrid RAG** | ✅ Real, wider than textbook | Dense + Postgres FTS/Quickwit BM25 + Meilisearch keyword + graph + visual, RRF-fused (k=60) + Cohere rerank — 5-6 arms, not the usual 2 |
| **Graph RAG** | ✅ Real, two-tier | In-process 1-hop entity/relation arm RRF-fused (`w_graph`) + a separate Neo4j-backed deep multi-hop `/v1/graph/traverse` |
| **Visual/Multimodal RAG** | ✅ Real | Cohere Embed v4 multimodal query embedding + optional ColQwen2.5 MaxSim rerank, RRF-fused (`w_visual`) |
| **Agentic RAG** | ⚠️ Real but overstated | 14 typed retrieval tools exist and are contracted (`gap-data.md §6.3`) — but Model Plane's execution-core agent currently only calls **one** of them (`knowledge_search`, hybrid-only). The retry/reformulate loop is real; full agent-directed tool routing across strategies is not wired yet. |
| **Self-RAG** | ❌ Absent | No reflection-token self-critique mechanism anywhere — zero hits for self-rag/groundedness/reflection tokens in either plane |
| **Corrective RAG (CRAG)** | ⚠️ Half-wired | The confidence gate (reranker-score threshold → `low_confidence` status) is real and genuinely consumed by Model Plane's agent. The auto-corrective *retrieval-strategy* hint is dead code. |
| **Adaptive RAG** | ✅ Real — **verified directly after two agents disagreed** | `pipeline/smart_mix.rs` (341 lines) classifies query text with deterministic English+Norwegian lexical heuristics (question words, relational cues, code/ID-like tokens, visual cues) and suggests per-query blend-weight overrides for the dense/BM25/graph/visual arms — this is genuine adaptive *retrieval-strategy* routing, not just the separate LLM-provider routing in `intent.rs`. One research agent missed this file and wrongly called it "a name collision" with model routing; I read `smart_mix.rs` myself to confirm it's real. |
| **Temporal RAG** | ⚠️ Partial, and narrower than the current definition | Three real mechanisms exist: `/v1/retrieve/timeline` (before/after date filter + trace replay), a freshness/staleness scorer, and a recency-decay postprocessor that actually multiplies into ranking — **but it ships OFF by default**, never validated against the golden eval set, and the three were never unified. 2025/2026 papers (TG-RAG, STAR-RAG, T-GRAG) define "Temporal RAG" as resolving conflicts over timestamped knowledge graphs — genuinely more than recency filtering. What we have is real but narrower. |
| **Cache-Augmented Generation (CAG)** | ✅ Plumbing real, **confirmed unused** | `context_pins` table + `pack_context_with_pins` genuinely prepends static facts instead of retrieving them (textbook CAG) — but the repo's own live-data audit states "context_pins: 0 — CAG unused." |

**Direct answer:** 6 of 9 named paradigms are real and load-bearing (Hybrid, Graph, Visual, Adaptive, plus partial Agentic/CRAG). Temporal RAG is real-but-narrow. CAG has real plumbing nobody uses. Self-RAG doesn't exist at all. If "temporal RAG actually mattering" is the goal, the fix is an eval/config task (flip `RECENCY_DECAY_ENABLED` on somewhere non-prod, tune against the golden set, then default it on) — not new code.

---

## 3. Per-modality: Azure AI Foundry vs. local, with real numbers

### Image (your requirement: **local**)
- **Azure:** nothing worth adding. Foundry's only dedicated image-embedding entrant is DINOv2 — no text tower (image-similarity only, not text→image retrieval) and billed as managed-compute VM time, not serverless. Doesn't clear the bar.
- **Local:** **SigLIP2 So400m/14** (Google, Apache-2.0, 400M params, ~0.8GB fp16) as the default — fits the 6GB test rig with room to spare, has a real text tower. For a real server: **Qwen3-VL-Embedding-2B** (2.1B params, current 2026 open-weight leader on cross-modal retrieval benchmarks).
- **Cost:** a rented RunPod L4 24GB ≈ $285/month flat handles very high image volume — cheaper than any per-call cloud API past a low threshold, and required anyway by your locality requirement.

### Video (your requirement: **local**, zero code today)
- **Azure:** no real fit. Content Understanding's video analyzer is extraction/description (transcripts, chapters), not a dense embedding model — using it would mean re-architecting as "extract text, then text-embed," a different and lossier capability.
- **Local:** **LanguageBind** (PKU-YuanGroup, MIT) — binds video *and* audio into CLIP's shared space, so one model family covers both your video and audio requirements at once.
- **Cost:** the Hetzner GEX44 box already earmarked for ColQwen (20GB VRAM, EU-hosted, ~$200/month flat) can likely absorb this too. Cloud alternative (Twelve Labs Marengo, the only real cloud video-embedding option) would run ~$420/month at 10,000 min ingest, scaling linearly — self-hosting stays flat regardless of volume.

### Audio (your requirement: **local**, zero code today — distinct from existing Whisper transcription)
- **Azure:** no audio-similarity embedding product exists as of Aug 2026 (Speaker Recognition is voice-print verification, not general sound retrieval).
- **Local:** **LAION-CLAP** (193M params, sub-1GB VRAM, runs on CPU even) for a simple start; **GLAP** (855M params) if unified speech+music+sound-event coverage in one space is wanted.
- **Cost:** ~$60-270/month depending on volume, can share the same GPU box as video/image.

### PDF (your requirement: **ZDR**)
- **Azure Document Intelligence has no real ZDR posture and cannot be made to have one.** No attestable contract exists (no `AZURE_OPENAI_ZDR_CONFIRMED`-equivalent), and by default every analyze call stores the document + results in shared regional temp storage for up to 24 hours, reducible only by proactively calling a Delete-Result API after every request. That's best-effort minimal retention, not zero-retention-by-construction — it does not meet the bar this repo's own ZDR mechanism sets for Azure OpenAI.
- **Self-hosted is the only way to actually meet a hard ZDR bar here.** **Docling** (IBM Research, MIT) — DocLayNet layout + TableFormer tables, CPU-viable at low/moderate volume (no GPU spend required), with an optional Granite-Docling-258M VLM path for full document understanding on a fraction of ColQwen2.5's footprint.
- **Recommendation:** don't build the `DOCINTEL_ENDPOINT` integration `visual-rag-integration-plan.md` still lists as open — it would import exactly the gap you're trying to close.

### General documents (your requirement: **best quality**, cloud OK)
- **Azure:** stop defaulting to `text-embedding-3-large` — every real current alternative beats it. **Voyage-3.5** is GA on Azure AI Foundry (via the Marketplace listing) and clears `text-embedding-3-large` by a wide margin on RTEB (0.7571 vs 0.6567), with zero new vendor/ops integration since it's in the same Foundry tenant. **Octen-Embedding-8B** is the current #1 open-weight model on RTEB (0.8045) — worth a ticket to confirm its Foundry billing model before treating it as a near-term default.
- **Local (only if cost ever forces it):** Octen-Embedding-8B again (Apache-2.0, needs a 24GB-class GPU) — skip NV-Embed-v2/GTE-Qwen2/mxbai-embed-large, all measurably behind on current benchmarks.

---

## 4. The quantization system

**Don't build a general quantization system.** Build a small, scoped fix for the one real candidate
that exists (ColQwen2.5-v0.2), in this order:

1. **Fix the actual blocker first, unrelated to VRAM:** the checked-out venv's
   `colpali-engine==0.3.17` + `transformers==4.57.6` combination can't even import `app.py`
   (`ModernVBert` needs `transformers>=5`). This must be fixed regardless of any quantization work
   — recreate the venv per `requirements.txt`'s documented pin. ~Half a day.
2. **Check Hugging Face for an already-published quantized checkpoint** before writing anything —
   ColPali-family models frequently get community int8/ONNX quantizations. Might eliminate the need
   to build anything at all.
3. **If nothing exists:** `bitsandbytes` int8 (`load_in_8bit=True`) is the correct first cut — a
   ~1-line change in `app.py`'s model-loading branch, expected to roughly halve the ~7.5GB fp16
   footprint to ~3.75GB, clearing a 6GB card with real headroom. **Do not reach for ds4/GGUF-style
   tooling** — see below, wrong ecosystem entirely for this model.
4. **Seriously consider sidestepping quantization altogether:** `nvidia/llama-nemotron-embed-vl-1b-v2`
   + `rerank-vl-1b-v2` (~1.7B params each) **fit a 6GB card unquantized**, with a more permissive
   commercial license than ColQwen2.5's. This might be the actual right move rather than quantizing
   the current model.
5. **Architecturally:** this stays an offline, lab-scoped artifact-production step — the quantized
   checkpoint is just a different `MODEL_ID` the existing `colqwen-reranker` service loads. No new
   service, no conflict with "no independent embeddings/reranking outside isolated labs" (this
   reranker was never in the routed production path to begin with). If it ever graduates to
   production, the sanctioned integration point is a fourth client variant in
   `embedding-engine-rs/src/provider/mod.rs`, alongside the existing three.
6. **Honest sequencing check:** this reranker is undeployed, has zero test coverage of its actual
   reranking behavior, and was explicitly called out as a v1 anti-pattern in this plane's own
   planning docs. Fix the import bug → add real test coverage → *then* quantize. Otherwise you're
   optimizing VRAM footprint for a code path nobody can currently prove works at all.

### Your 8 sources, grounded (all fetched directly, not assumed)

- **NVIDIA quantization blog:** full PTQ/QAT taxonomy — formats (FP32/FP16/BF16/FP8), symmetric vs.
  affine schemes, per-tensor/channel/block granularity, and a genuinely load-bearing caveat: their
  own data shows **NF4 (4-bit) can increase energy 25-56% on sub-3B models** — ColQwen2.5's ~3B
  backbone sits right at that boundary, so don't jump straight to 4-bit without benchmarking.
- **HF Optimum quantization guide:** same theory, plus the four concrete backend packages
  (bitsandbytes, GPTQ, ONNX Runtime, OpenVINO) and named calibration techniques (min-max,
  moving-average min-max, histogram-based).
- **antirez/ds4 ("DwarfStar"):** **not a general quantizer.** A narrow inference engine built for
  exactly DeepSeek V4 Flash/PRO and GLM 5.2, that borrows GGUF/llama.cpp's quantization machinery.
  Wrong tool for a ColPali-family vision-language model — that ecosystem is decoder-only-LLM/MoE
  focused, not vision embedding.
- **antirez.com/news/165:** explains ds4's popularity as a coincidence (a large-enough open model +
  an extreme 2/8-bit asymmetric recipe fitting 96-128GB consumer RAM + AI-assisted dev speed) —
  explicitly says the project's future is "track whatever model is next," not become a general tool.
- **LlamaIndex embeddings module + repo:** a thin, pluggable `BaseEmbedding` abstraction
  (OpenAI/Cohere/Azure/`HuggingFaceEmbedding` with an `onnx`/`openvino` backend option) — a good
  **pattern** to mirror in `embedding-engine-rs`'s own provider abstraction, not code to adopt
  (adopting LlamaIndex itself would duplicate Data Plane v2's own retrieval stack).
- **HF Nemotron-VL-1B blog:** the real find — `llama-nemotron-embed-vl-1b-v2` (bi-encoder,
  Llama-3.2-1B + SigLIP2-400M, 2048-dim) and `rerank-vl-1b-v2` (cross-encoder), both ~1.7B params,
  fit 6GB **without any quantization work**. See point 4 above.
- **Wisprflow.ai / docs / notetaker:** **contradicts, rather than models, your "audio must be
  local" requirement** — it's a cloud-inference voice product with local-capture-only privacy
  (audio leaves the device for processing). Relevant as UX inspiration (notetaker's speaker-ID via
  calendar invite + personal dictionary is a nice pattern), not as a local-inference blueprint.

---

## Sources consulted

All 8 user-provided URLs (fetched directly), plus: Azure AI Foundry model catalog (image/video/audio/
document-intelligence/embedding surfaces, Aug 2026), RTEB leaderboard, Hugging Face model cards for
every model named above, and direct reads of: `embedding-engine-rs/src/provider/{mod,visual}.rs`,
`retrieval-engine-rs/src/pipeline/smart_mix.rs`, `colqwen-reranker/{app.py,README.md,requirements.txt}`,
`imports-core/app/parsers.py`, `inference-core/src/provider/{video,speech,zdr}.rs`,
`image_consumer.rs`, and `docs/{gap-data.md,visual-rag-integration-plan.md,retrieval-quality-and-durability-plan-2026-08-05.md}`.
