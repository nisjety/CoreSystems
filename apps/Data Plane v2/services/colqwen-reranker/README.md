# colqwen-reranker

Visual reranker for the Data Plane v2 retrieval pipeline. Late-interaction
(MaxSim) reranking of Embed-v4's top-K **page-image** candidates using
**ColQwen2.5**. The retrieval-engine calls it over HTTP
(`src/search/colqwen.rs`); `VISUAL_RERANK_ENABLED` defaults ON but the client
is a no-op until `COLQWEN_ENDPOINT_URL` points at a running instance.

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
Run natively against MPS (versions are PINNED in requirements.txt — the
colpali-engine/transformers pair breaks at import when mismatched; see the
comment block in that file):
```bash
cd services/colqwen-reranker
python3 -m venv .venv && source .venv/bin/activate
pip install 'torch==2.6.0' 'torchvision==0.21.0'   # native arm64 MPS wheels
pip install -r requirements.txt
python app.py                     # serves :8090 on MPS (slow but real)
```
The retrieval-engine (in Docker) reaches the host server via
`COLQWEN_ENDPOINT_URL=http://host.docker.internal:8090`.

Model is downloaded from HF on first start (~7 GB, cached under
`~/.cache/huggingface`).

### Image-URL reachability (host-run server)
Page-image candidates carry `image_url` values hosted by the producer
(Quarry's artifact store / MinIO), which are **Docker-network-internal**. A
server running on the HOST cannot resolve those hostnames — set
`COLQWEN_URL_REWRITE="<internal-host:port>=<host-reachable:port>"`
(comma-separate multiple) once visual ingest is live, and make sure the
artifact endpoint has a host-published port. In-cluster (GPU box on the same
network or with routable URLs) no rewrite is needed.
