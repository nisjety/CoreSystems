#!/bin/sh
# Full attribution study, 87 queries x both orgs:
#   rr-on / rr-off      -> what the cross-encoder is worth (now that it retries 429s)
#   loo-<arm>           -> leave-one-out for each unmeasured arm weight
#   floor               -> dense+bm25 only
# EVAL_DELAY_MS=600 paces queries so the study measures the reranker rather than
# the rate limiter.
#
# CHUNKED per token, not one token per cell: a rerank-on cell's 87 queries at
# observed p95 ~5s (plus EVAL_DELAY_MS=600 pacing) can run past the internal
# token's 300s TTL (PLANE_TOKEN_TTL_DATA_PLANE_SECONDS) — the tail of the run
# then 401s and the cell produces NO output file at all. Caught live
# 2026-08-28 running this from a fresh, unattended invocation for the first
# time: `[77] FAIL 401` / `[78] FAIL 401`, ~77 queries in, consistent with
# ~3.5s/query x 77 ~= 270s against a 300s TTL. Nothing downstream reported
# this as a false pass (see eval-gate.py's baseline-vs-current-cells check),
# but a nightly job that fails on token expiry rather than a real regression
# is exactly as useless as one that silently skips a cell. Chunking is the
# fix eval-run-retrieval.py's own token() docstring already documents
# ("chunking lets the caller mint a fresh token per slice") — it just was
# never actually wired up here.
set -u
SP="$1"
SPW=$(cygpath -w "$SP" 2>/dev/null || echo "$SP")
# Control Plane owns the `corpus-seeder` principal and publishes its credential
# as DATA_PLANE_CORPUS_SEEDER_API_KEY. Prefer that over a hand-pasted copy: a
# stale seed-cred.txt authenticates as nothing, and the failure surfaces much
# later as an opaque 401 from auth-service. The tr strips surrounding
# whitespace including a trailing CR, which a file saved on Windows carries
# and $(cat) does NOT remove -- that lone byte makes the credential wrong
# while looking correct. [:space:] avoids backslash escapes entirely.
CRED="${DATA_PLANE_CORPUS_SEEDER_API_KEY:-}"
if [ -z "$CRED" ] && [ -f "$SP/seed-cred.txt" ]; then
  CRED=$(tr -d '[:space:]' < "$SP/seed-cred.txt")
fi
[ -n "$CRED" ] || { echo "no corpus-seeder credential: export DATA_PLANE_CORPUS_SEEDER_API_KEY (Control Plane .env.generated-secrets) or place seed-cred.txt in $SP" >&2; exit 1; }

mint() {
  docker exec -e CRED="$CRED" -e ORG="$1" auth-service node -e '
const body = JSON.stringify({orgId:process.env.ORG, scopes:["documents:read","data:read","data:quality:admin"], reason:"retrieval evaluation run"});
fetch("http://127.0.0.1:3011/api/data-plane/internal-token", {method:"POST",
  headers:{"content-type":"application/json","x-service-id":"corpus-seeder","x-service-api-key":process.env.CRED}, body})
 .then(async r=>{ if(r.status>=300){process.stderr.write("HTTP "+r.status);return;} const j=await r.json(); process.stdout.write(j.token); })
 .catch(e=>process.stderr.write("ERR "+e.message));' 2>/dev/null > "$2"
  [ -s "$2" ] || { echo "MINT FAILED for $1"; exit 1; }
}

# Query slices are independent JSON arrays of per-query result dicts —
# concatenating them in offset order reproduces exactly what one unchunked run
# would have written, which is what lets eval-score-attribution.py and
# eval-gate.py stay unaware chunking ever happened.
#
# A chunk's docker run can fail badly enough to write NOTHING (caught live: a
# Cohere-side 500 storm during loo-wiki did exactly this) — this script has no
# `set -e`, so an unguarded `open()` crashing with an uncaught
# FileNotFoundError did not stop the run; it just left an ugly traceback and
# let run_cell limp on to the next (org, cell) pair as if nothing had happened.
# Missing chunks are now skipped with a clear message instead of crashing.
#
# If EVERY chunk for a cell is missing, this deliberately writes NOTHING
# rather than an empty `[]`: eval-gate.py's own missing-baseline-cell check
# (added the same day this was found) turns an absent file into a clear,
# named failure. An empty-but-present file would be worse — scoring it means
# averaging over zero queries, which is an uncaught StatisticsError inside
# eval-gate.py, not a clear message. A partial file (some chunks missing, at
# least one present) is still written; eval-gate.py's existing
# queries-vs-baseline floor (round 8) catches that case as "the eval itself
# did not complete" on its own.
merge_chunks() {
  out="$1"; shift
  python3 -c '
import json, sys
rows = []
missing = 0
for f in sys.argv[2:]:
    try:
        rows.extend(json.load(open(f, encoding="utf-8")))
    except FileNotFoundError:
        missing += 1
        print(f"  (missing chunk, skipped: {f})", file=sys.stderr)
if missing:
    print(f"  {missing}/{len(sys.argv) - 2} chunks missing for this cell", file=sys.stderr)
if rows:
    json.dump(rows, open(sys.argv[1], "w", encoding="utf-8"))
else:
    print(f"  no chunks produced any output — leaving {sys.argv[1]} absent", file=sys.stderr)
' "$out" "$@"
}

CHUNK="${CHUNK:-25}"

# cell name | rerank | w_graph w_wiki w_visual w_keyword (dense .45 / bm25 .1 fixed)
run_cell() {
  name="$1"; rerank="$2"; wg="$3"; ww="$4"; wv="$5"; wk="$6"
  for cell in "baseline:org-corpus-baseline:golden-v2.json:eval-token.txt" \
              "contextual:org-corpus-contextual:golden-v2-ctx.json:ctx-token.txt"; do
    label=${cell%%:*}; rest=${cell#*:}
    org=${rest%%:*}; rest=${rest#*:}
    golden=${rest%%:*}; tok=${rest#*:}

    total=$(python3 -c 'import json,sys; print(len(json.load(open(sys.argv[1], encoding="utf-8"))))' "$SP/$golden")
    off=0
    chunk_files=""
    while [ "$off" -lt "$total" ]; do
      mint "$org" "$SP/$tok"
      printf '%s %-11s [%3d..%3d) ' "$name" "$label" "$off" "$((off + CHUNK))"
      MSYS_NO_PATHCONV=1 MSYS2_ARG_CONV_EXCL='*' docker run --rm --network dpv2-net \
        -v "$SPW:/work" \
        -e EVAL_ORG="$org" -e EVAL_TOKEN_FILE="/work/$tok" -e EVAL_GOLDEN_FILE="/work/$golden" \
        -e EVAL_OFFSET="$off" -e EVAL_LIMIT="$CHUNK" \
        -e W_BM25=0.1 -e W_GRAPH="$wg" -e W_WIKI="$ww" -e W_VISUAL="$wv" -e W_KEYWORD="$wk" \
        -e EVAL_RERANK="$rerank" -e EVAL_DELAY_MS=600 -e QUIET=1 \
        -e OUT_FILE="/work/st-$name-$label-chunk-$off.json" \
        python:3.12-slim python /work/run_eval_hybrid.py "$name" 2>&1 \
        | grep -E "latency|ABORT|FAIL" | head -2
      chunk_files="$chunk_files $SP/st-$name-$label-chunk-$off.json"
      off=$((off + CHUNK))
    done
    merge_chunks "$SP/st-$name-$label.json" $chunk_files
    rm -f $chunk_files
  done
}

run_cell rr-on       on  0.2 0.1 0.2 0.05
run_cell rr-off      off 0.2 0.1 0.2 0.05
run_cell loo-graph   on  0.0 0.1 0.2 0.05
run_cell loo-wiki    on  0.2 0.0 0.2 0.05
run_cell loo-visual  on  0.2 0.1 0.0 0.05
run_cell loo-keyword on  0.2 0.1 0.2 0.0
run_cell floor       on  0.0 0.0 0.0 0.0
echo "STUDY COMPLETE"
