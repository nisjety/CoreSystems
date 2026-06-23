# colqwen-reranker

Visual reranker for the Data Plane v2 retrieval pipeline. Late-interaction
(MaxSim) reranking of Embed-v4's top-K **page-image** candidates using
**ColQwen2.5 / Qwen3-VL-Embedding**. The retrieval-engine calls it over HTTP
(`src/search/colqwen.rs`); it is OFF by default and gated by
`VISUAL_RERANK_ENABLED` + `COLQWEN_ENDPOINT_URL`.

## Contract
```
POST /rerank  { "query": str, "image_urls": [str, ...] } -> { "scores": [float, ...] }
GET  /healthz -> { "ok": true, "model": ..., "device": ... }
```
`scores[i]` is the MaxSim relevance of `image_urls[i]` to `query`, in order.

## Production (GPU: Hetzner GEX44 / Azure NV once quota lands)
```bash
docker build -t colqwen-reranker .
docker run --gpus all -p 8090:8090 \
  -v hf-cache:/root/.cache/huggingface \
  -e COLQWEN_MODEL=vidore/colqwen2.5-v0.2 \
  colqwen-reranker
```
Then on the retrieval-engine: `VISUAL_RERANK_ENABLED=true`,
`COLQWEN_ENDPOINT_URL=http://<gpu-host>:8090`.

## Local verification (Apple Silicon / MPS — no Docker GPU passthrough)
Run natively against MPS:
```bash
cd services/colqwen-reranker
python3 -m venv .venv && source .venv/bin/activate
pip install torch                 # native arm64 MPS wheel
pip install -r requirements.txt
python app.py                     # serves :8090 on MPS (slow but real)
```
The retrieval-engine (in Docker) reaches the host server via
`COLQWEN_ENDPOINT_URL=http://host.docker.internal:8090`.

Model is downloaded from HF on first start (~3–7 GB, cached under
`~/.cache/huggingface`).
