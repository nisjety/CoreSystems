Qdrant HNSW/IVF Tuning Checklist

Goal: reduce vector search latency while keeping recall acceptable.

1) Measure baseline
- Collect a representative set of queries and ground-truth nearest neighbors.
- Measure average latency and recall@k for current settings.

2) Key parameters
- `m` (HNSW connectivity): lower m → faster, less memory; typical 8-48.
- `ef` (query time effort): higher ef → better recall, slower; tune per-query or globally.
- `ef_construct`: affects index build time and quality; set during indexing.
- `index_type` / `quantization`: experiment with quantization (if using disk/IVF) for memory vs latency tradeoffs.

3) Tuning approach
- Start with increasing `ef` gradually until recall stabilizes, then reduce `m` to save memory.
- Benchmark latency vs recall curve and choose operating point.
- Consider per-tenant indexing configurations if access patterns vary.

4) Operational notes
- When changing HNSW params, reindexing is required.
- Use `collection.consolidate` and monitor Qdrant metrics (CPU, disk IO).
- For high QPS, prefer in-memory indices with adequate RAM and tune `ef` per request.

5) Quick commands
- Update index config via Qdrant management APIs or client SDKs.
- Use sample scripts to run batched queries and compute recall/latency.

6) Monitoring
- Track `search_latency_ms`, `requests_per_second`, and `memory_usage`.
- Alert when latency regresses or recall drops below threshold.

Reference: qdrant docs and HNSW literature.
