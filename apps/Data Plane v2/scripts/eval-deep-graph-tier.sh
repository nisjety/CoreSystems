#!/bin/sh
# Does the DEEP multi-hop graph tier change fused retrieval, versus the
# in-process 1-hop tier?
#
# Isolated by the caller's SCOPE, not by a weight: graph-index requires
# `graph:read` on service principals for `/v1/graph/traverse`, so a token
# without it gets the 1-hop fallback and a token with it gets the remote
# traversal. Everything else — weights, corpus, queries — is identical, so the
# delta is the deep tier's contribution and nothing else.
#
# Runs the default path (no mode_mix, so smart hybrid decides, which is what
# real callers get) in token-sized chunks.
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
CHUNK="${CHUNK:-25}"
TOTAL="${TOTAL:-87}"

mint() { # $1 outfile  $2 scopes-json
  docker exec -e CRED="$CRED" -e ORG=org-corpus-baseline -e SC="$2" auth-service node -e '
const body = JSON.stringify({orgId:process.env.ORG, scopes:JSON.parse(process.env.SC), reason:"deep graph tier eval"});
fetch("http://127.0.0.1:3011/api/data-plane/internal-token", {method:"POST",
  headers:{"content-type":"application/json","x-service-id":"corpus-seeder","x-service-api-key":process.env.CRED}, body})
 .then(async r=>{ if(r.status>=300){process.stderr.write("HTTP "+r.status);return;} const j=await r.json(); process.stdout.write(j.token); })
 .catch(e=>process.stderr.write("ERR "+e.message));' 2>/dev/null > "$1.tmp" && mv "$1.tmp" "$1"
  [ -s "$1" ] || { echo "MINT FAILED"; exit 1; }
}

# Flush the retrieval-result cache between arms.
#
# REQUIRED, not hygiene: the cache key is
# (query, top_k, top_n, zdr, sovereign, admin, mix, filters) and contains
# NOTHING about the caller's scopes or bearer. So the second arm reads the
# first arm's entries verbatim — the first attempt at this A/B recorded 87
# cache hits and a 24ms p50 against the other arm's 3.5s, i.e. it measured the
# cache, not the tier.
flush_retrieval_cache() {
  PW=$(docker exec data-plane-v2-retrieval-engine-1 sh -c 'echo $DRAGONFLY_URL'         | sed 's|redis://:||; s|@.*||' | tr -d '')
  n=$(docker exec data-plane-v2-dragonfly-1 sh -c         "redis-cli -a '$PW' --no-auth-warning --scan --pattern 'dpv2:ret:*'          | xargs -r redis-cli -a '$PW' --no-auth-warning DEL | wc -l" 2>/dev/null | tr -d '')
  echo "  (flushed retrieval cache: ${n:-0} batch(es))"
}

BASE='"documents:read","data:read","data:quality:admin"'
for arm in "onehop:[$BASE]" "deep:[$BASE,\"graph:read\"]"; do
  label=${arm%%:*}
  scopes=${arm#*:}
  flush_retrieval_cache
  off=0
  rm -f "$SP"/deep-"$label"-*.json
  while [ "$off" -lt "$TOTAL" ]; do
    mint "$SP/eval-token.txt" "$scopes"
    printf '  %-7s [%2d..%2d] ' "$label" "$off" "$((off + CHUNK))"
    MSYS_NO_PATHCONV=1 MSYS2_ARG_CONV_EXCL='*' docker run --rm --network dpv2-net \
      -v "$SPW:/work" \
      -e EVAL_ORG=org-corpus-baseline -e EVAL_TOKEN_FILE=/work/eval-token.txt \
      -e EVAL_GOLDEN_FILE=/work/golden-v2.json \
      -e EVAL_NO_MIX=1 -e EVAL_OFFSET="$off" -e EVAL_LIMIT="$CHUNK" \
      -e EVAL_DELAY_MS=0 -e QUIET=1 \
      -e OUT_FILE="/work/deep-$label-$off.json" \
      python:3.12-slim python /work/run_eval_hybrid.py "$label-$off" 2>&1 \
      | grep -E "latency|ABORT" | head -1
    off=$((off + CHUNK))
  done
done
echo "DEEP TIER A/B COMPLETE"
