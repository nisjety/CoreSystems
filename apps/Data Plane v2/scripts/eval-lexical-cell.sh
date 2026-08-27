#!/bin/sh
# Run the MINED LEXICAL query class and write it as a gated cell.
#
# ## Why this is a separate cell and not more queries in the main set
#
# The 87-query golden set is entirely long natural-language questions. That is
# the class where the dense arm dominates, and it is why sweeping `w_bm25`
# appeared to show the lexical arm only ever costing nDCG: the set contained
# none of the queries BM25 and the keyword arm exist to serve. Measured on the
# mined class instead, `w_bm25` is worth +0.37 nDCG / +0.20 recall
# (docs/retrieval-fusion-evidence-2026-08-26.md, round 6).
#
# Averaged into one number those two classes cancel, and a regression that
# destroys exact-identifier lookup while leaving prose retrieval intact would
# move the blended metric by less than its noise floor. So they are gated
# separately.
#
# ## Why the judgments are trustworthy
#
# `eval-mine-lexical-queries.py` takes tokens occurring in exactly ONE corpus
# document, so that document IS the answer by construction — no human deciding
# which file "defines" a symbol. Terms in 2 documents keep both as relevant;
# 3+ are dropped as ambiguous. Re-checkable at any time by re-running the miner.
#
# ## Why the DEFAULT path, unlike the attribution study
#
# `eval-attribution-study.sh` pins explicit weights, because its job is
# attribution — isolating what each arm contributes. Pinning `mode_mix`
# SUPPRESSES smart hybrid's query-adaptive routing, so those cells measure a
# configuration no real caller uses. A gate should protect what callers actually
# get, so this runs with EVAL_NO_MIX=1 and lets `smart_mix.rs` route. On this
# query class that is also the stronger configuration: smart hybrid reached
# 0.9652 nDCG unaided in round 6.
#
# Usage:
#   ./scripts/eval-lexical-cell.sh <work-dir>
#
# Requires the corpus-seeder credential in DATA_PLANE_CORPUS_SEEDER_API_KEY
# (Control Plane publishes it; a seed-cred.txt in <work-dir> still works as a
# fallback). Also requires, in <work-dir>:
# run_eval_hybrid.py (copy of scripts/eval-run-retrieval.py), and the
# golden-lexical*.json pair from scripts/eval-mine-lexical-queries.py.
# Requires running: the DP2 stack on dpv2-net, and Control Plane's auth-service.
#
# Then baseline it (MERGES, leaving the natural-language cells alone):
#   python3 scripts/eval-gate.py <work-dir> --update
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

for f in run_eval_hybrid.py golden-lexical.json golden-lexical-ctx.json; do
  [ -f "$SP/$f" ] || { echo "missing $SP/$f — see header"; exit 1; }
done

mint() {
  docker exec -e CRED="$CRED" -e ORG="$1" auth-service node -e '
const body = JSON.stringify({orgId:process.env.ORG, scopes:["documents:read","data:read","data:quality:admin"], reason:"lexical retrieval gate cell"});
fetch("http://127.0.0.1:3011/api/data-plane/internal-token", {method:"POST",
  headers:{"content-type":"application/json","x-service-id":"corpus-seeder","x-service-api-key":process.env.CRED}, body})
 .then(async r=>{ if(r.status>=300){process.stderr.write("HTTP "+r.status);return;} const j=await r.json(); process.stdout.write(j.token); })
 .catch(e=>process.stderr.write("ERR "+e.message));' 2>/dev/null > "$2"
  [ -s "$2" ] || { echo "MINT FAILED for $1"; exit 1; }
}

for cell in "lexical-baseline:org-corpus-baseline:golden-lexical.json:lex-token.txt" \
            "lexical-contextual:org-corpus-contextual:golden-lexical-ctx.json:lex-ctx-token.txt"; do
  label=${cell%%:*}; rest=${cell#*:}
  org=${rest%%:*}; rest=${rest#*:}
  golden=${rest%%:*}; tok=${rest#*:}
  mint "$org" "$SP/$tok"
  printf '%-20s ' "$label"
  # Named st-rr-on-* because that is the glob eval-gate.py collects cells from;
  # the reranker is on here via the default path, not via EVAL_RERANK.
  MSYS_NO_PATHCONV=1 MSYS2_ARG_CONV_EXCL='*' docker run --rm --network dpv2-net \
    -v "$SPW:/work" \
    -e EVAL_ORG="$org" -e EVAL_TOKEN_FILE="/work/$tok" -e EVAL_GOLDEN_FILE="/work/$golden" \
    -e EVAL_NO_MIX=1 -e EVAL_DELAY_MS=600 -e QUIET=1 \
    -e OUT_FILE="/work/st-rr-on-$label.json" \
    python:3.12-slim python /work/run_eval_hybrid.py "$label" 2>&1 \
    | grep -E "latency|ABORT|FAIL" | head -2
done
echo "LEXICAL CELL COMPLETE"
