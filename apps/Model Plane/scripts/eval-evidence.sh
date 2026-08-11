#!/usr/bin/env bash
set -euo pipefail

# Validate a content-free evaluation record before it is attached to a staged
# or signed candidate. The artifact captures only immutable fingerprints and
# aggregate per-case telemetry: hidden tasks, prompts, model output, traces,
# reasoning, and customer material remain outside this contract.

usage() {
  cat <<'EOF'
Usage:
  scripts/eval-evidence.sh validate <evidence-directory>
  scripts/eval-evidence.sh summarize <evidence-directory>
  scripts/eval-evidence.sh compare <model-only-evidence-directory> <integrated-harness-evidence-directory>

An evidence directory contains exactly:
  eval-manifest.env       versioned suite, harness, verifier, and budget metadata
  cases.tsv               case IDs and digest-only fixture/policy identities
  results.tsv             independent-verifier outcome and bounded telemetry
  evidence-manifest.tsv   immutable checksums and sizes for the three payload files

The two required lanes are `model-only` and `integrated-harness`. Publish one
record for each lane before making a comparative quality claim. This validator
does not certify a provider, benchmark, signer, or deployment; a signed release
candidate must bind any accepted evidence artifact separately.
EOF
}

die() {
  echo "eval evidence error: $*" >&2
  exit 1
}

sha256_file() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  else
    shasum -a 256 "$1" | awk '{print $1}'
  fi
}

require_regular_file() {
  [[ -f "$1" && ! -L "$1" ]] || die "required regular file is missing: $1"
}

assignment_value() {
  local file="$1" key="$2" value count
  value="$(awk -F= -v expected="$key" '
    $0 == "" || $0 ~ /^#/ { next }
    index($0, "=") == 0 { malformed = 1; next }
    $1 == expected { count++; value = substr($0, length(expected) + 2) }
    END {
      if (malformed || count != 1) exit 1
      print value
    }
  ' "$file")" || die "manifest assignment is missing, duplicated, or malformed: $key"
  printf '%s\n' "$value"
}

validate_sha256() {
  local label="$1" value="$2"
  [[ "$value" =~ ^[0-9a-f]{64}$ ]] || die "$label must be a lowercase SHA-256 digest"
  [[ "$value" != "$(printf '%064d' 0)" ]] || die "$label must not be a placeholder digest"
}

validate_positive_integer() {
  local label="$1" value="$2"
  [[ "$value" =~ ^[1-9][0-9]{0,9}$ ]] || die "$label must be a positive decimal integer"
}

validate_nonnegative_integer() {
  local label="$1" value="$2"
  [[ "$value" =~ ^(0|[1-9][0-9]{0,9})$ ]] || die "$label must be a non-negative decimal integer"
}

validate_eval_manifest() {
  local directory="$1" manifest
  manifest="$directory/eval-manifest.env"
  local -a expected=(
    EVAL_MANIFEST_VERSION SUITE_ID SUITE_REVISION EVALUATED_AT LANE
    MAX_ATTEMPTS MAX_WALL_CLOCK_MS MAX_ACTION_COUNT MAX_INPUT_TOKENS
    MAX_OUTPUT_TOKENS MAX_COST_MICRO_USD TRACE_RETENTION
    MODEL_PROVIDER_FINGERPRINT_SHA256 PROMPT_FINGERPRINT_SHA256
    TOOL_CAPABILITY_FINGERPRINT_SHA256 MEMORY_RETRIEVAL_FINGERPRINT_SHA256
    FEEDBACK_FINGERPRINT_SHA256 POLICY_FINGERPRINT_SHA256
    EXECUTOR_IMPLEMENTATION_SHA256 INDEPENDENT_VERIFIER_IMPLEMENTATION_SHA256
    CASES_SHA256 RESULTS_SHA256
  )
  local assignment_count key value executor verifier
  require_regular_file "$manifest"
  assignment_count="$(awk 'NF && $0 !~ /^#/ { count++ } END { print count + 0 }' "$manifest")"
  [[ "$assignment_count" == "${#expected[@]}" ]] || die "eval manifest contains missing or unsupported assignments"
  for key in "${expected[@]}"; do
    assignment_value "$manifest" "$key" >/dev/null
  done
  [[ "$(assignment_value "$manifest" EVAL_MANIFEST_VERSION)" == "1" ]] ||
    die "eval manifest version is unsupported"
  value="$(assignment_value "$manifest" SUITE_ID)"
  [[ "$value" =~ ^[a-z][a-z0-9-]{2,127}$ ]] || die "SUITE_ID has invalid characters"
  value="$(assignment_value "$manifest" SUITE_REVISION)"
  [[ "$value" =~ ^[0-9a-f]{40}([0-9a-f]{24})?$ ]] || die "SUITE_REVISION must be a full immutable revision"
  value="$(assignment_value "$manifest" EVALUATED_AT)"
  [[ "$value" =~ ^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}Z$ ]] ||
    die "EVALUATED_AT must use UTC RFC 3339 second precision"
  case "$(assignment_value "$manifest" LANE)" in
    model-only | integrated-harness) ;;
    *) die "LANE must be model-only or integrated-harness" ;;
  esac
  for key in MAX_ATTEMPTS MAX_WALL_CLOCK_MS MAX_ACTION_COUNT MAX_INPUT_TOKENS MAX_OUTPUT_TOKENS MAX_COST_MICRO_USD; do
    validate_positive_integer "$key" "$(assignment_value "$manifest" "$key")"
  done
  [[ "$(assignment_value "$manifest" TRACE_RETENTION)" == "zdr-safe-digest-only" ]] ||
    die "TRACE_RETENTION must be zdr-safe-digest-only"
  for key in MODEL_PROVIDER_FINGERPRINT_SHA256 PROMPT_FINGERPRINT_SHA256 TOOL_CAPABILITY_FINGERPRINT_SHA256 \
    MEMORY_RETRIEVAL_FINGERPRINT_SHA256 FEEDBACK_FINGERPRINT_SHA256 POLICY_FINGERPRINT_SHA256 \
    EXECUTOR_IMPLEMENTATION_SHA256 INDEPENDENT_VERIFIER_IMPLEMENTATION_SHA256 CASES_SHA256 RESULTS_SHA256; do
    validate_sha256 "$key" "$(assignment_value "$manifest" "$key")"
  done
  executor="$(assignment_value "$manifest" EXECUTOR_IMPLEMENTATION_SHA256)"
  verifier="$(assignment_value "$manifest" INDEPENDENT_VERIFIER_IMPLEMENTATION_SHA256)"
  [[ "$executor" != "$verifier" ]] || die "independent verifier must differ from executor"
  [[ "$(assignment_value "$manifest" CASES_SHA256)" == "$(sha256_file "$directory/cases.tsv")" ]] ||
    die "CASES_SHA256 does not match cases.tsv"
  [[ "$(assignment_value "$manifest" RESULTS_SHA256)" == "$(sha256_file "$directory/results.tsv")" ]] ||
    die "RESULTS_SHA256 does not match results.tsv"
}

validate_cases_and_results() {
  local directory="$1" manifest
  manifest="$directory/eval-manifest.env"
  local attempts wall actions input output cost
  require_regular_file "$directory/cases.tsv"
  require_regular_file "$directory/results.tsv"
  attempts="$(assignment_value "$manifest" MAX_ATTEMPTS)"
  wall="$(assignment_value "$manifest" MAX_WALL_CLOCK_MS)"
  actions="$(assignment_value "$manifest" MAX_ACTION_COUNT)"
  input="$(assignment_value "$manifest" MAX_INPUT_TOKENS)"
  output="$(assignment_value "$manifest" MAX_OUTPUT_TOKENS)"
  cost="$(assignment_value "$manifest" MAX_COST_MICRO_USD)"
  awk -F '\t' \
    -v max_attempts="$attempts" -v max_wall="$wall" -v max_actions="$actions" \
    -v max_input="$input" -v max_output="$output" -v max_cost="$cost" '
    function positive(value) { return value ~ /^[1-9][0-9]{0,9}$/ }
    function nonnegative(value) { return value ~ /^(0|[1-9][0-9]{0,9})$/ }
    function sha256(value) { return value ~ /^[0-9a-f]{64}$/ && value !~ /^0+$/ }
    NR == FNR {
      if (FNR == 1) {
        if ($0 != "case_id\tfixture_sha256\tpolicy_fingerprint_sha256") fail = "cases header"
        next
      }
      if (NF != 3 || $1 !~ /^[a-z][a-z0-9-]{2,127}$/ || !sha256($2) || !sha256($3)) {
        fail = "malformed cases row"
        next
      }
      if (++case_count[$1] != 1) fail = "duplicate case ID"
      cases[$1] = 1
      case_rows++
      next
    }
    FNR == 1 {
      if ($0 != "case_id\tattempt\toutcome\tverifier\twall_clock_ms\taction_count\tinput_tokens\toutput_tokens\tcost_micro_usd\trecovery_count\ttrace_sha256") fail = "results header"
      next
    }
    {
      if (NF != 11 || !($1 in cases) || !positive($2) || $2 > max_attempts ||
          !($3 == "passed" || $3 == "failed" || $3 == "limit_reached") ||
          !($4 == "passed" || $4 == "failed") || !positive($5) || $5 > max_wall ||
          !nonnegative($6) || $6 > max_actions || !nonnegative($7) || $7 > max_input ||
          !nonnegative($8) || $8 > max_output || !nonnegative($9) || $9 > max_cost ||
          !nonnegative($10) || !sha256($11)) {
        fail = "malformed or over-budget results row"
        next
      }
      if (($3 == "passed" && $4 != "passed") || ($3 != "passed" && $4 != "failed")) {
        fail = "outcome and independent verifier disagree"
      }
      pair = $1 SUBSEP $2
      if (++attempt_count[pair] != 1) fail = "duplicate case attempt"
      result_cases[$1] = 1
      result_rows++
    }
    END {
      if (case_rows == 0) fail = "no cases"
      if (result_rows == 0) fail = "no results"
      for (id in cases) if (!(id in result_cases)) fail = "case has no result"
      if (fail != "") {
        print "eval evidence error: " fail > "/dev/stderr"
        exit 1
      }
    }
  ' "$directory/cases.tsv" "$directory/results.tsv" || exit 1
}

validate_evidence_manifest() {
  local directory="$1" manifest
  manifest="$directory/evidence-manifest.tsv"
  local path expected_hash expected_bytes extra actual_hash actual_bytes count
  require_regular_file "$manifest"
  [[ "$(head -n 1 "$manifest")" == $'path\tsha256\tbytes' ]] || die "evidence manifest header is invalid"
  count=0
  while IFS=$'\t' read -r path expected_hash expected_bytes extra || [[ -n "${path:-}" ]]; do
    [[ "$path" == "path" ]] && continue
    [[ -n "$path" && -n "$expected_hash" && -n "$expected_bytes" && -z "${extra:-}" ]] ||
      die "evidence manifest row is malformed"
    case "$path" in cases.tsv|eval-manifest.env|results.tsv) ;;
      *) die "evidence manifest path is not allowed" ;;
    esac
    validate_sha256 "evidence manifest hash" "$expected_hash"
    validate_nonnegative_integer "evidence manifest byte count" "$expected_bytes"
    [[ "$(awk -F '\t' -v target="$path" '$1 == target { count++ } END { print count + 0 }' "$manifest")" == "1" ]] ||
      die "evidence manifest path is missing or duplicated: $path"
    actual_hash="$(sha256_file "$directory/$path")"
    actual_bytes="$(wc -c <"$directory/$path" | tr -d '[:space:]')"
    [[ "$actual_hash" == "$expected_hash" ]] || die "evidence manifest digest mismatch: $path"
    [[ "$actual_bytes" == "$expected_bytes" ]] || die "evidence manifest size mismatch: $path"
    count=$((count + 1))
  done <"$manifest"
  [[ "$count" == "3" ]] || die "evidence manifest must contain exactly three payload rows"
}

validate_evidence_directory() {
  local directory="$1" count
  [[ "$directory" == /* ]] || directory="$PWD/$directory"
  [[ -d "$directory" && ! -L "$directory" ]] || die "evidence directory does not exist or is a symbolic link"
  [[ -z "$(find "$directory" -type l -print -quit)" ]] || die "evidence directory must not contain symbolic links"
  count="$(find "$directory" -type f | wc -l | tr -d '[:space:]')"
  [[ "$count" == "4" ]] || die "evidence directory must contain exactly four regular files"
  validate_eval_manifest "$directory"
  validate_cases_and_results "$directory"
  validate_evidence_manifest "$directory"
}

summarize_evidence() {
  local directory="$1" manifest
  manifest="$directory/eval-manifest.env"
  local cases passed failed limit
  validate_evidence_directory "$directory"
  cases="$(awk 'NR > 1 { ids[$1] = 1 } END { for (id in ids) count++; print count + 0 }' "$directory/cases.tsv")"
  passed="$(awk -F '\t' 'NR > 1 && $3 == "passed" { count++ } END { print count + 0 }' "$directory/results.tsv")"
  failed="$(awk -F '\t' 'NR > 1 && $3 == "failed" { count++ } END { print count + 0 }' "$directory/results.tsv")"
  limit="$(awk -F '\t' 'NR > 1 && $3 == "limit_reached" { count++ } END { print count + 0 }' "$directory/results.tsv")"
  printf 'eval evidence valid: suite=%s lane=%s cases=%s passed=%s failed=%s limit_reached=%s evaluated_at=%s\n' \
    "$(assignment_value "$manifest" SUITE_ID)" "$(assignment_value "$manifest" LANE)" \
    "$cases" "$passed" "$failed" "$limit" "$(assignment_value "$manifest" EVALUATED_AT)"
}

compare_lanes() {
  local model_only="$1" integrated="$2"
  local model_manifest="$model_only/eval-manifest.env"
  local integrated_manifest="$integrated/eval-manifest.env"
  validate_evidence_directory "$model_only"
  validate_evidence_directory "$integrated"
  [[ "$(assignment_value "$model_manifest" LANE)" == "model-only" ]] ||
    die "first comparison directory must be the model-only lane"
  [[ "$(assignment_value "$integrated_manifest" LANE)" == "integrated-harness" ]] ||
    die "second comparison directory must be the integrated-harness lane"
  [[ "$(assignment_value "$model_manifest" SUITE_ID)" == "$(assignment_value "$integrated_manifest" SUITE_ID)" ]] ||
    die "comparison lanes use different suite IDs"
  [[ "$(assignment_value "$model_manifest" SUITE_REVISION)" == "$(assignment_value "$integrated_manifest" SUITE_REVISION)" ]] ||
    die "comparison lanes use different source revisions"
  awk -F '\t' '
    NR == FNR {
      if (FNR > 1) fixtures[$1] = $2
      next
    }
    FNR > 1 {
      if (!($1 in fixtures) || fixtures[$1] != $2) mismatch = 1
      seen[$1] = 1
      next
    }
    END {
      for (id in fixtures) if (!(id in seen)) mismatch = 1
      exit(mismatch ? 1 : 0)
    }
  ' "$model_only/cases.tsv" "$integrated/cases.tsv" ||
    die "comparison lanes do not use the same case IDs and fixture identities"
  printf 'paired eval lanes verified: suite=%s revision=%s\n' \
    "$(assignment_value "$model_manifest" SUITE_ID)" \
    "$(assignment_value "$model_manifest" SUITE_REVISION)"
}

command="${1:-}"
case "$command" in
  validate)
    [[ $# == 2 ]] || { usage >&2; exit 2; }
    validate_evidence_directory "$2"
    echo "eval evidence verified"
    ;;
  summarize)
    [[ $# == 2 ]] || { usage >&2; exit 2; }
    summarize_evidence "$2"
    ;;
  compare)
    [[ $# == 3 ]] || { usage >&2; exit 2; }
    compare_lanes "$2" "$3"
    ;;
  *)
    usage >&2
    exit 2
    ;;
esac
