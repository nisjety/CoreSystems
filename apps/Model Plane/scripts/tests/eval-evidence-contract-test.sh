#!/usr/bin/env bash
set -euo pipefail

# Contract coverage for the privacy-safe evaluation evidence validator. The
# fixture deliberately contains hashes and metrics only: an eval artifact must
# never need a prompt, model response, hidden task, or customer transcript.

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

sha256_file() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  else
    shasum -a 256 "$1" | awk '{print $1}'
  fi
}

write_fixture() {
  local directory="$1" revision="0123456789abcdef0123456789abcdef01234567"
  mkdir -p "$directory"
  printf 'case_id\tfixture_sha256\tpolicy_fingerprint_sha256\ncase-grounded\t%064d\t%064d\ncase-recovery\t%064d\t%064d\n' \
    1 2 3 4 >"$directory/cases.tsv"
  printf 'case_id\tattempt\toutcome\tverifier\twall_clock_ms\taction_count\tinput_tokens\toutput_tokens\tcost_micro_usd\trecovery_count\ttrace_sha256\n' \
    >"$directory/results.tsv"
  printf 'case-grounded\t1\tpassed\tpassed\t1200\t4\t500\t120\t2200\t0\t%064d\n' 5 >>"$directory/results.tsv"
  printf 'case-recovery\t1\tfailed\tfailed\t1800\t9\t700\t60\t3100\t2\t%064d\n' 6 >>"$directory/results.tsv"
  printf 'EVAL_MANIFEST_VERSION=1\n' >"$directory/eval-manifest.env"
  printf 'SUITE_ID=model-plane-release-regression\nSUITE_REVISION=%s\nEVALUATED_AT=2026-08-11T12:00:00Z\n' "$revision" >>"$directory/eval-manifest.env"
  printf 'LANE=integrated-harness\nMAX_ATTEMPTS=3\nMAX_WALL_CLOCK_MS=2000\nMAX_ACTION_COUNT=10\n' \
    >>"$directory/eval-manifest.env"
  printf 'MAX_INPUT_TOKENS=1000\nMAX_OUTPUT_TOKENS=500\nMAX_COST_MICRO_USD=5000\n' \
    >>"$directory/eval-manifest.env"
  printf 'TRACE_RETENTION=zdr-safe-digest-only\n' >>"$directory/eval-manifest.env"
  printf 'MODEL_PROVIDER_FINGERPRINT_SHA256=%064d\nPROMPT_FINGERPRINT_SHA256=%064d\n' 7 8 \
    >>"$directory/eval-manifest.env"
  printf 'TOOL_CAPABILITY_FINGERPRINT_SHA256=%064d\nMEMORY_RETRIEVAL_FINGERPRINT_SHA256=%064d\n' 9 10 \
    >>"$directory/eval-manifest.env"
  printf 'FEEDBACK_FINGERPRINT_SHA256=%064d\nPOLICY_FINGERPRINT_SHA256=%064d\n' 11 12 \
    >>"$directory/eval-manifest.env"
  printf 'EXECUTOR_IMPLEMENTATION_SHA256=%064d\nINDEPENDENT_VERIFIER_IMPLEMENTATION_SHA256=%064d\n' 13 14 \
    >>"$directory/eval-manifest.env"
  printf 'CASES_SHA256=%s\nRESULTS_SHA256=%s\n' \
    "$(sha256_file "$directory/cases.tsv")" "$(sha256_file "$directory/results.tsv")" \
    >>"$directory/eval-manifest.env"
  {
    printf 'path\tsha256\tbytes\n'
    for file in cases.tsv eval-manifest.env results.tsv; do
      printf '%s\t%s\t%s\n' "$file" "$(sha256_file "$directory/$file")" \
        "$(wc -c <"$directory/$file" | tr -d '[:space:]')"
    done
  } >"$directory/evidence-manifest.tsv"
}

refresh_artifact_digests() {
  local directory="$1" replacement
  replacement="$directory/eval-manifest.updated"
  awk -F= -v cases="$(sha256_file "$directory/cases.tsv")" \
    -v results="$(sha256_file "$directory/results.tsv")" '
    $1 == "CASES_SHA256" { print "CASES_SHA256=" cases; next }
    $1 == "RESULTS_SHA256" { print "RESULTS_SHA256=" results; next }
    { print }
  ' "$directory/eval-manifest.env" >"$replacement"
  mv "$replacement" "$directory/eval-manifest.env"
  {
    printf 'path\tsha256\tbytes\n'
    for file in cases.tsv eval-manifest.env results.tsv; do
      printf '%s\t%s\t%s\n' "$file" "$(sha256_file "$directory/$file")" \
        "$(wc -c <"$directory/$file" | tr -d '[:space:]')"
    done
  } >"$directory/evidence-manifest.tsv"
}

fixture="$TMP_DIR/valid"
write_fixture "$fixture"
"$ROOT_DIR/scripts/eval-evidence.sh" validate "$fixture"
"$ROOT_DIR/scripts/eval-evidence.sh" summarize "$fixture" | \
  grep -F 'lane=integrated-harness cases=2 passed=1 failed=1 limit_reached=0' >/dev/null

model_only="$TMP_DIR/model-only"
cp -R "$fixture" "$model_only"
sed -i.bak 's/LANE=integrated-harness/LANE=model-only/' "$model_only/eval-manifest.env"
rm "$model_only/eval-manifest.env.bak"
refresh_artifact_digests "$model_only"
"$ROOT_DIR/scripts/eval-evidence.sh" compare "$model_only" "$fixture" | \
  grep -F 'paired eval lanes verified: suite=model-plane-release-regression' >/dev/null

# A terminal limit is never a pass, even if a producer tries to label its
# verifier result as passed.
invalid_verifier="$TMP_DIR/invalid-verifier"
cp -R "$fixture" "$invalid_verifier"
sed -i.bak 's/case-recovery\t1\tfailed\tfailed/case-recovery\t1\tlimit_reached\tpassed/' \
  "$invalid_verifier/results.tsv"
rm "$invalid_verifier/results.tsv.bak"
refresh_artifact_digests "$invalid_verifier"
if "$ROOT_DIR/scripts/eval-evidence.sh" validate "$invalid_verifier" >/dev/null 2>&1; then
  echo "eval evidence accepted limit_reached as a verified success" >&2
  exit 1
fi

# The runner and verifier must be independently identified, and every file
# digest recorded in the immutable evidence manifest must match the payload.
same_verifier="$TMP_DIR/same-verifier"
cp -R "$fixture" "$same_verifier"
sed -i.bak 's/INDEPENDENT_VERIFIER_IMPLEMENTATION_SHA256=.*/INDEPENDENT_VERIFIER_IMPLEMENTATION_SHA256=0000000000000000000000000000000000000000000000000000000000000013/' \
  "$same_verifier/eval-manifest.env"
rm "$same_verifier/eval-manifest.env.bak"
refresh_artifact_digests "$same_verifier"
if "$ROOT_DIR/scripts/eval-evidence.sh" validate "$same_verifier" >/dev/null 2>&1; then
  echo "eval evidence accepted a non-independent verifier" >&2
  exit 1
fi

tampered="$TMP_DIR/tampered"
cp -R "$fixture" "$tampered"
printf '\n' >>"$tampered/results.tsv"
if "$ROOT_DIR/scripts/eval-evidence.sh" validate "$tampered" >/dev/null 2>&1; then
  echo "eval evidence accepted an unmanifested result mutation" >&2
  exit 1
fi

echo "eval evidence contracts: ok"
