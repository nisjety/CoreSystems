#!/usr/bin/env bash
# =============================================================================
# background-budget-contract-test.sh  (design task CR-T9b)
#
# The Data Plane's background LLM budget (owner default D-E) is read by TWO services,
# embedding-engine (contextual retrieval) and graph-index (graph extraction and community
# summaries), from the same four variables, and measured as ONE figure. This contract keeps the
# deployment side of that honest: what the two compose blocks pass, what `.env.example` tells an
# operator, that nothing is built in, and that the one metric the gate emits can be scraped and
# alerted on. It is a sibling of contextual-retrieval-config-contract-test.sh (and, like it, does
# not edit compose-security-contract-test.sh, which belongs to CR-T13 and CR-T5).
#
# WHY IT EXISTS
#   Two consumers of one budget are two chances to disagree: a default that differs between the
#   blocks (one service stops where the other alerts), a figure or a price that one of them has
#   built in (the owner supplies the figure; none is invented), a variable one service never
#   receives (an operator sets it and half of the work ignores it), an example that documents a
#   different default than the compose file runs. The Rust side holds the code to the same
#   spelling and the same SQL (`budget::tests::the_settings_are_spelled_the_way_graph_index_spells_them`
#   and `the_spend_statement_is_the_one_graph_index_runs`); this holds the deployment to it.
#
# WHAT IT CHECKS (every check names what it protects)
#   1. The embedding-engine AND the graph-index blocks of the Data Plane compose pass all four
#      variables, with the SAME documented defaults: `alert_only`, and blank for the figure and
#      both prices (nothing built in).
#   2. `.env.example` documents each of the four, with the same default, and does not set a
#      figure, a price or a mode other than alert_only: copied to `.env` it must not invent a
#      budget the owner has not given.
#   3. Both services read the four variables in their configuration (a variable the compose file
#      passes and nothing reads is a setting that does nothing).
#   4. The metric: embedding-engine serves `ctx_budget_exceeded_total` on `/metrics`, Prometheus
#      scrapes it, and an alert rule is written on it. (Prometheus itself is the compose file's
#      optional `obs` profile; the contract is that the pieces exist and agree.)
#
# Run from anywhere:   bash tests/scripts/background-budget-contract-test.sh
# Prove it can fail:   bash tests/scripts/background-budget-contract-test.sh --self-test
# Exit status: 0 the contract holds, 1 it does not, 2 the command line is wrong.
#
# The files are overridable (BB_COMPOSE, BB_ENV_EXAMPLE, BB_ENGINE_CONFIG, BB_GRAPH_CONFIG,
# BB_ENGINE_API, BB_PROMETHEUS, BB_ALERTS) so that --self-test can run the real checks over
# deliberately broken copies; nothing else uses them.
# =============================================================================
set -euo pipefail

script="${BASH_SOURCE[0]}"
data_root="$(cd "$(dirname "$script")/../.." && pwd)"

compose="${BB_COMPOSE:-$data_root/docker-compose.yml}"
env_example="${BB_ENV_EXAMPLE:-$data_root/.env.example}"
engine_config="${BB_ENGINE_CONFIG:-$data_root/services/embedding-engine-rs/src/config.rs}"
graph_config="${BB_GRAPH_CONFIG:-$data_root/services/graph-index-rs/src/config.rs}"
engine_api="${BB_ENGINE_API:-$data_root/services/embedding-engine-rs/src/api/mod.rs}"
prometheus="${BB_PROMETHEUS:-$data_root/infra/prometheus/prometheus.yml}"
alerts="${BB_ALERTS:-$data_root/infra/prometheus/alerts.yml}"

# ---------------------------------------------------------------- the self-test

if [ "${1:-}" = "--self-test" ]; then
  [ "$#" -eq 1 ] || { echo "usage: $0 [--self-test]" >&2; exit 2; }
  work="$(mktemp -d)"
  trap 'rm -rf "$work"' EXIT
  failures=0

  if ! bash "$script" >"$work/clean.out" 2>&1; then
    cat "$work/clean.out" >&2
    echo "SELFTEST FAIL: the real files do not satisfy the contract" >&2
    failures=$((failures + 1))
  fi

  expect_fail() { # LABEL NEEDLE -- run with whatever BB_* the caller exported: must exit 1 and say NEEDLE
    local label="$1" needle="$2"
    if bash "$script" >"$work/mutant.out" 2>&1; then
      echo "SELFTEST FAIL: $label: the contract still passes" >&2
      failures=$((failures + 1))
    elif ! grep -Fq "$needle" "$work/mutant.out"; then
      echo "SELFTEST FAIL: $label: it failed, but not with '$needle':" >&2
      sed 's/^/    /' "$work/mutant.out" >&2
      failures=$((failures + 1))
    fi
  }
  mutate() { # FILE SED-EXPRESSION -> path of the broken copy
    local out="$work/mutant.$RANDOM"
    tr -d '\r' <"$1" | sed -E "$2" >"$out"
    printf '%s' "$out"
  }

  # The deployment's defaults must be the same in both blocks, and must build nothing in. The
  # first occurrence of each line is the embedding-engine block, the second graph-index's.
  mutant="$(mutate "$compose" '0,/(BACKGROUND_BUDGET_MODE: \$\{BACKGROUND_BUDGET_MODE:-)alert_only\}/s//\1stop}/')"
  BB_COMPOSE="$mutant" expect_fail "the embedding engine defaults to stop" "BACKGROUND_BUDGET_MODE"
  mutant="$(mutate "$compose" '0,/(BACKGROUND_BUDGET_MONTHLY_USD: \$\{BACKGROUND_BUDGET_MONTHLY_USD:-)\}/s//\125}/')"
  BB_COMPOSE="$mutant" expect_fail "a figure is built into the embedding engine" "BACKGROUND_BUDGET_MONTHLY_USD"
  mutant="$(mutate "$compose" '0,/(BACKGROUND_PRICE_INPUT_USD_PER_MTOK: \$\{BACKGROUND_PRICE_INPUT_USD_PER_MTOK:-)\}/s//\10.15}/')"
  BB_COMPOSE="$mutant" expect_fail "a price is built into the embedding engine" "BACKGROUND_PRICE_INPUT_USD_PER_MTOK"
  mutant="$(mutate "$compose" '0,/BACKGROUND_PRICE_OUTPUT_USD_PER_MTOK: .*/{//d}')"
  BB_COMPOSE="$mutant" expect_fail "the embedding engine never receives a price" "BACKGROUND_PRICE_OUTPUT_USD_PER_MTOK"
  # The second occurrence (graph-index): delete every line of the variable but the first.
  mutant="$(mutate "$compose" '1,/BACKGROUND_BUDGET_MODE:/!{/BACKGROUND_BUDGET_MODE:/d}')"
  BB_COMPOSE="$mutant" expect_fail "graph-index never receives the mode" "graph-index"

  mutant="$(mutate "$env_example" '/^BACKGROUND_BUDGET_MONTHLY_USD=/d')"
  BB_ENV_EXAMPLE="$mutant" expect_fail "a variable is not documented in the example" "BACKGROUND_BUDGET_MONTHLY_USD"
  mutant="$(mutate "$env_example" 's/^BACKGROUND_BUDGET_MODE=.*/BACKGROUND_BUDGET_MODE=stop/')"
  BB_ENV_EXAMPLE="$mutant" expect_fail "the example sets a stop" "BACKGROUND_BUDGET_MODE"
  mutant="$(mutate "$env_example" 's/^BACKGROUND_BUDGET_MONTHLY_USD=.*/BACKGROUND_BUDGET_MONTHLY_USD=50/')"
  BB_ENV_EXAMPLE="$mutant" expect_fail "the example builds a figure in" "BACKGROUND_BUDGET_MONTHLY_USD"

  mutant="$(mutate "$engine_config" 's/background_price_output_usd_per_mtok/background_price_renamed/')"
  BB_ENGINE_CONFIG="$mutant" expect_fail "the embedding engine no longer reads a price" "background_price_output_usd_per_mtok"
  mutant="$(mutate "$graph_config" 's/background_budget_mode/background_mode_renamed/')"
  BB_GRAPH_CONFIG="$mutant" expect_fail "graph-index no longer reads the mode" "background_budget_mode"

  mutant="$(mutate "$engine_api" '/\.route\("\/metrics"/d')"
  BB_ENGINE_API="$mutant" expect_fail "the metrics route is gone" "/metrics"
  mutant="$(mutate "$prometheus" '/embedding-engine:9202/d')"
  BB_PROMETHEUS="$mutant" expect_fail "Prometheus no longer scrapes the embedding engine" "embedding-engine"
  mutant="$(mutate "$alerts" '/ctx_budget_exceeded_total/d')"
  BB_ALERTS="$mutant" expect_fail "no alert is written on the metric" "ctx_budget_exceeded_total"

  if [ "$failures" -ne 0 ]; then
    echo "background budget contract self-test: $failures failure(s)" >&2
    exit 1
  fi
  echo "selftest: the contract passes on the real files and fails on each way of breaking it"
  exit 0
fi
[ "$#" -eq 0 ] || { echo "usage: $0 [--self-test]" >&2; exit 2; }

# ---------------------------------------------------------------- the contract

failures=0
fail() { printf 'FAIL: %s\n' "$*" >&2; failures=$((failures + 1)); }

for file in "$compose" "$env_example" "$engine_config" "$graph_config" "$engine_api" "$prometheus" "$alerts"; do
  [ -f "$file" ] || { printf 'FAIL: %s is missing\n' "$file" >&2; exit 1; }
done

# The text without CR (a Windows checkout) and without comment lines: a sentence that explains a
# variable must not stand in for the variable.
active() { tr -d '\r' <"$1" | sed -E '/^[[:space:]]*#/d'; }

# One service's block: from its key to the next service key (two-space indent). awk reads ALL of
# its input and never `exit`s early (see the sibling contract: SIGPIPE under pipefail).
service_block() {
  active "$compose" | awk -v name="$1" '
    !done && $0 ~ "^  " name ":[[:space:]]*$" { inside = 1; print; next }
    inside && /^  [A-Za-z0-9_-]+:[[:space:]]*$/ { inside = 0; done = 1 }
    inside { print }'
}

# 1. The four variables, passed by both services, with the same defaults and nothing built in.
variables='
BACKGROUND_BUDGET_MODE|alert_only
BACKGROUND_BUDGET_MONTHLY_USD|
BACKGROUND_PRICE_INPUT_USD_PER_MTOK|
BACKGROUND_PRICE_OUTPUT_USD_PER_MTOK|
'
for service in embedding-engine graph-index; do
  block="$(service_block "$service")"
  [ -n "$block" ] || { printf 'FAIL: no %s service in %s\n' "$service" "$compose" >&2; exit 1; }
  while IFS='|' read -r key want; do
    [ -n "$key" ] || continue
    line="$(grep -E "^[[:space:]]*${key}:" <<<"$block" || true)"
    line="${line#*:}"
    line="${line#"${line%%[![:space:]]*}"}"
    line="${line%"${line##*[![:space:]]}"}"
    if [ -z "$line" ]; then
      fail "the $service block does not pass $key through: both services read the background budget, and a variable one of them never receives is a setting that holds for half the work"
      continue
    fi
    expect="\${${key}:-${want}}"
    if [ "$line" != "$expect" ]; then
      fail "$service: $key is '$line', want '$expect' (the owner supplies the figure and the prices, and the default is alert only: nothing is built in)"
    fi
  done <<<"$variables"
done

# 2. `.env.example` documents each, with the same default, and builds nothing in.
while IFS='|' read -r key want; do
  [ -n "$key" ] || continue
  line="$(active "$env_example" | grep -E "^${key}=" || true)"
  if [ -z "$line" ]; then
    fail "$key is not documented in .env.example (an operator cannot know it exists)"
    continue
  fi
  value="${line#*=}"
  if [ "$value" != "$want" ]; then
    fail "$key: .env.example says '$value', the compose default is '$want' (a copy of the example must not invent a budget, a price or a stop the owner has not given)"
  fi
done <<<"$variables"

# 3. Both services read the four variables in their configuration.
for pair in "embedding-engine:$engine_config" "graph-index:$graph_config"; do
  service="${pair%%:*}"
  file="${pair#*:}"
  for field in background_budget_mode background_budget_monthly_usd background_price_input_usd_per_mtok background_price_output_usd_per_mtok; do
    if ! grep -Eq "^[[:space:]]*pub[[:space:]]+${field}:" "$file"; then
      fail "the $service configuration has no field $field: the compose file passes a variable nothing reads"
    fi
  done
done

# 4. The metric: served, scraped, alerted on.
grep -Eq '\.route\("/metrics"' "$engine_api" || fail "the embedding engine's admin router has no /metrics route: ctx_budget_exceeded_total cannot be scraped"
grep -Fq 'embedding-engine:9202' "$prometheus" || fail "Prometheus does not scrape the embedding engine (embedding-engine:9202): the counter is emitted into the void"
grep -Fq 'ctx_budget_exceeded_total' "$alerts" || fail "no alert rule is written on ctx_budget_exceeded_total: a budget that is exceeded alerts nobody"

if [ "$failures" -ne 0 ]; then
  printf 'background budget contract: %d check(s) failed\n' "$failures" >&2
  exit 1
fi
echo "PASS: the background budget is passed, documented and defaulted the same way for both services, builds nothing in, and its metric is served, scraped and alerted on"
