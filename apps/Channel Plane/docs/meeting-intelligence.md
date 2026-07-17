# Meeting Intelligence (future scope)

> **Status: docs-only.** Like the rest of Channel Plane, this is a plan, not a
> deployed runtime. Do not wire active product flows against it as if it exists.
> It is recorded here because meeting capture is a natural *external channel*
> (Teams / Zoom / Google Meet), so the capture surface belongs with Channel
> Plane's future adapter runtime — but the processing and storage deliberately
> reuse the existing planes rather than duplicating them.

## Purpose

Let an organization bring **meetings** (audio + video) into Velion so they can
be transcribed, documented, searched, and reasoned over — **without the meeting
content ever leaving the company**. Meetings are among the most sensitive data an
org holds, so on-prem / open-source processing is a hard requirement, not a
preference.

## Core principle: a meeting is a *source type*, not a new plane

The single most important design decision: **do not build a parallel meetings
stack.** A meeting decomposes into three artifacts the platform already ingests,
so ~80% of the pipeline is existing rails:

1. **Transcript → a document.** Each diarized segment
   (`speaker`, `t_start`, `t_end`, `text`) becomes a Data Plane
   `knowledge_unit` carrying timestamp + speaker metadata. It embeds and
   retrieves through the existing dense / sparse / graph fusion unchanged.
2. **Slides / keyframes → "page images."** Emitted on the existing
   `dataplane.page_images.created` event, they flow straight into the visual
   retrieval arm and the ColQwen late-interaction reranker. "Slide 14 at 24:12"
   falls out of `page_no` + the segment timestamp — no new visual path.
3. **People / decisions / action items → graph entities.** They ride the
   existing graph-index extraction → traverse → community path.

Retrieval, graph, wiki, ZDR propagation, and org-scoping are therefore **already
built**. Meetings feed them a new source; they do not re-architect them.

## Cross-plane ownership

Meeting intelligence is explicitly **not** owned by one plane. Each slice stays
with its rightful owner under the existing authority rules:

| Responsibility | Owner | New or reuse |
|---|---|---|
| Capture the AV (Teams/Zoom/Meet bot, or file upload) | **Channel Plane** adapter (future) → **Ingestion (Quarry)** for evidence submission; connector auth via integration-corev2 | New connector + `meeting` source kind |
| Raw AV storage (access-controlled) | Object store (MinIO) | Reuse |
| ASR + diarization (audio → speaker-labeled, timestamped transcript) | **Model Plane** — inference is centralized; add an ASR capability/provider behind the inference contract | New self-hosted provider |
| Keyframe / slide extraction (video → frames on slide change) | **Ingestion (Quarry vision)** — already emits page images | Mostly reuse |
| Chunk + embed + index + retrieve | **Data Plane v2** via existing contracts | Reuse |
| Entity / decision / action-item graph | **Data Plane** graph-index | Reuse |
| Meeting minutes / summary | **Model Plane** reasoning → written back as a **wiki page** via Data Plane | Reuse |
| Org scope + per-user visibility + ZDR posture | **Control Plane** (private-until-shared authz — meetings are sensitive by default) | Reuse |
| Live transcription / notifications / operator inbox | **Application Plane** + Channel Plane realtime | Reuse |

Boundary rule reminder: Channel Plane may *capture and route*, but it persists
knowledge only through Data Plane contracts, and it embeds/reasons only through
Model Plane contracts. No independent embedding or storage in the channel layer.

## What is genuinely new (three components)

1. **Meeting capture connector** — a join-and-record bot (or file upload) that
   lands the AV in object storage and emits a `meeting` capture event. Lives in
   the Channel Plane adapter runtime (future) feeding Quarry.
2. **Keyframe extractor** — slide-change / scene detection over the video track
   (FFmpeg + perceptual-hash diff) producing page-image events.
3. **Self-hosted ASR provider** — the sovereignty piece (below), exposed as a
   Model Plane capability so ZDR, cost accounting, and capability governance all
   stay in one place.

## Sovereign / on-prem model stack (the "never leaves the company" requirement)

This maps cleanly onto the platform's existing ZDR axis: **meetings default to a
restrictive ZDR posture**, and the platform's ZDR guards already *fail closed on
retaining providers*. So the compliance requirement is enforced by routing
meetings through self-hosted providers the guards already permit — not by new
bespoke controls.

> Model choices move fast — treat the specific names below as the 2026-07
> starting point and confirm current best-in-class at build time (mirror the
> pinning discipline in `services/colqwen-reranker/requirements.txt`).

- **ASR:** `faster-whisper` (CTranslate2, fast) or **WhisperX** (adds word-level
  timestamps + VAD + forced alignment — word-level is what makes precise
  "24:12" citations possible) on **Whisper large-v3** / `large-v3-turbo`. Fully
  self-hostable; can share the GPU box with ColQwen.
- **Diarization (who-said-what):** `pyannote.audio` (best quality, but pretrained
  weights are gated — needs an HF token, still runs locally), or **NVIDIA NeMo** /
  `sherpa-onnx` for a no-gating, fully-open path.
- **Summary / minutes / action items:** a self-hosted open LLM (Llama / Qwen)
  reached through the Model Plane `FallbackChain` when ZDR is restrictive — this
  is open-decision **D-C** in Data Plane's `sovereign-rag-phased-plan.md`
  ("self-host reasoning so the boundary isn't crossed").
- **Slide / keyframe dense embedding:** today's page-image first stage is
  Cohere Embed v4 (a *retaining* provider), so it is **skipped under restrictive
  ZDR**. A fully-sovereign meeting path therefore needs a **self-hosted
  page-image embedder** (e.g. a GME-Qwen2-VL-class multimodal embedder) — this is
  open-decision **D-B/D-C**. ColQwen (already self-hosted) covers the rerank
  stage.
- **Native audio retrieval (optional, later):** if non-speech audio matters
  (tone, applause, overlapping speakers, music), a self-hosted audio embedder
  (CLAP-class) can add an audio arm. For speech-first meetings the transcript
  route is sufficient and far cheaper.

Net: audio never leaves the box; the only derived artifacts are the transcript
and keyframes; the raw AV is retained so every citation can replay the exact
moment. Nothing lossy that matters — the raw recording is always the ground truth
behind the searchable derivatives.

## Honest note on the current embedding system

The current stack is **strong for visual document retrieval** (Embed v4 dense +
ColQwen MaxSim rerank over page images — near state-of-the-art for slides/PDF
pages) but it has **no audio/sound retrieval at all** today, and its visual first
stage (Embed v4) is a *retaining cloud provider*, so it is not sovereign for
meetings without the self-hosted embedder above. For meetings specifically:

- **Visual (slides/keyframes):** reuse as-is for the non-sovereign case; add a
  self-hosted page-image embedder for the ZDR-restrictive case.
- **Sound:** there is nothing to reuse — audio retrieval is transcript-mediated
  (ASR → text → text embedding) unless/until a native audio arm is added.

So the current system is an excellent *visual document* engine and a *non-existent
sound* engine; meetings need the ASR path (and optionally an audio arm) layered on
top, all self-hosted.

## Open decisions (confirm before building)

- **Diarization required?** If who-said-what is mandatory, pyannote's gating vs a
  fully-open diarizer must be chosen up front.
- **Live vs post-meeting?** Streaming transcription (Application Plane realtime)
  vs batch changes the ASR configuration; batch is the simpler MVP.
- **Sovereignty depth?** Whether the org's ZDR posture forces the self-hosted
  page-image embedder + self-hosted reasoning from day one, or whether a
  non-restrictive tier may use the existing Embed v4 + cloud reasoning path.

## Phasing

1. **MVP:** file-upload capture → WhisperX ASR (Model Plane provider) → transcript
   document + segment chunks → retrieval works over existing rails. No video.
2. **Slides:** keyframe extractor → page-image events → visual arm + ColQwen light
   up for "show me the slide."
3. **Documentation:** self-hosted LLM minutes → wiki page + graph action-items,
   HITL on anything that triggers a task.
4. **Live + channels:** Application Plane realtime transcription + Channel Plane
   Teams/Zoom/Meet connector bots.
