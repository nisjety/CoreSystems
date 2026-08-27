"""Retrieval A/B with a configurable arm mix — the hybrid counterpart to run_eval.py.

run_eval.py pins mode_mix to dense-only because, at the time it was written, the
contextual org's Quickwit index carried ~4x duplicate chunks (Quickwit's
delete_by_query is asynchronous and never applied before the re-index), which
would have handicapped the contextual side on any sparse-fused comparison.

That handicap is gone: the sparse arm now de-duplicates by knowledge_id
(retrieval-engine-rs/src/search/sparse.rs::dedup_hits), so a duplicated index
yields top_k DISTINCT chunks. This script therefore measures what run_eval.py
deliberately could not — contextual retrieval's OTHER half, contextual BM25,
which needs context_body populated in Quickwit.

Emits raw per-query results as JSON so measure.py scores them; the data-quality
HTTP eval surface 401s the same token retrieval-engine accepts, so scoring stays
local rather than routed through a path known to be broken.

Env:
  EVAL_ORG          org to query
  EVAL_TOKEN_FILE   bearer token path
  EVAL_GOLDEN_FILE  golden judgments path
  W_DENSE, W_BM25   arm weights (default 1.0 / 0.4)
  OUT_FILE          where to write per-query results
"""
import json, os, sys, time, urllib.request, urllib.error

ORG = os.environ.get("EVAL_ORG", "org-corpus-baseline")
RETRIEVAL = "http://retrieval-engine:8004"
LABEL = sys.argv[1] if len(sys.argv) > 1 else "hybrid"
TOKEN_FILE = os.environ.get("EVAL_TOKEN_FILE", "/work/eval-token.txt")


def token():
    """Re-read per query: a rerank-on cell (87 queries at ~4s each) outlives
    the 5-minute internal-token TTL, so the study script re-mints into the
    same file mid-cell and the runner must pick the fresh one up."""
    return open(TOKEN_FILE).read().strip()
_ALL = json.load(open(os.environ.get("EVAL_GOLDEN_FILE", "/work/golden.json"), encoding="utf-8"))
OUT_FILE = os.environ.get("OUT_FILE", f"/work/hybrid-{LABEL}.json")
QUIET = os.environ.get("QUIET", "0") == "1"
# Pace queries so the eval itself does not re-trigger the rerank endpoint's
# per-minute 429 window it is trying to measure through. 0 = no pacing.
DELAY_MS = int(os.environ.get("EVAL_DELAY_MS", "0"))
# "on" / "off" / "default" (omit the field). The confidence gate is only
# active when the cross-encoder actually scored the list, so this toggle
# changes gating behaviour as well as ordering.
RERANK = os.environ.get("EVAL_RERANK", "default")
# Omit mode_mix entirely, so the service's own precedence chain applies:
# request > agent config > smart hybrid > static defaults. Needed to test
# whether smart_mix's query-adaptive weights fire — an explicit mode_mix
# suppresses them by design.
NO_MIX = os.environ.get("EVAL_NO_MIX", "0") == "1"
# Process a SLICE of the golden set. The data-plane internal token lives ~5
# minutes; a full 87-query run with the cross-encoder on exceeds that, and the
# tail of the run then 401s. Chunking lets the caller mint a fresh token per
# slice, which is more robust than a background refresher racing the run.
OFFSET = int(os.environ.get("EVAL_OFFSET", "0"))
LIMIT = int(os.environ.get("EVAL_LIMIT", "0")) or None

# Defaults are the DEPLOYED blend shape, read off the running service, so a
# sweep measures the real production decision rather than a dense-only proxy.
# An earlier sweep varied w_bm25 with every other arm zeroed, which measures the
# bm25:dense ratio and not the fusion the service actually performs — and it led
# me to describe 0.4 as "the deployed default" when the deployed value was 0.2
# all along (0.4 was this script's own default).
#   w_dense .45  w_bm25 .2  w_graph .2  w_wiki .1  w_visual .2  w_keyword .05
# w_visual is .2 from a live env override, not the .05 the code defaults to.
W = {
    "w_dense":   float(os.environ.get("W_DENSE", "0.45")),
    "w_bm25":    float(os.environ.get("W_BM25", "0.2")),
    "w_graph":   float(os.environ.get("W_GRAPH", "0.2")),
    "w_wiki":    float(os.environ.get("W_WIKI", "0.1")),
    "w_visual":  float(os.environ.get("W_VISUAL", "0.2")),
    "w_keyword": float(os.environ.get("W_KEYWORD", "0.05")),
}
W_DENSE, W_BM25 = W["w_dense"], W["w_bm25"]
GOLDEN = _ALL[OFFSET:OFFSET + LIMIT] if LIMIT else _ALL[OFFSET:]


def retrieve(query):
    body = {
        "org_id": ORG, "query": query,
        # top_k participates in the retrieval cache key, so overriding it is
        # the cache-busting lever for independent confirmation runs.
        "top_k": int(os.environ.get("EVAL_TOP_K", "30")), "top_n": 10,
        **({} if NO_MIX else {"mode_mix": {**W, **({} if RERANK == "default" else {"rerank": RERANK == "on"})}}),
        # Declared explicitly, because the orchestrator defaults
        # `sovereign_required` to TRUE and Azure-hosted Cohere Embed v4 can never
        # satisfy sovereignty — so an omitted field fails the query embedding
        # closed with "ZDR content must not egress to the Cohere Embed v4 text
        # path". That default is correct (absence of proof is not proof of
        # safety); this corpus is synthetic repo documentation with no residency
        # requirement, so the caller is the right place to say so.
        "sovereign_required": False,
        "zdr_mode": "disabled",
    }
    req = urllib.request.Request(
        RETRIEVAL + "/v1/retrieve", data=json.dumps(body).encode(),
        headers={"content-type": "application/json",
                 "authorization": "Bearer " + token(), "X-Org-ID": ORG},
        method="POST")
    try:
        with urllib.request.urlopen(req, timeout=300) as r:
            return r.status, json.loads(r.read() or b"{}")
    except urllib.error.HTTPError as e:
        return e.code, {"error": e.read().decode()[:300]}


print(f"=== {LABEL}: org={ORG} w_bm25={W_BM25} of {W} "
      f"queries={len(GOLDEN)} ===", flush=True)
results, lat, zero, fails = [], [], 0, 0
for i, g in enumerate(GOLDEN, 1):
    if DELAY_MS and i > 1:
        time.sleep(DELAY_MS / 1000.0)
    t0 = time.time()
    st, out = retrieve(g["query"])
    ms = (time.time() - t0) * 1000
    if st >= 300:
        fails += 1
        print(f"  [{i:2d}] FAIL {st} {json.dumps(out)[:160]}", flush=True)
        continue
    lat.append(ms)
    cands = out.get("candidates", [])
    if not cands:
        zero += 1
    # Keep only what scoring needs, in rank order.
    results.append({
        "query": g["query"],
        "relevant_ids": g["relevant_ids"],
        "latency_ms": ms,
        "retrieved": [{"document_id": c.get("document_id"),
                       "knowledge_id": c.get("knowledge_id")} for c in cands],
    })
    if not QUIET:
        hit = any(c.get("document_id") in set(g["relevant_ids"]) for c in cands)
        print(f"  [{i:2d}] {st} {len(cands):2d} cands {ms:6.0f}ms "
              f"hit={'Y' if hit else 'n'} {g['query'][:56]}", flush=True)

if fails:
    print(f"ABORT: {fails} query failures")
    sys.exit(2)
lat.sort()
print(f"  latency p50={lat[len(lat)//2]:.0f}ms "
      f"p95={lat[int(len(lat)*0.95)-1]:.0f}ms zero-result={zero}")
json.dump(results, open(OUT_FILE, "w", encoding="utf-8"))
print(f"  wrote {OUT_FILE} ({len(results)} queries)")
