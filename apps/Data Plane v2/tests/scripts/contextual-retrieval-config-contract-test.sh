#!/usr/bin/env bash
# =============================================================================
# contextual-retrieval-config-contract-test.sh  (design task CR-T13)
#
# Contextual retrieval is ON by default and the global flag is an incident-only kill
# switch (owner ruling of 2026-10-03). This contract keeps the deployment side of that
# ruling honest: what the compose file defaults to, what `.env.example` tells an operator,
# what the disposable test stacks do about it, and what the comments say.
#
# WHY IT EXISTS
#   Until CR-T13 the feature was off in the code and in the compose file and ran on the
#   live stack only because the git-ignored `.env` said so. Documents, comments and a
#   migration called it "OFF by default" while it was running, and a fresh environment
#   (a new machine, the CI smoke stack, a recreated container with a lost `.env`) silently
#   had no contexts. The defaults are now written in two places that must agree (the code,
#   checked by the Rust test `the_compose_defaults_are_the_code_defaults_and_contextual_
#   retrieval_is_on`, and the compose file, checked here) and documented in a third
#   (`.env.example`, checked here). It is a sibling of compose-security-contract-test.sh,
#   which calls it, so it runs on its own as well.
#
# WHAT IT CHECKS (every check names what it protects)
#   1. The embedding-engine block of the Data Plane compose carries every contextual
#      retrieval variable, with the documented default: the switch is `true`, the model is
#      `gpt-4o-mini` (the shared deployment; a dedicated background deployment alias is an
#      operator action that changes this one variable), the provider is pinned, the
#      residency floor is `eu`, the reconciler, the prompt refresh and the usage ledger
#      are dark, and the kill-switch reason is empty.
#   2. Every such variable is documented in `.env.example` with the SAME default, so an
#      operator who copies the example gets the defaults and not a different system.
#   3. `.env.example` does not throw the kill switch.
#   4. Every harness that brings the base compose up turns contextual retrieval off
#      EXPLICITLY, with a reason: the isolated stacks have no inference-core, and a boot
#      check that stopped them would hide behind a green "skipped".
#   5. No comment or document of the embedding engine, its compose block or its example
#      environment calls contextual retrieval "off by default", and none names the column
#      `contextualized_text`, which does not exist (the column is `chunk_context`).
#   6. `main` really runs the boot check (`provider::contextualize::check_boot`) before the
#      event consumers are built. The Rust tests prove what the check refuses; none of them
#      would notice a `main` that stopped calling it, and a service whose feature is on and
#      whose credential is missing would then run for weeks with the consumer ended quietly.
#
# Run from anywhere:   bash tests/scripts/contextual-retrieval-config-contract-test.sh
# Prove it can fail:   bash tests/scripts/contextual-retrieval-config-contract-test.sh --self-test
# Exit status: 0 the contract holds, 1 it does not, 2 the command line is wrong.
#
# The files are overridable (CR_CONFIG_COMPOSE, CR_CONFIG_ENV_EXAMPLE, CR_CONFIG_HARNESSES,
# CR_CONFIG_CODE_FILES, CR_CONFIG_MAIN) so that --self-test can run the real checks over
# deliberately broken copies; nothing else uses them.
# =============================================================================
set -euo pipefail

script="${BASH_SOURCE[0]}"
data_root="$(cd "$(dirname "$script")/../.." && pwd)"

compose="${CR_CONFIG_COMPOSE:-$data_root/docker-compose.yml}"
env_example="${CR_CONFIG_ENV_EXAMPLE:-$data_root/.env.example}"
main_rs="${CR_CONFIG_MAIN:-$data_root/services/embedding-engine-rs/src/main.rs}"

default_harnesses=(
  "$data_root/tests/e2e/run-isolated-mvp.sh"
  "$data_root/tests/scripts/run-isolated-real-authority.sh"
  "$data_root/tests/scripts/run-isolated-real-authority-browser.sh"
)
if [ -n "${CR_CONFIG_HARNESSES:-}" ]; then
  read -r -a harnesses <<<"$CR_CONFIG_HARNESSES"
else
  harnesses=("${default_harnesses[@]}")
fi

if [ -n "${CR_CONFIG_CODE_FILES:-}" ]; then
  read -r -a code_files <<<"$CR_CONFIG_CODE_FILES"
else
  code_files=("$compose" "$env_example")
  while IFS= read -r source; do
    code_files+=("$source")
  done < <(find "$data_root/services/embedding-engine-rs/src" -name '*.rs' 2>/dev/null | sort)
fi

# ---------------------------------------------------------------- the self-test

if [ "${1:-}" = "--self-test" ]; then
  [ "$#" -eq 1 ] || { echo "usage: $0 [--self-test]" >&2; exit 2; }
  work="$(mktemp -d)"
  trap 'rm -rf "$work"' EXIT
  failures=0

  # The real files pass.
  if ! bash "$script" >"$work/clean.out" 2>&1; then
    cat "$work/clean.out" >&2
    echo "SELFTEST FAIL: the real files do not satisfy the contract" >&2
    failures=$((failures + 1))
  fi

  # expect_fail LABEL NEEDLE -- run with whatever CR_CONFIG_* the caller exported: must exit 1
  # and say NEEDLE.
  : >"$work/nothing-stale.txt"
  expect_fail() {
    local label="$1" needle="$2"
    # Only the stale-statement mutants set CR_CONFIG_CODE_FILES; for the others the scan of the
    # source tree is a cost without a purpose, so it is pointed at an empty file.
    if CR_CONFIG_CODE_FILES="${CR_CONFIG_CODE_FILES:-$work/nothing-stale.txt}" bash "$script" >"$work/mutant.out" 2>&1; then
      echo "SELFTEST FAIL: $label: the contract still passes" >&2
      failures=$((failures + 1))
    elif ! grep -Fq "$needle" "$work/mutant.out"; then
      echo "SELFTEST FAIL: $label: it failed, but not with '$needle':" >&2
      sed 's/^/    /' "$work/mutant.out" >&2
      failures=$((failures + 1))
    fi
  }

  # Break the compose file one way at a time.
  mutate_compose() { # SED-EXPRESSION -> path
    local out="$work/compose.$RANDOM.yml"
    tr -d '\r' <"$compose" | sed -E "$1" >"$out"
    printf '%s' "$out"
  }
  mutate_example() {
    local out="$work/env.$RANDOM.example"
    tr -d '\r' <"$env_example" | sed -E "$1" >"$out"
    printf '%s' "$out"
  }

  mutant="$(mutate_compose 's/(CONTEXTUAL_RETRIEVAL_ENABLED: \$\{CONTEXTUAL_RETRIEVAL_ENABLED:-)true\}/\1false}/')"
  CR_CONFIG_COMPOSE="$mutant" expect_fail "the compose switch defaults to off" "CONTEXTUAL_RETRIEVAL_ENABLED"

  mutant="$(mutate_compose 's/(CONTEXTUAL_RETRIEVAL_MODEL: \$\{CONTEXTUAL_RETRIEVAL_MODEL:-)gpt-4o-mini\}/\1}/')"
  CR_CONFIG_COMPOSE="$mutant" expect_fail "the compose model has no default" "CONTEXTUAL_RETRIEVAL_MODEL"

  mutant="$(mutate_compose '/CONTEXT_INLINE_DOCUMENT_CONCURRENCY:/d')"
  CR_CONFIG_COMPOSE="$mutant" expect_fail "a variable the code reads is not passed through" "CONTEXT_INLINE_DOCUMENT_CONCURRENCY"

  mutant="$(mutate_compose '/CONTEXT_RETRIEVAL_KILL_SWITCH_REASON:/d')"
  CR_CONFIG_COMPOSE="$mutant" expect_fail "the kill-switch reason is not passed through" "CONTEXT_RETRIEVAL_KILL_SWITCH_REASON"

  mutant="$(mutate_compose 's/(CONTEXT_RECONCILER_ENABLED: \$\{CONTEXT_RECONCILER_ENABLED:-)0\}/\11}/')"
  CR_CONFIG_COMPOSE="$mutant" expect_fail "the reconciler is on by default" "CONTEXT_RECONCILER_ENABLED"

  mutant="$(mutate_example '/^CONTEXT_RECONCILER_SWEEP_SECS=/d')"
  CR_CONFIG_ENV_EXAMPLE="$mutant" expect_fail "a variable is not documented in the example" "CONTEXT_RECONCILER_SWEEP_SECS"

  mutant="$(mutate_example 's/^CONTEXTUAL_RETRIEVAL_MODEL=.*/CONTEXTUAL_RETRIEVAL_MODEL=gpt-5/')"
  CR_CONFIG_ENV_EXAMPLE="$mutant" expect_fail "the example documents another default" "CONTEXTUAL_RETRIEVAL_MODEL"

  mutant="$(mutate_example 's/^CONTEXTUAL_RETRIEVAL_ENABLED=.*/CONTEXTUAL_RETRIEVAL_ENABLED=false/')"
  CR_CONFIG_ENV_EXAMPLE="$mutant" expect_fail "the example throws the kill switch" "CONTEXTUAL_RETRIEVAL_ENABLED"

  # A harness that leaves contextual retrieval on, or switches it off without saying why.
  printf '#!/usr/bin/env bash\ndocker compose -f "$ROOT/docker-compose.yml" up -d\n' >"$work/harness-silent.sh"
  CR_CONFIG_HARNESSES="$work/harness-silent.sh" expect_fail "a harness leaves contextual retrieval on" "harness-silent.sh"
  printf '#!/usr/bin/env bash\nexport CONTEXTUAL_RETRIEVAL_ENABLED=false\n' >"$work/harness-no-reason.sh"
  CR_CONFIG_HARNESSES="$work/harness-no-reason.sh" expect_fail "a harness switches it off without a reason" "CONTEXT_RETRIEVAL_KILL_SWITCH_REASON"

  # Stale statements, in a comment of each of the three comment styles.
  printf '    // Contextual retrieval is OFF by default here.\n    fn x() {}\n' >"$work/stale.rs"
  CR_CONFIG_CODE_FILES="$work/stale.rs" expect_fail "a Rust comment says it is off by default" "off by default"
  # The same defect in the words the consumer's comment used (`None` when the feature is off, which
  # is the default), which the first rule did not match until a review found it left in stream/mod.rs.
  printf '    // Built once. `None` when the feature is off, which is the default: see contextualize.\n    fn x() {}\n' >"$work/stale-which.rs"
  CR_CONFIG_CODE_FILES="$work/stale-which.rs" expect_fail "a Rust comment says it is off, which is the default" "off by default"
  printf '# Contextual Retrieval (Anthropic, 2024)\n# OFF by default, and that default is the cost gate.\nKEY=1\n' >"$work/stale.env"
  CR_CONFIG_CODE_FILES="$work/stale.env" expect_fail "a comment of the example or the compose says it is off by default" "off by default"
  # The original defect: "OFF by default" in a paragraph that never names the feature, because the
  # paragraph above it, in the same block above the switch, does (the compose file and the config).
  printf '      # Contextual Retrieval prepends context to each chunk.\n      #\n      # OFF by default, and that default is the cost gate.\n      CONTEXTUAL_RETRIEVAL_ENABLED: ${CONTEXTUAL_RETRIEVAL_ENABLED:-false}\n' >"$work/block.yml"
  CR_CONFIG_CODE_FILES="$work/block.yml" expect_fail "the block above the switch says it is off by default" "above the contextual retrieval switch"
  printf '    // Contextual Retrieval prepends context to each chunk.\n    //\n    // OFF by default, and the default is load-bearing.\n    #[serde(default)]\n    pub contextual_retrieval_enabled: bool,\n' >"$work/block.rs"
  CR_CONFIG_CODE_FILES="$work/block.rs" expect_fail "the block above the config field says it is off by default" "above the contextual retrieval switch"
  printf 'const NOTE: &str = "Requires the migration adding knowledge_units.contextualized_text.";\n' >"$work/column.rs"
  CR_CONFIG_CODE_FILES="$work/column.rs" expect_fail "a file names a column that does not exist" "contextualized_text"
  # A `main` that no longer runs the boot check, that runs it only in a comment, or that runs it
  # after the consumers exist.
  tr -d '\r' <"$main_rs" | sed '/contextualize::check_boot(&cfg)?/d' >"$work/main-no-check.rs"
  CR_CONFIG_MAIN="$work/main-no-check.rs" expect_fail "main does not run the boot check" "check_boot"
  tr -d '\r' <"$main_rs" | sed -E 's#^([[:space:]]*)provider::contextualize::check_boot#\1// provider::contextualize::check_boot#' >"$work/main-commented.rs"
  CR_CONFIG_MAIN="$work/main-commented.rs" expect_fail "main only mentions the boot check in a comment" "check_boot"
  tr -d '\r' <"$main_rs" | sed '/contextualize::check_boot(&cfg)?/d' | sed -E '/^[[:space:]]*let event_runtime/a\
    provider::contextualize::check_boot(\&cfg)?;' >"$work/main-late.rs"
  CR_CONFIG_MAIN="$work/main-late.rs" expect_fail "main runs the boot check after the consumers are built" "not before the event consumers"
  # No false positive: another feature that is off by default, in the same file.
  printf '// Video captions are off by default.\n//\n// Contextual retrieval is on.\n' >"$work/other.rs"
  if ! CR_CONFIG_CODE_FILES="$work/other.rs" bash "$script" >"$work/other.out" 2>&1; then
    # The other checks are fine with the real files; only the stale scan was narrowed.
    if grep -Fq "off by default" "$work/other.out"; then
      cat "$work/other.out" >&2
      echo "SELFTEST FAIL: another feature's off-by-default comment is a false positive" >&2
      failures=$((failures + 1))
    fi
  fi

  if [ "$failures" -ne 0 ]; then
    echo "contextual retrieval config contract self-test: $failures failure(s)" >&2
    exit 1
  fi
  echo "selftest: the contract passes on the real files and fails on each way of breaking it"
  exit 0
fi
[ "$#" -eq 0 ] || { echo "usage: $0 [--self-test]" >&2; exit 2; }

# ---------------------------------------------------------------- the contract

failures=0
fail() { printf 'FAIL: %s\n' "$*" >&2; failures=$((failures + 1)); }

[ -f "$compose" ] || { printf 'FAIL: %s is missing\n' "$compose" >&2; exit 1; }
[ -f "$env_example" ] || { printf 'FAIL: %s is missing\n' "$env_example" >&2; exit 1; }

# The text without CR (a Windows checkout) and without comment lines: a sentence that
# explains a variable must not stand in for the variable.
active() { tr -d '\r' <"$1" | sed -E '/^[[:space:]]*#/d'; }

# One service's block: from its key to the next service key (two-space indent). awk reads ALL of
# its input and never `exit`s early: under `set -o pipefail` an awk that quits while `sed` still has
# lines to write kills `sed` with SIGPIPE, the pipeline status is 141, and `set -e` ends the script
# without a word on Linux (git-bash has no SIGPIPE, so it only showed there).
service_block() {
  active "$compose" | awk -v name="$1" '
    !done && $0 ~ "^  " name ":[[:space:]]*$" { inside = 1; print; next }
    inside && /^  [A-Za-z0-9_-]+:[[:space:]]*$/ { inside = 0; done = 1 }
    inside { print }'
}

engine="$(service_block embedding-engine)"
[ -n "$engine" ] || { echo "FAIL: no embedding-engine service in $compose" >&2; exit 1; }

# 1. The documented compose defaults. KEY|DEFAULT, one per line; an empty default is a
#    variable that is passed through and has no value of its own.
expected_defaults='
CONTEXTUAL_RETRIEVAL_ENABLED|true
CONTEXT_RETRIEVAL_KILL_SWITCH_REASON|
CONTEXTUAL_RETRIEVAL_MODEL|gpt-4o-mini
CONTEXTUAL_RETRIEVAL_PROVIDER_HINT|azure-openai
CONTEXTUAL_RETRIEVAL_MIN_RESIDENCY|eu
CONTEXTUAL_RETRIEVAL_PROMPT_VERSION|ctx-v1
CONTEXTUAL_RETRIEVAL_WINDOW_CHARS|32000
CONTEXTUAL_RETRIEVAL_MAX_DOCUMENT_CHARS|48000
CONTEXTUAL_RETRIEVAL_MAX_TOKENS|128
CONTEXTUAL_RETRIEVAL_TIMEOUT_MS|30000
CONTEXTUAL_RETRIEVAL_CONCURRENCY|4
CONTEXTUAL_RETRIEVAL_RETRY_ATTEMPTS|4
CONTEXTUAL_RETRIEVAL_RETRY_BASE_MS|2000
CONTEXT_INLINE_BUDGET_MS|20000
CONTEXT_INLINE_DOCUMENT_BUDGET_MS|8000
CONTEXT_INLINE_DOCUMENT_CONCURRENCY|2
CONTEXT_RECONCILER_ENABLED|0
CONTEXT_RECONCILER_INTERVAL_SECS|30
CONTEXT_RECONCILER_UNITS_PER_PASS|32
CONTEXT_RECONCILER_ORG_UNITS_PER_PASS|16
CONTEXT_RECONCILER_CONCURRENCY|2
CONTEXT_RECONCILER_ORG_CONCURRENCY|1
CONTEXT_RECONCILER_CHUNK_CONCURRENCY|2
CONTEXT_RECONCILER_LEASE_SECS|300
CONTEXT_RECONCILER_BUDGET_MS|30000
CONTEXT_RECONCILER_DOCUMENT_BUDGET_MS|25000
CONTEXT_RECONCILER_MAX_ATTEMPTS|8
CONTEXT_RECONCILER_BREAKER_FAILURES|3
CONTEXT_RECONCILER_BREAKER_PAUSE_SECS|300
CONTEXT_RECONCILER_PAUSE_FILE|/tmp/context-reconciler.pause
CONTEXT_RECONCILER_ZDR_RECHECK_SECS|3600
CONTEXT_RECONCILER_SWEEP_SECS|300
CONTEXT_RECONCILER_STALE_ENABLED|0
CONTEXT_RECONCILER_STALE_DOCUMENTS|8
CONTEXT_REFRESH_ON_PROMPT_CHANGE|0
CONTEXT_REFRESH_MAX_UNITS|0
USAGE_EMIT_ENABLED|0
'
# The `KEY: value` lines of the block and the `KEY=value` lines of the example are read once and
# in-process (associative arrays: bash 4 or later), so the contract costs no process per variable.
declare -A engine_value example_value
while IFS= read -r line; do
  if [[ $line =~ ^[[:space:]]*([A-Z][A-Z0-9_]*):[[:space:]]*(.*)$ ]]; then
    name="${BASH_REMATCH[1]}"
    value="${BASH_REMATCH[2]}"
    engine_value["$name"]="${value%"${value##*[![:space:]]}"}"
  fi
done <<<"$engine"
while IFS= read -r line; do
  if [[ $line =~ ^([A-Z][A-Z0-9_]*)=(.*)$ ]]; then
    example_value["${BASH_REMATCH[1]}"]="${BASH_REMATCH[2]}"
  fi
done < <(active "$env_example")

while IFS='|' read -r key want; do
  [ -n "$key" ] || continue
  if [ -z "${engine_value[$key]+set}" ]; then
    fail "the embedding-engine block does not pass $key through: the code reads it, and a variable the container never receives cannot be set by an operator"
    continue
  fi
  expect="\${${key}:-${want}}"
  if [ "${engine_value[$key]}" != "$expect" ]; then
    fail "$key: the compose line is '$key: ${engine_value[$key]}', want '$key: $expect'"
  fi
done <<<"$expected_defaults"

# 2. Every contextual retrieval variable the block passes is documented in `.env.example`
#    with the default the compose file has. Discovered from the compose file, not listed,
#    so a variable added later cannot be forgotten.
compose_keys=()
for name in "${!engine_value[@]}"; do
  case "$name" in
    CONTEXTUAL_RETRIEVAL_* | CONTEXT_* | USAGE_EMIT_ENABLED) compose_keys+=("$name") ;;
  esac
done
[ "${#compose_keys[@]}" -gt 0 ] || fail "found no contextual retrieval variable in the embedding-engine block"
while IFS= read -r key; do
  [ -n "$key" ] || continue
  value="${engine_value[$key]}"
  if [[ $value =~ ^\$\{[A-Z0-9_]+:-(.*)\}$ ]]; then
    default="${BASH_REMATCH[1]}"
  else
    fail "$key has no default in the compose file (a variable of this feature must work with nothing set)"
    continue
  fi
  if [ -z "${example_value[$key]+set}" ]; then
    fail "$key is not documented in .env.example (an operator cannot know it exists)"
  elif [ "${example_value[$key]}" != "$default" ]; then
    fail "$key: .env.example says '${example_value[$key]}', the compose default is '$default'"
  fi
done < <(printf '%s\n' "${compose_keys[@]}" | sort)

# 3. The example never throws the kill switch.
switch="${example_value[CONTEXTUAL_RETRIEVAL_ENABLED]:-}"
switch="${switch,,}"
if [ "$switch" = "false" ] || [ "$switch" = "0" ]; then
  fail ".env.example turns contextual retrieval off: copied to .env it would switch off a feature that stays on"
fi

# 4. Every harness that brings the base compose up turns the feature off explicitly, with a
#    reason. The isolated stacks have no inference-core.
for harness in "${harnesses[@]}"; do
  if [ ! -f "$harness" ]; then
    fail "the harness $harness is missing"
    continue
  fi
  name="$(basename "$harness")"
  text="$(tr -d '\r' <"$harness" | sed -E '/^[[:space:]]*#/d')"
  # Here-strings, not `printf | grep -q`: grep -q quits at its first match, and a producer that is
  # still writing then dies of SIGPIPE, which `pipefail` reports as a failed pipeline.
  if ! grep -Eq '^[[:space:]]*export CONTEXTUAL_RETRIEVAL_ENABLED=("?)false\1[[:space:]]*$' <<<"$text"; then
    fail "$name does not export CONTEXTUAL_RETRIEVAL_ENABLED=false: its stack has no inference-core, and a feature that is on by default must be turned off there by name"
  fi
  if ! grep -Eq '^[[:space:]]*export CONTEXT_RETRIEVAL_KILL_SWITCH_REASON=("[^"]{8,}"|[^"[:space:]]{8,})[[:space:]]*$' <<<"$text"; then
    fail "$name does not export a CONTEXT_RETRIEVAL_KILL_SWITCH_REASON of eight characters or more: the kill switch is refused without one"
  fi
done

# 5. No stale statement in a comment or a document of the embedding engine, its compose
#    block or its example environment. Two rules, because the original defect hid from either
#    alone: (a) a PARAGRAPH (a run of comment lines with no empty comment line between them; a
#    line of code ends it) that is about contextual retrieval (it says so, or names the flag, or
#    the word contextualize) must not call it off by default; (b) the comment BLOCK directly above
#    the line that declares the switch (compose, the config field, the example) must not, whatever
#    its paragraphs are about: the original said "OFF by default" in a paragraph that never named
#    the feature, because the paragraph above it did.
stale_statements() {
  awk '
    BEGIN {
      OFF = "(off[ -]by[ -]default|default(s)?( is| to)?[ -]off|disabled by default|off,? which is the default)"
    }
    function flush(   p) {
      if (para != "") {
        p = tolower(para)
        if (p ~ /contextual[ _]retrieval|contextualiz/ && p ~ OFF) {
          printf "%s:%d: a paragraph about contextual retrieval says it is off by default\n", file, start
        }
      }
      para = ""
    }
    function end_block(code,   b) {
      if (block != "" && tolower(code) ~ /contextual_retrieval_enabled/) {
        b = tolower(block)
        if (b ~ OFF) {
          printf "%s:%d: the comment above the contextual retrieval switch says it is off by default\n", file, block_start
        }
      }
      block = ""
      block_start = 0
    }
    FNR == 1 { flush(); end_block(""); file = FILENAME }
    { sub(/\r$/, "") }
    /contextualized_text/ { printf "%s:%d: names the column contextualized_text, which does not exist (the column is chunk_context)\n", file, FNR }
    {
      line = $0
      if (line ~ /^[[:space:]]*#!?\[/) {
        # A Rust attribute starts with a hash too. It is code, and it does not end the comment
        # block above the field it belongs to.
        flush()
      } else if (line ~ /^[[:space:]]*(\/\/[\/!]?|#)/) {
        sub(/^[[:space:]]*(\/\/[\/!]?|#)/, "", line)
        gsub(/^[[:space:]]+|[[:space:]]+$/, "", line)
        if (block_start == 0) block_start = FNR
        if (line == "") { flush(); next }
        block = block " " line
        if (para == "") start = FNR
        para = para " " line
      } else {
        flush()
        end_block(line)
      }
    }
    END { flush(); end_block("") }' "$@"
}
existing_files=()
for file in "${code_files[@]}"; do
  if [ -f "$file" ]; then
    existing_files+=("$file")
  else
    fail "the file $file is missing"
  fi
done
if [ "${#existing_files[@]}" -gt 0 ]; then
  hits="$(stale_statements "${existing_files[@]}")"
  if [ -n "$hits" ]; then
    while IFS= read -r hit; do fail "$hit"; done <<<"$hits"
  fi
fi

# 6. `main` runs the boot check, on a line of code (not a comment), before the event consumers
#    are built (`let event_runtime`). awk reads the file itself, not a pipe, so it may stop early.
if [ ! -f "$main_rs" ]; then
  fail "the file $main_rs is missing"
else
  check_line="$(awk '/provider::contextualize::check_boot\(&cfg\)\?/ && $0 !~ /^[[:space:]]*\/\// { print FNR; exit }' "$main_rs")"
  consumers_line="$(awk '/^[[:space:]]*let event_runtime/ { print FNR; exit }' "$main_rs")"
  if [ -z "$check_line" ]; then
    fail "main.rs does not call provider::contextualize::check_boot(&cfg)?: with the feature on and no credential the service would start, and its consumer would end without a word"
  elif [ -z "$consumers_line" ]; then
    fail "main.rs has no 'let event_runtime' line: this check needs updating to say where the consumers are built"
  elif [ "$check_line" -ge "$consumers_line" ]; then
    fail "main.rs calls check_boot at line $check_line, not before the event consumers are built (line $consumers_line)"
  fi
fi

if [ "$failures" -ne 0 ]; then
  printf 'contextual retrieval config contract: %d check(s) failed\n' "$failures" >&2
  exit 1
fi
echo "PASS: contextual retrieval is on by default, documented, and off in the isolated stacks by name"
