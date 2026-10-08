#!/bin/sh
# Full attribution study, 87 queries x both orgs:
#   rr-on / rr-off      -> what the cross-encoder is worth (now that it retries 429s)
#   loo-<arm>           -> leave-one-out for each unmeasured arm weight
#   floor               -> dense+bm25 only
# EVAL_DELAY_MS=600 paces queries so the study measures the reranker rather than
# the rate limiter.
#
# CR-T14: the `rr-on` cell also runs the two query classes that need a corpus that can show the difference
# (STUDY_CLASSES, default "natural norwegian mixed"):
#   norwegian   queries that use another FORM than the text (golden-v2-no*.json)  -> st-rr-on-norwegian-<org>.json
#   mixed       queries that mix Norwegian and English (golden-v2-mixed*.json)    -> st-rr-on-mixed-<org>.json
# They are built by eval-build-golden-set.py (eval-nightly.sh does it first). A class whose golden file is not in
# the work dir is SKIPPED, said out loud, so an older work dir still runs; `natural` is required. The attribution
# variants (rr-off, loo-*, floor) are about the natural-language class only, as before.
#
# CHUNKED per token, not one token per cell (see eval-lib.sh, which holds the chunking, the credential and the
# token minting, shared with eval-variant-cell.sh).
#
# Usage: ./scripts/eval-attribution-study.sh <work-dir>
set -u
[ $# -ge 1 ] || { echo "usage: $0 <work-dir>" >&2; exit 2; }
DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
# shellcheck source=eval-lib.sh
. "$DIR/eval-lib.sh"
eval_setup "$1"

# Boot-time check of the one switch of this script: a typo in it must not silently drop a class from the gate.
STUDY_CLASSES="${STUDY_CLASSES:-natural norwegian mixed}"
for class in $STUDY_CLASSES; do
  case "$class" in
    natural|norwegian|mixed) ;;
    *) echo "STUDY_CLASSES holds '$class': allowed are natural, norwegian and mixed" >&2; exit 2 ;;
  esac
done
case " $STUDY_CLASSES " in
  *" natural "*) ;;
  *) echo "STUDY_CLASSES must include natural: the attribution study is about it" >&2; exit 2 ;;
esac

# class_cells <class>: the cells of one query class, as label:org:golden-file:token-file
class_cells() {
  case "$1" in
    natural)   echo "baseline:org-corpus-baseline:golden-v2.json:eval-token.txt contextual:org-corpus-contextual:golden-v2-ctx.json:ctx-token.txt" ;;
    norwegian) echo "norwegian-baseline:org-corpus-baseline:golden-v2-no.json:no-token.txt norwegian-contextual:org-corpus-contextual:golden-v2-no-ctx.json:no-ctx-token.txt" ;;
    mixed)     echo "mixed-baseline:org-corpus-baseline:golden-v2-mixed.json:mixed-token.txt mixed-contextual:org-corpus-contextual:golden-v2-mixed-ctx.json:mixed-ctx-token.txt" ;;
  esac
}

# cell name | rerank | w_graph w_wiki w_visual w_keyword (dense .45 / bm25 .1 fixed) | classes
run_cell() {
  name="$1"; rerank="$2"; wg="$3"; ww="$4"; wv="$5"; wk="$6"; classes="$7"
  for class in $classes; do
    for cell in $(class_cells "$class"); do
      label=${cell%%:*}; rest=${cell#*:}
      org=${rest%%:*}; rest=${rest#*:}
      golden=${rest%%:*}; tok=${rest#*:}
      if [ "$class" != natural ] && [ ! -f "$SP/$golden" ]; then
        echo "$name $label: skipped, $golden is not in $SP (eval-build-golden-set.py --out builds it)"
        continue
      fi
      run_chunked "$name $label" "st-$name-$label.json" "$org" "$golden" "$tok" "st-$name-$label" \
        -e W_BM25=0.1 -e W_GRAPH="$wg" -e W_WIKI="$ww" -e W_VISUAL="$wv" -e W_KEYWORD="$wk" \
        -e EVAL_RERANK="$rerank" -e EVAL_DELAY_MS=600
    done
  done
}

run_cell rr-on       on  0.2 0.1 0.2 0.05 "$STUDY_CLASSES"
run_cell rr-off      off 0.2 0.1 0.2 0.05 natural
run_cell loo-graph   on  0.0 0.1 0.2 0.05 natural
run_cell loo-wiki    on  0.2 0.0 0.2 0.05 natural
run_cell loo-visual  on  0.2 0.1 0.0 0.05 natural
run_cell loo-keyword on  0.2 0.1 0.2 0.0  natural
run_cell floor       on  0.0 0.0 0.0 0.0  natural
echo "STUDY COMPLETE"
