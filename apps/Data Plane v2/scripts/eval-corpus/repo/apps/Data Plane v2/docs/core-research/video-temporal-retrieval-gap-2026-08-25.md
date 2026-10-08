# The video temporal-retrieval gap — research, 2026-08-25

## The question

`media-embedder`'s video arm cannot retrieve motion or direction. "Someone
falling" and "someone standing up" are the same video to it. This was flagged
when LanguageBind was replaced by SigLIP 2 frame pooling, on the reasoning that
mean-pooling is order-insensitive by construction. The question was: what is the
best way to close that gap?

## Headline answer

**A tower swap cannot close it.** No off-the-shelf video-text *bi-encoder* in the
published literature distinguishes a video from its own reverse. This is not a
consequence of choosing frame pooling — it is where the field is.

The realistic fix is architectural, and the cheapest option routes around vision
entirely: **caption the video with a video-LLM and index the caption as text**,
which turns a temporal-vision problem into a text-retrieval problem this system
is already very good at.

## Evidence

### The right way to measure it

The gate is a **reversal test**: embed a video and its exact frame reversal and
check whether the vectors differ. The frame *sets* are identical, so anything
order-blind returns the same vector.

This is not a homemade idea. **RTime — "Reversed in Time" (ACM MM 2024)** is the
published benchmark built on exactly this construction: take videos whose action
is temporally meaningful, reverse them to manufacture hard negatives, 21k videos
/ 10 captions each / ~122 hours, split into RTime-Origin, RTime-Hard and
**RTime-Binary** (literally: video vs. its reverse). Its stated motivation is
that existing video-text benchmarks are so weak on temporality that
*image-text* pretrained models already match *video-text* pretrained ones
zero-shot.

**Any candidate tower must be held to this before a temporal claim is made.**

### Published RTime-Binary results — everything is at chance

| model | T2V | V2T |
|---|---|---|
| CLIP | 49.1% | 49.5% |
| UMT | 49.8% | 50.4% |
| InternVideo2-1B | 50.0% | 51.0% |

50% is chance on a binary task. **InternVideo2-1B** matters most here: it is a
video-native dual encoder trained on 50M video-text pairs, and it was the
obvious "swap to a real video tower" candidate. It is at chance.

### Local measurements (this repo, transformers 4.57)

Reversal test on ffmpeg-generated videos, run through the live service:

| tower | cos(video, reversed video) |
|---|---|
| SigLIP 2 frame pooling | 1.000000 |
| X-CLIP `video_embeds` | 1.000000 |
| X-CLIP MIT pooler (its "multiframe" module) | 1.000000 |
| plain mean of frame projections | 1.000000 |

Control: the same harness separates *different content* (horizontal vs vertical
motion) at cos 0.997, so it is not a degenerate test. X-CLIP's MIT pools
order-invariantly in practice despite carrying a `position_embedding` parameter.

End-to-end through the running service on real mp4s (a box crossing
left-to-right vs. the same clip reversed): SigLIP 2 gives **cos 0.999118**. The
frame-level tests above give exactly 1.000000; real files differ slightly
because ffmpeg samples marginally different frames from each. Either way there
is no usable margin.

> A note on method, because it bit us: an earlier version of this test built its
> clips with `ffmpeg -vf "drawbox=...:t=fill"`. The `t` parameter collides with
> the time variable used in the `x` expression, ffmpeg silently drew nothing, and
> the "test" compared two identical all-black videos — cos 1.000000 for a reason
> that had nothing to do with the model. Generate frames explicitly (PIL) and
> encode from an image sequence, then ASSERT the motion is present (e.g. the
> bright-pixel centroid moves) before trusting any number.

### X-CLIP is additionally unusable as a retrieval arm

Independent of temporality, X-CLIP cannot serve a vector index. Its forward
computes

```python
text_embeds = text_embeds + self.prompts_generator(text_embeds, img_features)
```

where `img_features` derives from the **candidate video**. So the text vector a
video is matched against is a function of that video:

* the same query embedded against two different videos: cos **0.998311** (≠ 1.0)
* `get_text_features` (the un-prompted vector) vs the prompted one: cos **0.9439**

There is therefore no precomputable query vector. X-CLIP is a **cross-encoder**.
`/embed/text` fails closed on that backend rather than returning the 0.94-wrong
vector, which would have looked like it worked.

## Candidates considered

| candidate | bi-encoder? | passes reversal? | practical |
|---|---|---|---|
| SigLIP 2 frame pooling (current) | yes | **no** (measured 1.000000) | CPU, 768-dim ✓ |
| X-CLIP | **no** (video-conditioned text) | no (measured) | CPU but unusable as an arm |
| InternVideo2-1B | yes | **no** (published: 50.0%) | GPU |
| LanguageBind | yes | untested; cannot import at all | dead deps |
| GVE-3B / GVE-7B (Alibaba, Apache-2.0) | yes (embedder) | **unknown** | ~4B params, GPU |
| MobileViCLIP (ICCV 2025) | yes | unknown | efficient/mobile |
| VidVec / ViLL-E / VeRVE (2026, MLLM embedders) | yes | unknown | large |

On GVE specifically — it is the strongest open-weights video *embedder*
available (SOTA zero-shot on its own UVRB benchmark, Qwen2.5-VL base, LoRA,
Apache-2.0) and it does report the best "Temporal" score of its cohort
(**T = 0.469** vs Unite-7B 0.412). But **UVRB's "Temporal" dimension measures
event dynamics** — fine-grained temporal description (CaReBench-Temporal) and
camera motion (CameraBench) — **explicitly not reverse-video discrimination**. So
that number is *not* evidence it passes the gate. Treating it as such would
repeat the X-CLIP mistake.

Hardware note: GVE-3B at bf16 is roughly 8 GB of weights. The GPU available to
this project is an RTX A1000 (6 GB) — the same card that forced ColQwen to nf4
and a top-K of 3 at ~10 s/page. GVE is not realistically deployable for indexing
a video corpus on current hardware.

## Recommended path

**1. Caption-to-text (IMPLEMENTED — see below).** Run a video-LLM or Azure AI
Content Understanding's video analyzer over each video, and index the resulting
description into the *existing* text arms. Motion becomes retrievable because the
caption literally says "the person falls" — order is expressed in language, where
this system's retrieval is strongest, and stronger still since contextual
embeddings and contextual BM25 both landed. Notably, the 2026-08-19 modality
audit *rejected* Content Understanding for the video arm because it is
"extraction/description, not a dense embedding model" — correct for building an
embedding arm, and precisely why it fits here instead. No GPU, no new vector
space, no new collection.

**2. Keep SigLIP 2 for visual recall.** It is doing the job it is good at:
what appears on screen. Both arms then fuse through the existing RRF.

**3. Two-stage rerank (only if 1 proves insufficient).** Bi-encoder recall, then
an order-aware cross-encoder over the top-K. X-CLIP already fits that shape
mechanically. Validate first: RTime-QA (2505.19125) reports that even large
multimodal models struggle on atomic temporal events, so a reranker is not
automatically order-aware either.

**4. Do NOT swap towers expecting motion retrieval.** The published numbers say
it will not help, and it costs a GPU budget.

## If a candidate is ever evaluated

Reproduce the gate before anything else — it is cheap and decisive:

1. Take any video with directional action; produce `ffmpeg -vf reverse` of it.
2. Embed both through the candidate's *bi-encoder* path (a precomputable query
   vector — not a pairwise forward).
3. `cos ≈ 1.0` → order-blind. Stop; it does not close the gap.
4. Only if it separates them, evaluate properly on RTime-Binary.

`/healthz` reports `order_sensitive` per backend so a deployment's real
capability is observable rather than inferred from a model name. It is `false`
for both current towers, and that is measured, not assumed.

## What was built (caption-to-text)

Shipped, OFF by default behind `VIDEO_CAPTION_ENABLED`.

```
video segment event
  → media-embedder POST /filmstrip/video      # ffmpeg samples N frames,
  │                                            # tiles them in reading order,
  │                                            # stamps each with its timestamp
  → Model Plane AnalyzeImage (one call)        # "describe what HAPPENS, in order"
  → knowledge_units row                        # text = "[video 2.0s–6.5s] <narrative>"
      ├─ BM25 arm     : immediate (content_tsv is a generated column)
      └─ dense text arm: vector upserted into QDRANT_COLLECTION
```

Design points worth keeping:

* **One vision call per segment, not per frame.** The filmstrip is a single
  labelled sheet, so the model sees the progression instead of describing frames
  in isolation. Verified: a forward clip and its reverse produce filmstrips that
  differ in **10.8% of pixels**, which is the precondition for the captions to
  differ at all.
* **Order is asked for explicitly.** The prompt states the tiles are
  time-ordered and requires ordering words ("then", "after"). A description that
  merely listed contents would reproduce the bag-of-frames failure in text.
* **Captions are namespaced, never confused with real chunks.** Deterministic
  `{document_id}:vidcap{segment_no}` ids and *negative* `chunk_index`, so they
  cannot collide with index-engine's 0..n text chunks, and a redelivery upserts
  in place instead of accumulating duplicates.
* **This is the one egressing path in the media arm.** The module header's "no
  ZDR guard needed, nothing egresses" claim is now explicitly scoped:
  `caption_allowed_for_document` reads `zdr_classification` from the `documents`
  row (not the event, so a forged event cannot downgrade it) and fails closed on
  a missing or unreadable row.
* **Failure is partial at worst.** Every step degrades to "segment keeps its own
  vector, no caption". A vision-provider outage cannot fail a media ingest, and a
  failed *embed* still leaves the caption live on the lexical arm.
* **Additive, not a replacement.** SigLIP 2 still embeds the segment for visual
  recall; the caption adds a text channel. Both fuse through the existing RRF.

Not solved by this, and worth being clear about: the caption is only as good as
the vision model's description, and it inherits that model's blind spots. It
makes motion *expressible* — it does not guarantee the right words. The honest
test after enabling it is whether "someone falling" now retrieves the right
segment, not whether a caption exists.

## Sources

- [Reversed in Time: A Novel Temporal-Emphasized Benchmark for Cross-Modal Video-Text Retrieval (RTime)](https://arxiv.org/abs/2412.19178)
- [RTime-QA: Atomic Temporal Event Understanding in Large Multi-modal Models](https://arxiv.org/pdf/2505.19125)
- [Towards Universal Video Retrieval: GVE + UVRB](https://arxiv.org/abs/2510.27571)
- [Alibaba-NLP/GVE-3B](https://huggingface.co/Alibaba-NLP/GVE-3B) · [GVE-7B](https://huggingface.co/Alibaba-NLP/GVE-7B)
- [InternVideo2: Scaling Foundation Models for Multimodal Video Understanding](https://arxiv.org/html/2403.15377v4)
- [VidVec: Unlocking Video MLLM Embeddings for Video-Text Retrieval](https://arxiv.org/abs/2602.08099)
- [MobileViCLIP (ICCV 2025)](https://openaccess.thecvf.com/content/ICCV2025/papers/Yang_MobileViCLIP_An_Efficient_Video-Text_Model_for_Mobile_Devices_ICCV_2025_paper.pdf)
