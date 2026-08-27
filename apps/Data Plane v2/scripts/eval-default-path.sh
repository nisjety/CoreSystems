#!/bin/sh
# Evaluate the DEFAULT retrieval path (no mode_mix -> smart hybrid decides) over
# the full 87-query set, in token-sized chunks.
#
# Chunked because the data-plane internal token lives ~5 minutes and a full run
# with the cross-encoder on exceeds that; a background refresher racing the run
# proved unreliable (43 then 4 tail failures). One fresh token per chunk is
# deterministic instead.
set -u
SP="$1"
SPW=$(cygpath -w "$SP" 2>/dev/null || echo "$SP")
CRED=$(cat "$SP/seed-cred.txt")
CHUNK="${CHUNK:-25}"
TOTAL="${TOTAL:-87}"

mint() {
  docker exec -e CRED="$CRED" -e ORG="$1" auth-service node -e '
const body = JSON.stringify({orgId:process.env.ORG, scopes:["documents:read","data:read","data:quality:admin"], reason:"default-path eval"});
fetch("http://127.0.0.1:3011/api/data-plane/internal-token", {method:"POST",
  headers:{"content-type":"application/json","x-service-id":"corpus-seeder","x-service-api-key":process.env.CRED}, body})
 .then(async r=>{ if(r.status>=300){process.stderr.write("HTTP "+r.status);return;} const j=await r.json(); process.stdout.write(j.token); })
 .catch(e=>process.stderr.write("ERR "+e.message));' 2>/dev/null > "$2.tmp" && mv "$2.tmp" "$2"
  [ -s "$2" ] || { echo "MINT FAILED for $1"; exit 1; }
}

for cell in "baseline:org-corpus-baseline:golden-v2.json:eval-token.txt" \
            "contextual:org-corpus-contextual:golden-v2-ctx.json:ctx-token.txt"; do
  label=${cell%%:*}; rest=${cell#*:}
  org=${rest%%:*}; rest=${rest#*:}
  golden=${rest%%:*}; tok=${rest#*:}
  off=0
  rm -f "$SP"/smartchunk-"$label"-*.json
  while [ "$off" -lt "$TOTAL" ]; do
    mint "$org" "$SP/$tok"
    printf '  %-11s [%2d..%2d] ' "$label" "$off" "$((off + CHUNK))"
    MSYS_NO_PATHCONV=1 MSYS2_ARG_CONV_EXCL='*' docker run --rm --network dpv2-net \
      -v "$SPW:/work" \
      -e EVAL_ORG="$org" -e EVAL_TOKEN_FILE="/work/$tok" -e EVAL_GOLDEN_FILE="/work/$golden" \
      -e EVAL_NO_MIX=1 -e EVAL_OFFSET="$off" -e EVAL_LIMIT="$CHUNK" \
      -e EVAL_DELAY_MS=0 -e QUIET=1 \
      -e OUT_FILE="/work/smartchunk-$label-$off.json" \
      python:3.12-slim python /work/run_eval_hybrid.py "smart-$label-$off" 2>&1 \
      | grep -E "latency|ABORT" | head -1
    off=$((off + CHUNK))
  done
done
echo "DEFAULT-PATH CHUNKED EVAL COMPLETE"
