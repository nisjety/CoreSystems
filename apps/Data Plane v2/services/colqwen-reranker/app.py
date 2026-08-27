"""ColQwen visual reranker — late-interaction (MaxSim) over page images.

A thin, host-independent inference server: the retrieval-engine POSTs a query +
the candidate page-image URLs (Embed-v4's top-K) and gets one MaxSim relevance
score per image back. The SAME image runs locally (Apple MPS, for verification)
and on a GPU box (CUDA, Hetzner/Azure) for production — only the device differs.

Contract (matches services/retrieval-engine-rs/src/search/colqwen.rs):
    POST /rerank  { "query": str, "image_urls": [str, ...] }
               -> { "scores": [float, ...] }   # one MaxSim score per url, in order
    GET  /healthz -> { "ok": true, "model": ..., "device": ... }
"""

import concurrent.futures
import io
import os

import httpx
import torch
from colpali_engine.models import ColQwen2_5, ColQwen2_5_Processor
from fastapi import FastAPI
from PIL import Image
from pydantic import BaseModel

MODEL_ID = os.environ.get("COLQWEN_MODEL", "vidore/colqwen2.5-v0.2")
PORT = int(os.environ.get("PORT", "8090"))

# Optional host rewrite for fetching page images. The retrieval-engine emits
# Docker-internal URLs (e.g. http://quarry-edge:8082/...); a server running
# OUTSIDE that network (local host, or a Hetzner box) can't resolve them. Set
# COLQWEN_URL_REWRITE="quarry-edge:8082=localhost:8082" (comma-separate multiple)
# so this server can reach them. Empty in-cluster (no rewrite needed).
_URL_REWRITES = [
    tuple(p.split("=", 1))
    for p in os.environ.get("COLQWEN_URL_REWRITE", "").split(",")
    if "=" in p
]

# Device/precision: CUDA→bf16 (prod GPU), Apple MPS→fp16 (local verify — fp32
# was tried first but the model's single weight buffer is 15.15 GB in fp32,
# above MPS's max buffer size, so the load hard-fails; fp16 halves it and is
# the standard ColQwen inference precision), else CPU fp32. Override with
# COLQWEN_DTYPE=float16|bfloat16|float32 when the hardware disagrees.
_DTYPES = {
    "float16": torch.float16,
    "bfloat16": torch.bfloat16,
    "float32": torch.float32,
}
if torch.cuda.is_available():
    DEVICE, DTYPE = "cuda", torch.bfloat16
elif getattr(torch.backends, "mps", None) and torch.backends.mps.is_available():
    DEVICE, DTYPE = "mps", torch.float16
else:
    DEVICE, DTYPE = "cpu", torch.float32
DTYPE = _DTYPES.get(os.environ.get("COLQWEN_DTYPE", "").strip().lower(), DTYPE)

# Attention kernel. This was "eager" — chosen because flash-attn is CUDA-only —
# but eager was the actual reason this service could not run on a small GPU, and
# the weight footprint was only ever a secondary problem. Eager materializes the
# vision tower's full [heads, seq, seq] score matrix and transformers upcasts
# that softmax to fp32 (modeling_qwen2_5_vl.py: `softmax(..., dtype=float32)`),
# so four A4 pages at 2944 patches each = 11.8k patches becomes ONE ~8.3 GiB
# allocation — measured live on an RTX A1000 (6 GB): it OOMs inside the ViT even
# with 4-bit weights and 1.35 GiB of VRAM still free. `sdpa` uses PyTorch's
# fused/memory-efficient kernels, never materializes that matrix, and — unlike
# flash-attn — is supported on CUDA, MPS and CPU alike. It is therefore a strict
# improvement over eager on every device this service targets, not a CUDA-only
# optimization. eager stays reachable as a debugging escape hatch.
ATTN_IMPL = os.environ.get("COLQWEN_ATTN_IMPL", "").strip() or "sdpa"

# Optional weight quantization (CUDA only — bitsandbytes has no MPS/CPU backend).
# Needed on cards that cannot hold 3.815 B params in 16-bit (7.11 GiB). On a 6 GB
# card the bf16 load does NOT fail cleanly: the Windows/WDDM driver silently
# backs the ~1.15 GiB overflow with host RAM, so the model appears to load and
# then streams weights over PCIe on every forward pass — measured 177 s per
# 4-page rerank versus 35 s for nf4, a 5x penalty for a "successful" load. nf4
# brings the weights to 3.39 GiB, fully resident in real VRAM.
#
# The vision tower and the 128-dim projection head stay unquantized: the ViT is
# what reads the page pixels and `custom_text_proj` emits the multivectors whose
# MaxSim IS the score, so quantizing those two trades ranking quality for ~1.3
# GiB this service does not need. NOTE: quantization is verified here to RUN, not
# to preserve ranking fidelity — that needs a ViDoRe-style eval against pages
# with known relevance, which has not been done.
QUANT = os.environ.get("COLQWEN_QUANT", "").strip().lower()
_quant_config = None
if QUANT in ("nf4", "int8"):
    if DEVICE != "cuda":
        raise RuntimeError(
            f"COLQWEN_QUANT={QUANT} requires CUDA; bitsandbytes has no {DEVICE} backend"
        )
    from transformers import BitsAndBytesConfig

    _skip = ["visual", "custom_text_proj"]
    if QUANT == "nf4":
        _quant_config = BitsAndBytesConfig(
            load_in_4bit=True,
            bnb_4bit_quant_type="nf4",
            bnb_4bit_compute_dtype=DTYPE,
            bnb_4bit_use_double_quant=True,
            llm_int8_skip_modules=_skip,
        )
    else:
        _quant_config = BitsAndBytesConfig(
            load_in_8bit=True, llm_int8_skip_modules=_skip
        )
elif QUANT not in ("", "none"):
    raise RuntimeError(f"COLQWEN_QUANT must be nf4|int8|none, got {QUANT!r}")

# How many page images go into a single model call. This used to be unbounded —
# every candidate was processed in one batch — but the ViT's attention cost grows
# with the square of the TOTAL patch count in the batch, so at
# visual_rerank_top_k=20 (the retrieval-engine's default) that is one ~59k-patch
# problem. Per-image scores are independent, so chunking changes no result, only
# peak memory: measured 4.37 GiB at batch=4 versus 3.65 GiB at batch=1 for the
# same request. Raise it on a large GPU where throughput beats headroom.
IMAGE_BATCH = max(1, int(os.environ.get("COLQWEN_IMAGE_BATCH", "2")))

print(
    f"[colqwen] loading {MODEL_ID} on {DEVICE} ({DTYPE}, attn={ATTN_IMPL}, "
    f"quant={QUANT or 'none'}, image_batch={IMAGE_BATCH})",
    flush=True,
)
_load_kwargs = {
    "torch_dtype": DTYPE,
    "device_map": DEVICE,
    "attn_implementation": ATTN_IMPL,
}
if _quant_config is not None:
    _load_kwargs["quantization_config"] = _quant_config
model = ColQwen2_5.from_pretrained(MODEL_ID, **_load_kwargs).eval()
processor = ColQwen2_5_Processor.from_pretrained(MODEL_ID)
print(f"[colqwen] ready on {DEVICE}", flush=True)

app = FastAPI(title="colqwen-reranker")


class RerankRequest(BaseModel):
    query: str
    image_urls: list[str]


@app.get("/healthz")
def healthz():
    return {
        "ok": True,
        "model": MODEL_ID,
        "device": DEVICE,
        "attn": ATTN_IMPL,
        "quant": QUANT or "none",
    }


# A single pooled client reused across requests/threads (httpx.Client is safe to
# share) — avoids per-image connection setup and keep-alive teardown.
_HTTP = httpx.Client(timeout=30.0, follow_redirects=True)

# Cap fan-out so a large candidate set can't spawn one thread per URL.
_FETCH_MAX_WORKERS = int(os.environ.get("COLQWEN_FETCH_WORKERS", "8"))


def _fetch_image(url: str) -> Image.Image:
    for frm, to in _URL_REWRITES:
        url = url.replace(frm, to)
    r = _HTTP.get(url)
    r.raise_for_status()
    return Image.open(io.BytesIO(r.content)).convert("RGB")


@app.post("/rerank")
def rerank(req: RerankRequest):
    if not req.image_urls:
        return {"scores": []}
    # Fetch candidate images concurrently instead of serially: at
    # visual_rerank_top_k=20 the old list-comprehension issued 20 blocking GETs
    # (each up to the 30s timeout) back-to-back on the visual-rerank hot path.
    # ThreadPoolExecutor.map preserves input order (scores map back by index) and
    # re-raises the first fetch error, keeping the previous fail-on-error contract.
    workers = max(1, min(_FETCH_MAX_WORKERS, len(req.image_urls)))
    with concurrent.futures.ThreadPoolExecutor(max_workers=workers) as pool:
        images = list(pool.map(_fetch_image, req.image_urls))
    with torch.no_grad():
        bq = processor.process_queries([req.query]).to(model.device)
        q_emb = model(**bq)
        # Chunked so peak memory tracks IMAGE_BATCH rather than the caller's
        # top-K. Each image's multivector is independent of the others sharing
        # its batch, so concatenating per-chunk results reproduces exactly what
        # one all-at-once batch produced.
        chunks = [
            model(
                **processor.process_images(images[i : i + IMAGE_BATCH]).to(model.device)
            )
            for i in range(0, len(images), IMAGE_BATCH)
        ]
        i_emb = chunks[0] if len(chunks) == 1 else torch.cat(chunks, dim=0)
        # [n_queries, n_images] late-interaction MaxSim scores.
        scores = processor.score_multi_vector(q_emb, i_emb)
    return {"scores": scores[0].float().cpu().tolist()}


if __name__ == "__main__":
    import uvicorn

    uvicorn.run(app, host="0.0.0.0", port=PORT)
