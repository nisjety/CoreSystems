#!/usr/bin/env bash
# =============================================================================
# rerank-input-contract-test.sh  (design task CR-T15)
#
# RERANK_INPUT_INCLUDE_CONTEXT decides whether the reranker judges each chunk with its context in force.
# It is off by default, read as text, and the retrieval-quality harness reads the same name from the
# running retrieval-engine container. This contract keeps the places that name it agreeing: the compose
# default, the .env.example default, the code's default, the harness's reader and the runbook row.
#
# WHAT IT CHECKS (each check names what it protects)
#   1. The retrieval-engine block of the Data Plane compose passes the variable with the default 0. A
#      missing or different default would switch the reranker's input mode on a fresh deployment.
#   2. .env.example sets it to 0, once, so an operator who copies the example gets the same default.
#   3. The config field reads the variable through `default_switch_off`, and that default is "false": the
#      code's own default is off, whatever the compose file says.
#   4. The eval harness reads the same variable, with the same default, and accepts only 0 or 1. The
#      promotion gate compares that reading with what the variant declares, so a different name would
#      measure the wrong state.
#   5. The runbook's switch table names the variable.
#
# Run from anywhere:   bash tests/scripts/rerank-input-contract-test.sh
# Exit status: 0 the contract holds, 1 it does not.
# The files are overridable (RI_COMPOSE, RI_ENV_EXAMPLE, RI_CONFIG_RS, RI_EVAL_SERVICE, RI_RUNBOOK) so that
# a deliberately broken copy can prove the contract fails. Nothing else uses them.
#
# Matching note: every match reads its text into a variable first and uses a here-string. A
# `pipe | grep -q` under pipefail can report a present line as missing, so the contract never uses it.
# =============================================================================
set -euo pipefail

script="${BASH_SOURCE[0]}"
data_root="$(cd "$(dirname "$script")/../.." && pwd)"

compose="${RI_COMPOSE:-$data_root/docker-compose.yml}"
env_example="${RI_ENV_EXAMPLE:-$data_root/.env.example}"
config_rs="${RI_CONFIG_RS:-$data_root/services/retrieval-engine-rs/src/config.rs}"
eval_service="${RI_EVAL_SERVICE:-$data_root/scripts/eval-service.py}"
runbook="${RI_RUNBOOK:-$data_root/docs/contextual-retrieval-runbook.md}"

name="RERANK_INPUT_INCLUDE_CONTEXT"
failures=0

fail() {
  printf 'FAIL: %s\n' "$1" >&2
  failures=$((failures + 1))
}

# A file's text with CRLF removed, so the checks read the same on a Windows checkout and on Linux.
text_of() {
  tr -d '\r' <"$1"
}

# The retrieval-engine service block, from its key to the next service key.
service_block() {
  tr -d '\r' <"$compose" | awk '
    $0 == "  retrieval-engine:" { inside = 1; print; next }
    inside && /^  [a-z][a-z0-9_-]*:$/ { exit }
    inside { print }' || true
}

# 1. Compose: the retrieval-engine block passes the variable with the default 0.
block="$(service_block)"
if ! grep -Fxq -- "      $name: \${$name:-0}" <<<"$block"; then
  fail "compose: the retrieval-engine block does not pass $name with the default 0"
fi

# 2. .env.example: the variable is set to 0, exactly once.
example_text="$(text_of "$env_example")"
if ! grep -Fxq -- "$name=0" <<<"$example_text"; then
  fail ".env.example: $name is not set to 0"
fi
occurrences="$(grep -c -- "^$name=" <<<"$example_text" || true)"
if [ "$occurrences" != "1" ]; then
  fail ".env.example: $name is set $occurrences times, not once"
fi

# 3. Code: the field takes the switch default, and that default is off.
config_text="$(text_of "$config_rs")"
if ! awk '
  prev ~ /#\[serde\(default = "default_switch_off"\)\]/ && index($0, "pub rerank_input_include_context: String,") { found = 1 }
  { prev = $0 }
  END { exit !found }' <<<"$config_text"; then
  fail "config.rs: rerank_input_include_context is not read through default_switch_off"
fi
if ! awk '
  /fn default_switch_off\(\) -> String \{/ { getline nxt; if (nxt ~ /"false"\.into\(\)/) ok = 1 }
  END { exit !ok }' <<<"$config_text"; then
  fail "config.rs: default_switch_off is not \"false\", so the code's default is not off"
fi

# 4. Harness: reads the same variable with the same default, and accepts only 0 or 1.
eval_text="$(text_of "$eval_service")"
if ! grep -Fq -- "$name:-0" <<<"$eval_text"; then
  fail "eval-service.py: does not read $name with the default 0"
fi
if ! grep -Fq -- 'value in ("0", "1")' <<<"$eval_text"; then
  fail "eval-service.py: does not refuse a value other than 0 or 1"
fi

# 5. Runbook: the switch table has a row for the variable.
runbook_text="$(text_of "$runbook")"
if ! grep -Fq -- "| \`$name\`" <<<"$runbook_text"; then
  fail "runbook: no row in the switch table for $name"
fi

if [ "$failures" -ne 0 ]; then
  printf 'rerank input contract: %d check(s) failed\n' "$failures" >&2
  exit 1
fi
echo "rerank input contract holds: $name defaults to 0 in compose, .env.example and config.rs; the harness reads the same name"
