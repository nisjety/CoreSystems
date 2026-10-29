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

print(f"[colqwen] loading {MODEL_ID} on {DEVICE} ({DTYPE})", flush=True)
model = (
    ColQwen2_5.from_pretrained(
        MODEL_ID,
        torch_dtype=DTYPE,
        device_map=DEVICE,
        # flash-attn is CUDA-only; eager keeps MPS/CPU working.
        attn_implementation="eager",
    )
    .eval()
)
processor = ColQwen2_5_Processor.from_pretrained(MODEL_ID)
print(f"[colqwen] ready on {DEVICE}", flush=True)

app = FastAPI(title="colqwen-reranker")


class RerankRequest(BaseModel):
    query: str
    image_urls: list[str]


@app.get("/healthz")
def healthz():
    return {"ok": True, "model": MODEL_ID, "device": DEVICE}


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
        bi = processor.process_images(images).to(model.device)
        i_emb = model(**bi)
        # [n_queries, n_images] late-interaction MaxSim scores.
        scores = processor.score_multi_vector(q_emb, i_emb)
    return {"scores": scores[0].float().cpu().tolist()}


if __name__ == "__main__":
    import uvicorn

    uvicorn.run(app, host="0.0.0.0", port=PORT)
