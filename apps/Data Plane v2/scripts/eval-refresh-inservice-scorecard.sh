#!/bin/sh
# Trigger a data-quality-go eval run for each fixture org and print the
# by_class breakdown (round 10: Scorecard.ByClass, segmenting
# natural_language vs lexical_identifier rather than one blended number).
#
# Requires the SAME things eval-nightly.sh already checked: the stack up,
# DATA_PLANE_CORPUS_SEEDER_API_KEY exported. Run through eval-nightly.sh, not
# standalone, unless you export that yourself first.
#
# Informational only — see eval-nightly.sh for why this doesn't gate anything
# yet (no committed baseline for the in-service scorecard).
set -eu

: "${DATA_PLANE_CORPUS_SEEDER_API_KEY:?export DATA_PLANE_CORPUS_SEEDER_API_KEY first}"

mint() {
  # $1 = org_id, writes the token to stdout
  docker exec -e CRED="$DATA_PLANE_CORPUS_SEEDER_API_KEY" -e ORG="$1" auth-service node -e '
const body = JSON.stringify({orgId:process.env.ORG, scopes:["data:quality:admin"], reason:"nightly eval scorecard refresh"});
fetch("http://127.0.0.1:3011/api/data-plane/internal-token", {method:"POST",
  headers:{"content-type":"application/json","x-service-id":"corpus-seeder","x-service-api-key":process.env.CRED}, body})
 .then(async r=>{ if(r.status>=300){process.stderr.write("HTTP "+r.status);return;} const j=await r.json(); process.stdout.write(j.token); })
 .catch(e=>process.stderr.write("ERR "+e.message));' 2>/dev/null
}

for org in org-corpus-baseline org-corpus-contextual; do
  echo "### $org"
  # `|| true`: a bare `VAR=$(cmd)` propagates cmd's exit status under
  # `set -e`, which would abort this whole (informational-only) script on the
  # FIRST org whose mint has a hiccup, rather than falling through to the
  # empty-token check on the next line and trying the second org anyway.
  TOKEN=$(mint "$org" || true)
  [ -n "$TOKEN" ] || { echo "  token mint failed, skipping"; continue; }

  KEY="nightly-$(date -u +%Y%m%d)-${org}"
  # Same `set -e` hazard as the mint above, twice over: a failing `docker exec`
  # and a python JSONDecodeError (on malformed/empty input) both exit non-zero,
  # and both of these are otherwise bare statements this script would abort on
  # before ever reaching the `[ -n "$EVAL_ID" ]` fallback below.
  RUN=$(docker exec -e TOK="$TOKEN" -e KEY="$KEY" -e ORG="$org" data-plane-v2-data-quality-1 sh -c \
    'curl -s -X POST http://127.0.0.1:8013/v1/evals/retrieval -H "content-type: application/json" -H "authorization: Bearer $TOK" -H "Idempotency-Key: $KEY" -d "{\"strategy\":\"hybrid\",\"corpus\":\"$ORG\"}"' || true)
  EVAL_ID=$(printf '%s' "$RUN" | python3 -c 'import sys,json; print(json.load(sys.stdin).get("eval_id",""))' 2>/dev/null || true)
  [ -n "$EVAL_ID" ] || { echo "  eval create failed: $RUN"; continue; }

  # RunEval executes inline (see internal/eval/runner.go) so this is
  # typically already complete; a short poll covers any future async path.
  for _ in 1 2 3 4 5; do
    RESULT=$(docker exec -e TOK="$TOKEN" -e EID="$EVAL_ID" data-plane-v2-data-quality-1 sh -c \
      'curl -s http://127.0.0.1:8013/v1/evals/retrieval/$EID -H "authorization: Bearer $TOK"' || true)
    STATUS=$(printf '%s' "$RESULT" | python3 -c 'import sys,json; print(json.load(sys.stdin).get("status",""))' 2>/dev/null || true)
    # NOT `[ A ] || [ B ] && break`: under `set -e`, that whole compound list's
    # own exit status is the shell's exit status when neither side is true
    # (still pending) — which would abort the entire nightly run on the very
    # first poll that isn't already done, before ever getting a second look.
    if [ "$STATUS" = "completed" ] || [ "$STATUS" = "failed" ]; then
      break
    fi
    sleep 2
  done

  printf '%s' "$RESULT" | python3 -c '
import json, sys
d = json.load(sys.stdin)
# %-formatting, not f-strings, and double-quoted dict keys throughout,
# deliberately: this whole block is inside a single-quoted shell string, so a
# python string literal in here MUST use double quotes (a single quote would
# close the shell string early — confirmed live: it silently turned
# sc["queries_run"] into the bareword sc[queries_run], a NameError). An
# f-string here would need `sc["key"]` inside `{}`, which is also fine on its
# own, but the first version of this script used backslash-escaped quotes
# instead (`{sc[\"key\"]}`) to dodge a DIFFERENT quoting layer, which is a
# SyntaxError on Python < 3.12. Both real bugs, both only caught by actually
# running this, not by reading it — %-formatting sidesteps the whole class.
status = d.get("status")
if status != "completed":
    print("  status=%s (not completed)" % status)
    raise SystemExit
sc = d["scorecard"]
print("  queries_run=%s golden_queries=%s" % (sc["queries_run"], sc["golden_queries"]))
print("  blended: recall=%.4f nDCG=%.4f MRR=%.4f" % (sc["mean_recall_at_10"], sc["mean_ndcg_at_10"], sc["mean_mrr"]))
for cls, s in sorted(sc.get("by_class", {}).items()):
    print("  %-22s n=%-4d recall=%.4f nDCG=%.4f MRR=%.4f" % (cls, s["queries_run"], s["mean_recall_at_10"], s["mean_ndcg_at_10"], s["mean_mrr"]))
'
done
