#!/bin/sh
# Measure ONE variant of the contextual organization for the promotion gate (CR-T14).
#
# A prompt version, a model or a rerank input becomes the default only through
#
#     python3 scripts/eval-gate.py <work-dir> --promote <candidate> --against <default>
#
# which needs the evidence of BOTH variants. This script produces one variant's, in <work-dir>:
#
#     state-<variant>.json            what the contextual organization IS (policy, the provenance of its units, the
#                                     retrieval engine's rerank-input flag) next to what the variant DECLARES it to be
#     coverage-<variant>.json         GET /v1/quality/context-coverage of the contextual organization, as it is
#     cell-<variant>-rr-off.json      the natural-language class with the reranker OFF (attribution: noise about 0.008)
#     cell-<variant>-rr-on-1..3.json  the same with the reranker ON, three times (confirmation: noise about +/-0.04, and
#                                     each run its own top_k, which is part of the retrieval cache key, so that the three
#                                     are three measurements and not one cached answer three times)
#     cell-<variant>-lexical.json     the exact-identifier class (the mined queries)
#
# THE VARIANT IS A STATE OF THE ORGANIZATION, NOT A SWITCH OF THIS SCRIPT. Putting the organization in that state is an
# operator's act, with its own cost and its own approval, and this script only measures it:
#
#   ctx-v1 / ctx-v2    pin the prompt version in the organization's processing policy (PUT .../processing-policy,
#                      context_prompt_version), then let the reconciler refresh the units
#                      (CONTEXT_REFRESH_ON_PROMPT_CHANGE=1 with CONTEXT_REFRESH_MAX_UNITS, or context-backfill --org-id
#                      --target-version): one embedding and one generation per unit, which is paid
#   model-<alias>      change CONTEXTUAL_RETRIEVAL_MODEL and refresh the same way
#   rerank-ctx         set RERANK_INPUT_INCLUDE_CONTEXT=1 on retrieval-engine (CR-T15; nothing to refresh)
#   breadcrumb         not built (CR-T18, deferred): refused
#
# and it DECLARES the state with --prompt / --model / --rerank-input-context. The gate refuses a candidate whose units
# do not carry the declared prompt version and model (otherwise the measurement would be of the default under the
# candidate's name), so a forgotten refresh is a HOLD, not a silent pass. The organization must not change while the
# cells run: its coverage is read before and after, and a difference removes the variant's files and fails.
#
# Cost of one variant: 371 retrieval requests (87 + 3 x 87 + 23), 261 of them reranked (paid Cohere rerank calls) and
# all of them embedded. Run it three times in a promotion (the default, the candidate, and again if the first run
# was hit by a 429 storm), not in a loop.
#
# Usage:
#   ./scripts/eval-variant-cell.sh <work-dir> <variant> [--prompt V] [--model M] [--rerank-input-context 0|1]
# <work-dir> needs run_eval_hybrid.py (a copy of scripts/eval-run-retrieval.py), golden-v2-ctx.json (from
# eval-build-golden-set.py --out) and golden-lexical-ctx.json (from eval-mine-lexical-queries.py).
set -u
[ $# -ge 2 ] || { echo "usage: $0 <work-dir> <variant> [--prompt V] [--model M] [--rerank-input-context 0|1]" >&2; exit 2; }
DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
WORK_ARG="$1"; VARIANT="$2"; shift 2

case "$VARIANT" in
  breadcrumb)
    echo "breadcrumb is not built (CR-T18, owner decision OD-BREADCRUMB): there is nothing to measure" >&2; exit 2 ;;
  ""|-*|*[!a-z0-9._-]*)
    echo "'$VARIANT' is not a variant name (lower case letters, digits, . _ -)" >&2; exit 2 ;;
esac
case "$*" in
  *--prompt*|*--model*|*--rerank-input-context*) ;;
  *) echo "declare what the variant is (--prompt, --model or --rerank-input-context): a variant that declares nothing cannot be checked" >&2; exit 2 ;;
esac

# shellcheck source=eval-lib.sh
. "$DIR/eval-lib.sh"
eval_setup "$WORK_ARG"
for f in run_eval_hybrid.py golden-v2-ctx.json golden-lexical-ctx.json; do
  [ -f "$SP/$f" ] || { echo "missing $SP/$f: see the header" >&2; exit 1; }
done

ORG=org-corpus-contextual
TOK=variant-token.txt
WEIGHTS="-e W_BM25=0.1 -e W_GRAPH=0.2 -e W_WIKI=0.1 -e W_VISUAL=0.2 -e W_KEYWORD=0.05 -e EVAL_DELAY_MS=600"

# The files of a variant are all of ONE run: its state, its coverage and its five cells, by exact name (a glob on the
# variant's name would also take the files of a variant whose name starts with it).
VARIANT_FILES="state-$VARIANT.json coverage-$VARIANT.json cell-$VARIANT-rr-off.json cell-$VARIANT-rr-on-1.json cell-$VARIANT-rr-on-2.json cell-$VARIANT-rr-on-3.json cell-$VARIANT-lexical.json"
clear_variant_files() {
  for name in $VARIANT_FILES; do rm -f "$SP/$name"; done
}
# What an earlier run left is removed before anything is measured: a slice that writes nothing this time must leave its
# cell MISSING (the promotion gate then says so), not silently replaced by the old run's file, measured in another state.
clear_variant_files

# counts_of <file>: the counts of a coverage report, as one comparable line
counts_of() {
  python3 -c 'import json,sys; print(json.dumps(json.load(open(sys.argv[1], encoding="utf-8"))["counts"], sort_keys=True))' "$1"
}

# 1. what the organization is, BEFORE measuring it
python3 "$DIR/eval-service.py" state "$SP" "$VARIANT" "$@" || exit $?
python3 "$DIR/eval-service.py" coverage "$SP" || exit 1
cp "$SP/coverage-contextual.json" "$SP/coverage-$VARIANT.json"
before=$(counts_of "$SP/coverage-contextual.json")

# 2. the cells
# shellcheck disable=SC2086
run_chunked "$VARIANT rr-off" "cell-$VARIANT-rr-off.json" "$ORG" golden-v2-ctx.json "$TOK" "cell-$VARIANT-rr-off" $WEIGHTS -e EVAL_RERANK=off
i=1
while [ "$i" -le 3 ]; do
  # shellcheck disable=SC2086
  run_chunked "$VARIANT rr-on-$i" "cell-$VARIANT-rr-on-$i.json" "$ORG" golden-v2-ctx.json "$TOK" "cell-$VARIANT-rr-on-$i" $WEIGHTS -e EVAL_RERANK=on -e EVAL_TOP_K="$((29 + i))"
  i=$((i + 1))
done
# The exact-identifier class runs the DEFAULT path (no pinned mix), like eval-lexical-cell.sh: that is what callers get.
run_chunked "$VARIANT lexical" "cell-$VARIANT-lexical.json" "$ORG" golden-lexical-ctx.json "$TOK" "cell-$VARIANT-lexical" -e EVAL_NO_MIX=1 -e EVAL_DELAY_MS=600

# 3. the organization must be what it was
python3 "$DIR/eval-service.py" coverage "$SP" || exit 1
after=$(counts_of "$SP/coverage-contextual.json")
if [ "$before" != "$after" ]; then
  echo "the contextual organization CHANGED while the cells ran (its coverage counts differ before and after): the measurement is of no single state, so its files are removed" >&2
  clear_variant_files
  exit 1
fi
# "Complete" is said only of a variant the promotion gate can read: a slice that wrote nothing leaves its cell missing.
incomplete=0
for name in $VARIANT_FILES; do
  [ -f "$SP/$name" ] || { echo "$name was not written (a slice produced no output): the run of $VARIANT is incomplete" >&2; incomplete=1; }
done
[ "$incomplete" = 0 ] || exit 1
echo "VARIANT $VARIANT COMPLETE"
