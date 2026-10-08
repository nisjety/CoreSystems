#!/bin/sh
# Shared by eval-attribution-study.sh and eval-variant-cell.sh. SOURCED, never run:
#
#     . "$(dirname "$0")/eval-lib.sh"
#     eval_setup "$work_dir"
#
# It holds what both of them must do the same way, so that a fix to one is a fix to both: find the
# corpus-seeder credential, mint a token for an organisation, run one cell of the retrieval driver in
# CHUNKS (one fresh token per slice of queries), and merge the slices.
#
# Why chunked, and why a fresh token per slice: a rerank-on cell's 87 queries at an observed p95 of about
# 5 s (plus EVAL_DELAY_MS=600 pacing) can run past the internal token's 300 s TTL
# (PLANE_TOKEN_TTL_DATA_PLANE_SECONDS), and the tail of the run then 401s and the cell produces NO output
# file at all. Caught live 2026-08-28 running this from a fresh, unattended invocation for the first time:
# `[77] FAIL 401` / `[78] FAIL 401`, ~77 queries in, consistent with ~3.5 s/query x 77 ~= 270 s against a 300 s
# TTL. Nothing downstream reported this as a false pass (see eval-gate.py's baseline-vs-current-cells check),
# but a nightly job that fails on token expiry rather than a real regression is exactly as useless as one that
# silently skips a cell. Chunking is the fix eval-run-retrieval.py's own token() docstring already documents
# ("chunking lets the caller mint a fresh token per slice").

# eval_setup <work-dir>: sets SP (the work dir), SPW (the same path as the Docker host sees it) and CRED.
eval_setup() {
  SP="$1"
  SPW=$(cygpath -w "$SP" 2>/dev/null || echo "$SP")
  # Control Plane owns the `corpus-seeder` principal and publishes its credential as
  # DATA_PLANE_CORPUS_SEEDER_API_KEY. Prefer that over a hand-pasted copy: a stale seed-cred.txt
  # authenticates as nothing, and the failure surfaces much later as an opaque 401 from auth-service. The tr
  # strips surrounding whitespace including a trailing CR, which a file saved on Windows carries and $(cat)
  # does NOT remove -- that lone byte makes the credential wrong while looking correct. [:space:] avoids
  # backslash escapes entirely.
  CRED="${DATA_PLANE_CORPUS_SEEDER_API_KEY:-}"
  if [ -z "$CRED" ] && [ -f "$SP/seed-cred.txt" ]; then
    CRED=$(tr -d '[:space:]' < "$SP/seed-cred.txt")
  fi
  [ -n "$CRED" ] || { echo "no corpus-seeder credential: export DATA_PLANE_CORPUS_SEEDER_API_KEY (Control Plane .env.generated-secrets) or place seed-cred.txt in $SP" >&2; exit 1; }
  CHUNK="${CHUNK:-25}"
}

# mint <org> <token-file>: a bearer for the org, with the scopes a retrieval cell needs.
mint() {
  docker exec -e CRED="$CRED" -e ORG="$1" auth-service node -e '
const body = JSON.stringify({orgId:process.env.ORG, scopes:["documents:read","data:read","data:quality:admin"], reason:"retrieval evaluation run"});
fetch("http://127.0.0.1:3011/api/data-plane/internal-token", {method:"POST",
  headers:{"content-type":"application/json","x-service-id":"corpus-seeder","x-service-api-key":process.env.CRED}, body})
 .then(async r=>{ if(r.status>=300){process.stderr.write("HTTP "+r.status);return;} const j=await r.json(); process.stdout.write(j.token); })
 .catch(e=>process.stderr.write("ERR "+e.message));' 2>/dev/null > "$2"
  [ -s "$2" ] || { echo "MINT FAILED for $1"; exit 1; }
}

# merge_chunks <out> <chunk files...>
#
# Query slices are independent JSON arrays of per-query result dicts -- concatenating them in offset order
# reproduces exactly what one unchunked run would have written, which is what lets eval-score-attribution.py
# and eval-gate.py stay unaware chunking ever happened.
#
# A chunk's docker run can fail badly enough to write NOTHING (caught live: a Cohere-side 500 storm during
# loo-wiki did exactly this) -- the scripts have no `set -e`, so an unguarded `open()` crashing with an
# uncaught FileNotFoundError did not stop the run; it just left an ugly traceback and let the run limp on to
# the next cell as if nothing had happened. Missing chunks are now skipped with a clear message instead.
#
# If EVERY chunk for a cell is missing, this deliberately writes NOTHING rather than an empty `[]`:
# eval-gate.py's own missing-baseline-cell check turns an absent file into a clear, named failure. An
# empty-but-present file would be worse -- scoring it means averaging over zero queries. A partial file (some
# chunks missing, at least one present) is still written; eval-gate.py's queries-vs-baseline floor catches
# that case as "the eval itself did not complete" on its own.
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

# run_chunked <log-label> <out-file> <org> <golden-file> <token-file> <chunk-prefix> [docker -e options...]
#
# Runs eval-run-retrieval.py (staged in the work dir as run_eval_hybrid.py) over the golden file in slices of
# $CHUNK queries, with a fresh token per slice, and merges the slices into <out-file> (inside the work dir).
# Everything after the sixth argument is passed to `docker run` (the cell's own -e options: weights, rerank
# mode, pacing, top_k).
run_chunked() {
  label="$1"; out="$2"; org="$3"; golden="$4"; tok="$5"; prefix="$6"; shift 6
  [ -f "$SP/$golden" ] || { echo "missing $SP/$golden"; return 1; }
  total=$(python3 -c 'import json,sys; print(len(json.load(open(sys.argv[1], encoding="utf-8"))))' "$SP/$golden")
  off=0
  chunk_files=""
  while [ "$off" -lt "$total" ]; do
    mint "$org" "$SP/$tok"
    printf '%s [%3d..%3d) ' "$label" "$off" "$((off + CHUNK))"
    MSYS_NO_PATHCONV=1 MSYS2_ARG_CONV_EXCL='*' docker run --rm --network dpv2-net \
      -v "$SPW:/work" \
      -e EVAL_ORG="$org" -e EVAL_TOKEN_FILE="/work/$tok" -e EVAL_GOLDEN_FILE="/work/$golden" \
      -e EVAL_OFFSET="$off" -e EVAL_LIMIT="$CHUNK" -e QUIET=1 \
      -e OUT_FILE="/work/$prefix-chunk-$off.json" \
      "$@" \
      python:3.12-slim python /work/run_eval_hybrid.py "$label" 2>&1 \
      | grep -E "latency|ABORT|FAIL" | head -2
    chunk_files="$chunk_files $SP/$prefix-chunk-$off.json"
    off=$((off + CHUNK))
  done
  merge_chunks "$SP/$out" $chunk_files
  rm -f $chunk_files
}
