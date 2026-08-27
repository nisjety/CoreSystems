#!/bin/sh
# Full attribution study, 87 queries x both orgs:
#   rr-on / rr-off      -> what the cross-encoder is worth (now that it retries 429s)
#   loo-<arm>           -> leave-one-out for each unmeasured arm weight
#   floor               -> dense+bm25 only
# EVAL_DELAY_MS=600 paces queries so the study measures the reranker rather than
# the rate limiter. Fresh token per cell (short TTL).
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

# cell name | rerank | w_graph w_wiki w_visual w_keyword (dense .45 / bm25 .1 fixed)
run_cell() {
  name="$1"; rerank="$2"; wg="$3"; ww="$4"; wv="$5"; wk="$6"
  for cell in "baseline:org-corpus-baseline:golden-v2.json:eval-token.txt" \
              "contextual:org-corpus-contextual:golden-v2-ctx.json:ctx-token.txt"; do
    label=${cell%%:*}; rest=${cell#*:}
    org=${rest%%:*}; rest=${rest#*:}
    golden=${rest%%:*}; tok=${rest#*:}
    mint "$org" "$SP/$tok"
    printf '%s %-11s ' "$name" "$label"
    MSYS_NO_PATHCONV=1 MSYS2_ARG_CONV_EXCL='*' docker run --rm --network dpv2-net \
      -v "$SPW:/work" \
      -e EVAL_ORG="$org" -e EVAL_TOKEN_FILE="/work/$tok" -e EVAL_GOLDEN_FILE="/work/$golden" \
      -e W_BM25=0.1 -e W_GRAPH="$wg" -e W_WIKI="$ww" -e W_VISUAL="$wv" -e W_KEYWORD="$wk" \
      -e EVAL_RERANK="$rerank" -e EVAL_DELAY_MS=600 -e QUIET=1 \
      -e OUT_FILE="/work/st-$name-$label.json" \
      python:3.12-slim python /work/run_eval_hybrid.py "$name" 2>&1 \
      | grep -E "latency|ABORT|FAIL" | head -2
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
