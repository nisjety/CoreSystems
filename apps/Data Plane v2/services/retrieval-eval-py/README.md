# retrieval-eval-py

Offline retrieval evaluation for Data Plane v2. **Lab only** — per
`apps/master-ownership-matrix.md`, Python in this plane is "RAG evals,
chunking/rerank experiments, offline scoring only" and never on the serving hot
path.

## Why this exists

Before this, retrieval quality was scored by a proxy:

```go
recall = min(candidates/10, 1.0)
ndcg   = recall * 0.9
```

That measures *how many candidates came back*, not whether any of them were
right. `eval_golden_judgments` existed with the real metric path already wired
(`data-quality-go/internal/eval/golden.go` → `goldenMetrics`, labelling results
`metric_source: golden` vs `proxy`) but held **0 rows**, so every reported
number was the proxy. Plan item **P0.5**; it gates every relevance change in
P1/P2.

## Method: known-item retrieval

`seed_golden_set.py` builds a *known-item* set: for each selected chunk it
derives a query from that chunk's distinctive vocabulary and records the
chunk's `knowledge_id` as the relevant answer.

**What this measures honestly:** "given a question drawn from chunk X, does the
retriever return chunk X in the top 10." That is a real recall@10 / nDCG@10 /
MRR signal and a **floor**, not a ceiling.

**Known limitations — do not overstate results:**

- Other chunks may also legitimately answer the query but are not labelled, so
  precision is *not* measured and recall is conservative.
- Queries are derived from corpus vocabulary, so they favour lexical overlap.
  Use them to detect regressions, not to prove semantic quality.
- This is a bootstrap. Human or LLM-adjudicated judgments should replace it, and
  the eval dimensions worth adding are **consistency** (rephrasings agree),
  **false recall** (confident answers with no supporting chunk — the one that
  matters most for a citations product), and **context-rot resistance**.

## Usage

```bash
python3 services/retrieval-eval-py/seed_golden_set.py --dry-run
```

Prints the generated queries for review. Add `--apply` to upsert into
`eval_golden_judgments` (idempotent — `ON CONFLICT (org_id, query_norm)`).

Query normalisation deliberately mirrors
`data-quality-go/internal/eval/golden.go:23`
(`strings.Join(strings.Fields(strings.ToLower(q)), " ")`); a mismatch here means
judgments silently never join to traces.

## Running the scored eval

`data-quality-go` owns execution (`POST /v1/evals/run`, `POST /v1/evals/golden`).
It requires a verified `aud=data-plane` bearer — `CONTROL_PLANE_ENFORCEMENT` is
strict and the internal-API-key path no longer authenticates. See the
"blocked" notes in
`docs/retrieval-quality-and-durability-plan-2026-08-05.md`.
