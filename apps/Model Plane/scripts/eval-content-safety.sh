#!/usr/bin/env bash
set -euo pipefail

# WS26-S8: the content-safety evaluation. It runs the deterministic corpus gate,
# then the live classifier eval against a non-ZDR test org. The eval prints
# counts and latency percentiles only, never corpus text, and it fails when a
# threshold in docs/CONTENT_SAFETY.md is not met. The archived output is what
# docs/MODEL_PLANE_RELEASE_EVIDENCE.md quotes for a run.

usage() {
  cat <<'EOF'
Usage:
  CONTENT_SAFETY_EVAL_BEARER=<delegated inference bearer> \
  CONTENT_SAFETY_EVAL_ORG_ID=<non-ZDR test org> \
    scripts/eval-content-safety.sh

Optional:
  CONTENT_SAFETY_EVAL_INFERENCE_URL  inference-core address (default http://127.0.0.1:9092)
  CONTENT_SAFETY_INJECTION_MODEL     model the classifier asks for (default: inference-core's)
  CONTENT_SAFETY_EVAL_OUT            file for the archived output (default: a new file in TMPDIR)
  CARGO_TARGET_DIR                   reuse a warm Rust target directory

The bearer is the token alone, without the word Bearer. The script never prints
it, and the eval refuses a bearer whose claims mark the org as ZDR. Run it only
against a non-ZDR test org: the corpus is synthetic and public, and the eval
sends every row to the classifier.
EOF
}

die() {
  echo "content-safety eval: $*" >&2
  exit 1
}

case "${1:-}" in
  -h | --help)
    usage
    exit 0
    ;;
  "") ;;
  *)
    usage >&2
    exit 2
    ;;
esac

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
rust_dir="$script_dir/../rust"
[[ -f "$rust_dir/Cargo.toml" ]] || die "the Model Plane Rust workspace is not at $rust_dir"

[[ -n "${CONTENT_SAFETY_EVAL_BEARER:-}" ]] ||
  die "set CONTENT_SAFETY_EVAL_BEARER to a delegated inference bearer for the non-ZDR test org"
[[ -n "${CONTENT_SAFETY_EVAL_ORG_ID:-}" ]] ||
  die "set CONTENT_SAFETY_EVAL_ORG_ID to the non-ZDR test org"

out="${CONTENT_SAFETY_EVAL_OUT:-$(mktemp "${TMPDIR:-/tmp}/content-safety-eval.XXXXXX")}"
export CARGO_INCREMENTAL=0
cd "$rust_dir"

# Each step's output goes to the archive as well as the terminal. The first
# failing step stops the run, so a failed corpus gate never spends live calls.
run_step() {
  local title="$1"
  shift
  local status=0
  printf '== %s ==\n' "$title" | tee -a "$out"
  "$@" 2>&1 | tee -a "$out" || status=$?
  if [[ "$status" -ne 0 ]]; then
    printf 'content-safety eval: %s failed (exit %s)\n' "$title" "$status" | tee -a "$out" >&2
    exit "$status"
  fi
}

run_step "deterministic corpus gate" cargo test -q -p mp-content-safety
run_step "offline gate arithmetic" cargo test -q -p model-gateway --test content_safety_eval
run_step "live classifier eval" \
  cargo test -q -p model-gateway --test content_safety_eval -- --ignored --nocapture

echo "content-safety eval: every gate passed; output archived at $out"
