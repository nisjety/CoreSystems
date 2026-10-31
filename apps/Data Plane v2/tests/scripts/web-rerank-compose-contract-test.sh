#!/usr/bin/env bash
# =============================================================================
# web-rerank-compose-contract-test.sh  (design task QB-T08)
#
# POST /v1/rerank (Quarry option B) is a paid, service-only route that is DARK until an operator
# switches it on. Its switches, their defaults, the OpenAPI contract, the runbook and the status
# record must agree with the code and with each other. This contract keeps them agreeing. It is a
# sibling of contextual-retrieval-config-contract-test.sh, and it runs on its own.
#
# WHAT IT CHECKS (each check names what it protects)
#   1. The retrieval-engine block of the Data Plane compose carries every web-rerank variable, with
#      the documented default: the route is dark, the processor is not approved, the service
#      allowlist is quarry-edge, the alert threshold is the placeholder, and the provider limits and
#      the breaker are at their safe values.
#   2. Each compose default equals the code's default, read through the serde binding of its Config field
#      changed in one place cannot silently differ from the other.
#   3. RERANK_ENDPOINT carries no public Cohere fallback: the web route accepts only an Azure host
#      (the route rule in rerank_api), and the compose must not hand it a public default.
#   4. No attestation switch is wired in the compose, no attestation.rs lives in rerank_api, and the
#      attestation seam still answers "not attested" by default (the shared module is Z-41's).
#   5. The OpenAPI document names /v1/rerank, its two schemas, the required zdr_mode and the outcome
#      statuses the route answers with.
#   6. The runbook exists and names every switch, the kill switches, the attestation owner (Z-41),
#      the missing price sheet, the quota owner and the ZDR documentation conflict.
#   7. The status record has a web-rerank section.
#   8. The touched files have one line ending each: every line of the compose, the OpenAPI, the status
#      record and the runbook is CRLF (a Windows checkout) or every line is LF (the committed blobs, and
#      a Linux checkout). A file mixed from both fails.
#   9. The read-only pre-flight agrees with this contract (its offline checks pass).
#
# Run from anywhere:   bash tests/scripts/web-rerank-compose-contract-test.sh
# Prove it can fail:   bash tests/scripts/web-rerank-compose-contract-test.sh --self-test
# Exit status: 0 the contract holds, 1 it does not, 2 the command line is wrong.
#
# The files are overridable (WR_COMPOSE, WR_OPENAPI, WR_DOCS, WR_STATUS, WR_CONFIG_RS, WR_RATE_RS,
# WR_RERANK_DIR, WR_PREFLIGHT) so that --self-test can run the real checks over deliberately broken
# copies. Nothing else uses them.
#
# Matching note: every match reads its text into a variable first and uses a here-string. A
# `pipe | grep -q` under pipefail can report a present line as missing (grep stops at its first
# match and the writer gets SIGPIPE), so the contract never uses that shape.
# =============================================================================
set -euo pipefail

script="${BASH_SOURCE[0]}"
data_root="$(cd "$(dirname "$script")/../.." && pwd)"

compose="${WR_COMPOSE:-$data_root/docker-compose.yml}"
openapi="${WR_OPENAPI:-$data_root/openapi/d4d5.yaml}"
docs="${WR_DOCS:-$data_root/docs/web-rerank-v1.md}"
status_doc="${WR_STATUS:-$data_root/DATA_PLANE_STATUS.md}"
config_rs="${WR_CONFIG_RS:-$data_root/services/retrieval-engine-rs/src/config.rs}"
rate_rs="${WR_RATE_RS:-$data_root/services/retrieval-engine-rs/src/rate_limit/mod.rs}"
rerank_dir="${WR_RERANK_DIR:-$data_root/services/retrieval-engine-rs/src/rerank_api}"
preflight="${WR_PREFLIGHT:-$data_root/scripts/qb-preflight.sh}"

failures=0
block=""

fail() {
  printf 'FAIL: %s\n' "$1" >&2
  failures=$((failures + 1))
}

# The retrieval-engine service block (CRLF-safe), from its key to the next service key.
service_block() {
  tr -d '\r' <"$compose" | awk '
    $0 == "  retrieval-engine:" { inside = 1; print; next }
    inside && /^  [a-z][a-z0-9_-]*:$/ { exit }
    inside { print }' || true
}

# The default written for NAME in the block: the text between ":-" and the closing brace.
compose_default() {
  local name=$1
  printf '%s\n' "$block" | sed 's/^[[:space:]]*//' |
    sed -n "s/^$name: \${$name:-\([^}]*\)}[[:space:]]*\$/\1/p" | head -n 1 || true
}

# Booleans compare by meaning: 0/false and 1/true are the same switch value.
normalize_switch() {
  case "$1" in
    0 | false) printf 'false' ;;
    1 | true) printf 'true' ;;
    *) printf '%s' "$1" ;;
  esac
}

# The first quoted literal the function returns (the signature line itself is skipped).
code_default_string() {
  local file=$1 fn=$2 text
  text=$(tr -d '\r' <"$file")
  grep -A3 "fn $fn()" <<<"$text" | sed -n '2,$p' | grep -o '"[^"]*"' | head -n 1 | tr -d '"' || true
}

# The first number the function returns, underscores removed. The signature line is skipped, so the
# `u64` in it is never read as a value.
code_default_number() {
  local file=$1 fn=$2 text
  text=$(tr -d '\r' <"$file")
  grep -A3 "fn $fn()" <<<"$text" | sed -n '2,$p' | grep -oE '[0-9][0-9_]*' | head -n 1 | tr -d '_' || true
}

# The default a float env read uses in code: `.unwrap_or(5.0_f64)`, printed as `5`.
code_unwrap_number() {
  local file=$1 marker=$2 text
  text=$(tr -d '\r' <"$file")
  grep -A4 "\"$marker\"" <<<"$text" | grep -oE 'unwrap_or\([0-9.]+' | head -n 1 |
    sed 's/unwrap_or(//; s/\.0*$//' || true
}

# The serde default function that Config binds to FIELD: the `default = "..."` on the attribute directly above
# `pub FIELD:`. The check follows that binding, so a field rebound to another default is seen.
field_default_fn() {
  local field=$1 text
  text=$(tr -d '\r' <"$config_rs")
  grep -B1 -E "^[[:space:]]*pub $field:" <<<"$text" | grep -oE 'default = "[^"]+"' | head -n 1 |
    sed 's/^default = "//; s/"$//' || true
}

# Line endings: a file is consistent when every line ends in CRLF or every line ends in LF. Which one
# depends on the checkout (a Windows clone with core.autocrlf=true has CRLF; a Linux clone of the
# committed blobs, which are LF, has LF), so the contract accepts either and fails a file that is
# partly one and partly the other, or whose last line has no newline.
eol_check() {
  local file=$1
  node -e '
    const text = require("node:fs").readFileSync(process.argv[1], "utf8");
    const lines = text.split("\n");
    const last = lines.pop();
    const crlf = lines.filter((line) => line.endsWith("\r")).length;
    const consistent = crlf === 0 || crlf === lines.length;
    process.exit(consistent && last.length === 0 ? 0 : 1);
  ' "$file"
}

check_compose() {
  block=$(service_block)
  if [ -z "$block" ]; then
    fail "the retrieval-engine block is missing from the Data Plane compose"
    return
  fi

  # 1. Every switch is present with its documented default.
  local pair name want
  for pair in \
    "WEB_RERANK_ENABLED 0" \
    "WEB_RERANK_PROCESSOR_APPROVED false" \
    "WEB_RERANK_ALLOWED_SERVICES service:quarry-edge" \
    "WEB_RERANK_DAILY_UNIT_BUDGET 10000" \
    "RERANK_PROVIDER_MAX_INFLIGHT 0" \
    "RERANK_PROVIDER_RPS 0" \
    "RERANK_PROVIDER_TENANT_WAIT_MS 1000" \
    "RERANK_BREAKER_FAILURES 5" \
    "RERANK_BREAKER_WINDOW_SECS 30" \
    "RERANK_BREAKER_OPEN_SECS 30" \
    "DPV2_WEB_RERANK_RATE_LIMIT_PER_ORG_RPS 5" \
    "DPV2_WEB_RERANK_RATE_LIMIT_PER_ORG_BURST 10"; do
    name=${pair% *}
    want=${pair#* }
    if [ -z "$(compose_default "$name")" ]; then
      fail "$name is not wired in the retrieval-engine block (the route would read an unset value)"
    elif [ "$(compose_default "$name")" != "$want" ]; then
      fail "$name defaults to $(compose_default "$name"), not $want"
    fi
  done

  # The route is dark by default: the switch and the processor gate both read off.
  if [ "$(normalize_switch "$(compose_default WEB_RERANK_ENABLED)")" != "false" ]; then
    fail "WEB_RERANK_ENABLED defaults on: the paid route would run without the owner's switch"
  fi
  if [ "$(normalize_switch "$(compose_default WEB_RERANK_PROCESSOR_APPROVED)")" != "false" ]; then
    fail "WEB_RERANK_PROCESSOR_APPROVED defaults true: the processor gate would be open without counsel"
  fi

  # 2. Each compose default equals the code's default: the serde default function that the Config field
  #    binds (the attribute on the line above `pub FIELD:`). A field rebound to another default fails here,
  #    even when the function it used to name still reads the old value.
  local code_switch
  code_switch=$(normalize_switch "$(code_default_string "$config_rs" "$(field_default_fn web_rerank_enabled)")")
  [ "$(code_default_string "$config_rs" "$(field_default_fn web_rerank_allowed_services)")" = "$(compose_default WEB_RERANK_ALLOWED_SERVICES)" ] ||
    fail "WEB_RERANK_ALLOWED_SERVICES: compose and config.rs disagree"
  [ "$(code_default_number "$config_rs" "$(field_default_fn web_rerank_daily_unit_budget)")" = "$(compose_default WEB_RERANK_DAILY_UNIT_BUDGET)" ] ||
    fail "WEB_RERANK_DAILY_UNIT_BUDGET: compose and config.rs disagree"
  [ "$(normalize_switch "$(compose_default WEB_RERANK_ENABLED)")" = "$code_switch" ] ||
    fail "WEB_RERANK_ENABLED: compose and the code's switch default disagree"
  [ "$(normalize_switch "$(compose_default WEB_RERANK_PROCESSOR_APPROVED)")" = "$(normalize_switch "$(code_default_string "$config_rs" "$(field_default_fn web_rerank_processor_approved)")")" ] ||
    fail "WEB_RERANK_PROCESSOR_APPROVED: compose and the code's switch default disagree"
  [ "$(compose_default RERANK_PROVIDER_MAX_INFLIGHT)" = "$(code_default_string "$config_rs" "$(field_default_fn rerank_provider_max_inflight)")" ] ||
    fail "RERANK_PROVIDER_MAX_INFLIGHT: compose and config.rs disagree"
  [ "$(compose_default RERANK_PROVIDER_RPS)" = "$(code_default_string "$config_rs" "$(field_default_fn rerank_provider_rps)")" ] ||
    fail "RERANK_PROVIDER_RPS: compose and config.rs disagree"
  [ "$(compose_default RERANK_PROVIDER_TENANT_WAIT_MS)" = "$(code_default_string "$config_rs" "$(field_default_fn rerank_provider_tenant_wait_ms)")" ] ||
    fail "RERANK_PROVIDER_TENANT_WAIT_MS: compose and config.rs disagree"
  [ "$(compose_default RERANK_BREAKER_FAILURES)" = "$(code_default_string "$config_rs" "$(field_default_fn rerank_breaker_failures)")" ] ||
    fail "RERANK_BREAKER_FAILURES: compose and config.rs disagree"
  [ "$(compose_default RERANK_BREAKER_WINDOW_SECS)" = "$(code_default_string "$config_rs" "$(field_default_fn rerank_breaker_window_secs)")" ] ||
    fail "RERANK_BREAKER_WINDOW_SECS: compose and config.rs disagree"
  [ "$(compose_default RERANK_BREAKER_OPEN_SECS)" = "$(code_default_string "$config_rs" "$(field_default_fn rerank_breaker_open_secs)")" ] ||
    fail "RERANK_BREAKER_OPEN_SECS: compose and config.rs disagree"
  [ "$(compose_default DPV2_WEB_RERANK_RATE_LIMIT_PER_ORG_RPS)" = "$(code_unwrap_number "$rate_rs" DPV2_WEB_RERANK_RATE_LIMIT_PER_ORG_RPS)" ] ||
    fail "DPV2_WEB_RERANK_RATE_LIMIT_PER_ORG_RPS: compose and rate_limit/mod.rs disagree"
  [ "$(compose_default DPV2_WEB_RERANK_RATE_LIMIT_PER_ORG_BURST)" = "$(code_unwrap_number "$rate_rs" DPV2_WEB_RERANK_RATE_LIMIT_PER_ORG_BURST)" ] ||
    fail "DPV2_WEB_RERANK_RATE_LIMIT_PER_ORG_BURST: compose and rate_limit/mod.rs disagree"

  # 3. No public Cohere fallback on the endpoint line (the web route refuses non-Azure hosts; the
  #    compose must not supply a public default to it either).
  local endpoint_line
  endpoint_line=$(printf '%s\n' "$block" | sed 's/^[[:space:]]*//' | grep -E '^RERANK_ENDPOINT:' || true)
  if [ -z "$endpoint_line" ]; then
    fail "RERANK_ENDPOINT is not wired in the retrieval-engine block"
  elif grep -Eiq 'cohere\.(ai|com)' <<<"$endpoint_line"; then
    fail "RERANK_ENDPOINT has a public Cohere fallback literal; the web route must name an Azure host or none"
  fi

  # 4. Attestation is data owned by the shared module (Z-41): no compose switch, no local copy.
  if grep -Eq 'RERANK_ZDR_ATTESTATION|WEB_RERANK_ENDPOINT' <<<"$block"; then
    fail "the compose wires an attestation or a web-only endpoint; attestation belongs to the shared module"
  fi
  if [ -e "$rerank_dir/attestation.rs" ]; then
    fail "rerank_api/attestation.rs exists; the attestation is the shared egress-attestation module's (Z-41)"
  fi
  local mod_text
  mod_text=$(tr -d '\r' <"$rerank_dir/mod.rs")
  if ! grep -Fq 'pub struct NoAttestation;' <<<"$mod_text"; then
    fail "the attestation seam no longer answers 'not attested' by default (rerank_api/mod.rs)"
  fi
}

check_openapi() {
  local text schema route
  text=$(tr -d '\r' <"$openapi")
  grep -Eq '^  /v1/rerank:$' <<<"$text" || fail "openapi/d4d5.yaml does not name /v1/rerank"
  grep -Eq '^    RerankRequest:$' <<<"$text" || fail "openapi/d4d5.yaml has no RerankRequest schema"
  grep -Eq '^    RerankResponse:$' <<<"$text" || fail "openapi/d4d5.yaml has no RerankResponse schema"

  # zdr_mode is required on the wire (never defaulted): the RerankRequest required list names it.
  schema=$(awk '
    /^    RerankRequest:$/ { inside = 1; next }
    inside && /^    [A-Za-z]/ { exit }
    inside { print }' <<<"$text" || true)
  grep -Eq 'required:.*zdr_mode' <<<"$schema" || fail "RerankRequest does not require zdr_mode"

  # The route answers 200 for every outcome, so the outcome statuses are documented on the path.
  route=$(awk '
    /^  \/v1\/rerank:$/ { inside = 1; print; next }
    inside && /^  [\/A-Za-z]/ { exit }
    inside { print }' <<<"$text" || true)
  grep -q 'skipped' <<<"$route" || fail "the /v1/rerank path does not document the skipped outcome"
  grep -q 'degraded' <<<"$route" || fail "the /v1/rerank path does not document the degraded outcome"
}

check_docs() {
  if [ ! -f "$docs" ]; then
    fail "the web-rerank runbook docs/web-rerank-v1.md is missing"
    return
  fi
  local text name
  text=$(tr -d '\r' <"$docs")
  for name in \
    WEB_RERANK_ENABLED \
    WEB_RERANK_PROCESSOR_APPROVED \
    WEB_RERANK_ALLOWED_SERVICES \
    WEB_RERANK_DAILY_UNIT_BUDGET \
    DPV2_WEB_RERANK_RATE_LIMIT_PER_ORG_RPS \
    DPV2_WEB_RERANK_RATE_LIMIT_PER_ORG_BURST \
    RERANK_PROVIDER_MAX_INFLIGHT \
    RERANK_PROVIDER_RPS \
    RERANK_PROVIDER_TENANT_WAIT_MS \
    RERANK_BREAKER_FAILURES \
    RERANK_BREAKER_WINDOW_SECS \
    RERANK_BREAKER_OPEN_SECS; do
    grep -Fq -- "$name" <<<"$text" || fail "the runbook does not document $name"
  done
  # What the runbook must say it does not know: the owners, and the missing price sheet.
  grep -Eiq 'kill switch' <<<"$text" || fail "the runbook names no kill switch"
  grep -Fq 'Z-41' <<<"$text" || fail "the runbook does not name the attestation owner (Z-41)"
  grep -Eiq 'price sheet' <<<"$text" || fail "the runbook does not record the missing price sheet"
  grep -Eiq 'quota owner|quota ownership' <<<"$text" || fail "the runbook does not name quota ownership"
  grep -Fq 'sovereign-rag-blueprint-reconciliation.md' <<<"$text" ||
    fail "the runbook does not record the ZDR documentation conflict"
}

check_status() {
  if [ ! -f "$status_doc" ]; then
    fail "DATA_PLANE_STATUS.md has no web-rerank section"
    return
  fi
  local text
  text=$(tr -d '\r' <"$status_doc")
  grep -Eiq '^## .*web rerank' <<<"$text" || fail "DATA_PLANE_STATUS.md has no web-rerank section"
}

check_line_endings() {
  eol_check "$compose" || fail "the Data Plane compose mixes CRLF and LF lines"
  eol_check "$openapi" || fail "openapi/d4d5.yaml mixes CRLF and LF lines"
  if [ -f "$status_doc" ]; then
    eol_check "$status_doc" || fail "DATA_PLANE_STATUS.md mixes CRLF and LF lines"
  fi
  if [ -f "$docs" ]; then
    eol_check "$docs" || fail "docs/web-rerank-v1.md mixes CRLF and LF lines"
  fi
}

check_preflight() {
  if [ ! -f "$preflight" ]; then
    fail "the read-only pre-flight scripts/qb-preflight.sh is missing"
    return
  fi
  bash "$preflight" --offline >/dev/null 2>&1 ||
    fail "scripts/qb-preflight.sh --offline disagrees with the committed registry, the compose or the route"
}

run_checks() {
  failures=0
  check_compose
  check_openapi
  check_docs
  check_status
  check_line_endings
  check_preflight
  [ "$failures" -eq 0 ]
}

# ---------------------------------------------------------------- the self-test
# The real files must pass. Then each deliberate defect is applied to a fresh copy of the real files,
# and the same checks must FAIL on it.
mutant_route_on() {
  sed -i 's/WEB_RERANK_ENABLED: ${WEB_RERANK_ENABLED:-0}/WEB_RERANK_ENABLED: ${WEB_RERANK_ENABLED:-1}/' "$1"
}
mutant_processor_open() {
  sed -i 's/WEB_RERANK_PROCESSOR_APPROVED: ${WEB_RERANK_PROCESSOR_APPROVED:-false}/WEB_RERANK_PROCESSOR_APPROVED: ${WEB_RERANK_PROCESSOR_APPROVED:-true}/' "$1"
}
mutant_public_cohere() {
  sed -i 's|RERANK_ENDPOINT: ${RERANK_ENDPOINT:-${COHERE_URL:-}}|RERANK_ENDPOINT: ${RERANK_ENDPOINT:-https://api.cohere.ai/v1/rerank}|' "$1"
}
mutant_budget_unwired() {
  sed -i '/WEB_RERANK_DAILY_UNIT_BUDGET:/d' "$1"
}
mutant_attestation_switch() {
  node -e '
    const fs = require("node:fs");
    const file = process.argv[1];
    const text = fs.readFileSync(file, "utf8");
    const eol = text.includes("\r\n") ? "\r\n" : "\n";
    const anchor = "      RERANK_TOP_K: ${RERANK_TOP_K:-50}" + eol;
    if (!text.includes(anchor)) process.exit(3);
    fs.writeFileSync(file, text.replace(anchor, anchor + "      RERANK_ZDR_ATTESTATION_ENABLED: \"1\"" + eol));
  ' "$1"
}
mutant_switch_rebound() {
  # The switch field is bound to another serde default. The old function still reads "false" in config.rs,
  # so only the binding shows the change.
  node -e '
    const fs = require("node:fs");
    const file = process.argv[1];
    const text = fs.readFileSync(file, "utf8");
    const pattern = /default = "default_switch_off"\)\]\r?\n(\s*pub web_rerank_enabled:)/;
    if (!pattern.test(text)) process.exit(3);
    fs.writeFileSync(file, text.replace(pattern, (match) => match.replace("default_switch_off", "default_switch_on")));
  ' "$1"
}
mutant_default_drift() {
  sed -i 's/RERANK_BREAKER_OPEN_SECS: ${RERANK_BREAKER_OPEN_SECS:-30}/RERANK_BREAKER_OPEN_SECS: ${RERANK_BREAKER_OPEN_SECS:-60}/' "$1"
}
mutant_openapi_route() {
  sed -i 's|^  /v1/rerank:|  /v1/rerankx:|' "$1"
}
mutant_runbook_switch() {
  sed -i '/WEB_RERANK_ALLOWED_SERVICES/d' "$1"
}
mutant_bare_lf() {
  node -e '
    const fs = require("node:fs");
    const file = process.argv[1];
    const text = fs.readFileSync(file, "utf8");
    // One line ends the other way round, so the file is mixed on a CRLF checkout and on an LF one.
    const mixed = text.includes("\r\n") ? text.replace("\r\n", "\n") : text.replace("\n", "\r\n");
    if (mixed === text) process.exit(3);
    fs.writeFileSync(file, mixed);
  ' "$1"
}

self_test() {
  local tmp
  tmp=$(mktemp -d)
  trap 'rm -rf "$tmp"' RETURN
  mkdir -p "$tmp/copy"

  # The copies, refreshed before every run, so each defect is the only change in its run.
  refresh_copies() {
    cp "$compose" "$tmp/copy/compose.yml"
    cp "$openapi" "$tmp/copy/openapi.yaml"
    cp "$status_doc" "$tmp/copy/status.md"
    cp "$config_rs" "$tmp/copy/config.rs"
    if [ -f "$docs" ]; then cp "$docs" "$tmp/copy/docs.md"; else rm -f "$tmp/copy/docs.md"; fi
  }
  run_on_copies() {
    WR_COMPOSE="$tmp/copy/compose.yml" \
      WR_OPENAPI="$tmp/copy/openapi.yaml" \
      WR_DOCS="$tmp/copy/docs.md" \
      WR_STATUS="$tmp/copy/status.md" \
      WR_CONFIG_RS="$tmp/copy/config.rs" \
      bash "$script" --check-only >"$tmp/out" 2>&1
  }

  refresh_copies
  if ! run_on_copies; then
    printf 'SELF-TEST FAIL: the real files do not pass the contract\n' >&2
    cat "$tmp/out" >&2
    return 1
  fi

  local mutant
  local -a mutants=(
    mutant_route_on
    mutant_processor_open
    mutant_public_cohere
    mutant_budget_unwired
    mutant_attestation_switch
    mutant_default_drift
    mutant_switch_rebound
    mutant_openapi_route
    mutant_runbook_switch
    mutant_bare_lf
  )
  for mutant in "${mutants[@]}"; do
    refresh_copies
    case "$mutant" in
      mutant_openapi_route) "$mutant" "$tmp/copy/openapi.yaml" ;;
      mutant_runbook_switch) [ -f "$tmp/copy/docs.md" ] && "$mutant" "$tmp/copy/docs.md" ;;
      mutant_bare_lf) "$mutant" "$tmp/copy/compose.yml" ;;
      mutant_switch_rebound) "$mutant" "$tmp/copy/config.rs" ;;
      *) "$mutant" "$tmp/copy/compose.yml" ;;
    esac
    if run_on_copies; then
      printf 'SELF-TEST FAIL: the contract accepted a copy with defect %s\n' "$mutant" >&2
      return 1
    fi
  done
  printf 'self-test: the contract holds on the real files and refuses %d deliberate defects\n' "${#mutants[@]}"
  return 0
}

case "${1:-}" in
  "")
    if run_checks; then
      echo "PASS: web-rerank compose contract"
      exit 0
    fi
    echo "FAIL: web-rerank compose contract ($failures check(s))" >&2
    exit 1
    ;;
  --check-only)
    run_checks
    ;;
  --self-test)
    if self_test; then
      echo "PASS: web-rerank compose contract self-test"
      exit 0
    fi
    exit 1
    ;;
  *)
    echo "usage: $0 [--self-test]" >&2
    exit 2
    ;;
esac
