#!/usr/bin/env bash
# =============================================================================
# contextual-config-live-proof.sh  (design task CR-T13)
#
# Live proof that contextual retrieval is ON by default and that the incident kill switch is a
# deliberate, recorded act. Two steps. NEITHER makes a provider call, a model call or a paid call
# of any kind, and neither writes anything: the first only reads the running embedding-engine
# container, the second runs the image with NO network at all (`--network none`) and unreachable
# dummy endpoints, so it can reach no database, no broker and no provider.
#
# Run it after the embedding-engine image and the compose file of CR-T13 are deployed through
# the launcher stage copies (never a hand-built `docker compose`), and after the live proof of
# CR-T8 and CR-T9a is green (rollout step 6 of the runbook: the default flips last).
#
# Nothing here prints a secret. The environment of a container is never dumped: each variable is
# read by name, and only the non-secret settings of this feature are on the list. Do not add `set -x`.
#
#   CR_LIVE_STEP=defaults bash tests/scripts/contextual-config-live-proof.sh
#       Read-only, on the RUNNING container (EMBEDDING_ENGINE_CONTAINER, default
#       data-plane-v2-embedding-engine-1). It proves the feature is on because the DEFAULTS say
#       so, not because a file of the operator's does: recreate the container first with the two
#       variables the live `.env` carries (CONTEXTUAL_RETRIEVAL_ENABLED, CONTEXTUAL_RETRIEVAL_MODEL)
#       either absent from a COPY of the environment file or set to the empty string in the
#       launcher's environment (an empty value takes the compose default, and the process
#       environment wins over `.env`). The live `.env` is never edited in place.
#         D1  every variable of the list has the documented value inside the container: the switch
#             `true`, the model `gpt-4o-mini`, the provider `azure-openai`, the floor `eu`, the
#             prompt `ctx-v1`, the kill-switch reason empty, the reconciler `0`.
#         D2  the startup log of THIS container run says `contextual retrieval is ON` with the
#             model, the provider, the floor and the prompt version, and says nothing of a
#             kill switch; the consumer logged `contextual retrieval ENABLED`.
#         D3  /readyz answers 200 and reports `contextual_retrieval` as `{"state":"on",
#             "model":"gpt-4o-mini"}` and `checks.contextual_retrieval` as `on`.
#         D4  the environment file of the stack (EXPECT_ENV_FILE_WITHOUT_CR_KEYS=1 to require it)
#             carries neither CONTEXTUAL_RETRIEVAL_ENABLED nor CONTEXTUAL_RETRIEVAL_MODEL, which is
#             what makes D1 to D3 a proof of the defaults. Informational unless required.
#
#   CR_LIVE_STEP=boot-contract IMAGE=data-plane-v2-embedding-engine:<tag> \
#       bash tests/scripts/contextual-config-live-proof.sh
#       The image (or CR_BOOT_BIN=<path to an embedding-engine binary>, for a check before there
#       is an image) is started seven times with no network and dummy endpoints. It always ends
#       with a non-zero status (it cannot reach a database); what is proved is HOW:
#         B1  nothing set: the log says `contextual retrieval is ON` (model, provider, floor,
#             prompt), and the run proceeds to the database, where it fails (the contract passed).
#         B2  the switch off, no reason: it stops BEFORE the database, names
#             CONTEXT_RETRIEVAL_KILL_SWITCH_REASON and CONTEXTUAL_RETRIEVAL_ENABLED, exit status 1.
#         B3  the switch off with a reason: the log says `contextual retrieval is OFF` and carries
#             the reason, then proceeds to the database (the contract passed).
#         B4  a reason that is too short is refused before the database.
#         B5  a value that is not a boolean (`flase`) is refused before the database and named.
#         B6  an EMPTY switch is the default, on: the log says ON, never OFF.
#         B7  a blank model while on is refused before the database and named.
#
#   The kill switch on a RUNNING service (its `/readyz` report) is proved in a disposable stack,
#   never on the live one: the isolated harnesses run with the switch off and a reason, so in a
#   stack brought up with `tests/e2e/run-isolated-mvp.sh`'s environment,
#     docker compose -p <project> exec embedding-engine curl -s http://127.0.0.1:9202/readyz
#   must answer 200 with "contextual_retrieval":{"reason":"isolated test stack: ...","state":"kill_switch"}
#   and "checks":{"contextual_retrieval":"kill_switch"}. The same report is proved against the
#   real handler by the unit tests of src/api/mod.rs.
#
# Exit status: 0 every check of the step passed, 1 one did not, 2 the step is unknown or its
# prerequisites are missing.
# =============================================================================
set -euo pipefail

data_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
step="${CR_LIVE_STEP:-}"
container="${EMBEDDING_ENGINE_CONTAINER:-data-plane-v2-embedding-engine-1}"
env_file="${ENV_FILE:-$data_root/.env}"

failures=0
fail() { printf 'FAIL: %s\n' "$*" >&2; failures=$((failures + 1)); }
pass() { printf 'ok:   %s\n' "$*"; }
need() { command -v "$1" >/dev/null 2>&1 || { printf 'missing required command: %s\n' "$1" >&2; exit 2; }; }

# The non-secret variables of this feature that are read by name, one at a time.
defaults_list='
CONTEXTUAL_RETRIEVAL_ENABLED|true
CONTEXTUAL_RETRIEVAL_MODEL|gpt-4o-mini
CONTEXTUAL_RETRIEVAL_PROVIDER_HINT|azure-openai
CONTEXTUAL_RETRIEVAL_MIN_RESIDENCY|eu
CONTEXTUAL_RETRIEVAL_PROMPT_VERSION|ctx-v1
CONTEXT_RETRIEVAL_KILL_SWITCH_REASON|
CONTEXT_RECONCILER_ENABLED|0
CONTEXT_REFRESH_ON_PROMPT_CHANGE|0
'

step_defaults() {
  need docker
  need node
  docker inspect "$container" >/dev/null 2>&1 || { printf 'no container named %s\n' "$container" >&2; exit 2; }

  # D1: by name, one variable at a time.
  while IFS='|' read -r name want; do
    [ -n "$name" ] || continue
    got="$(docker exec "$container" printenv "$name" 2>/dev/null || true)"
    if [ "$got" = "$want" ]; then
      pass "D1 $name=${got:-<empty>}"
    else
      fail "D1 $name is '${got:-<empty>}' in the container, want '${want:-<empty>}'"
    fi
  done <<<"$defaults_list"

  # D2: the log of this container run (the container is recreated, so it is the run's own).
  logs="$(docker logs "$container" 2>&1 | sed 's/\x1b\[[0-9;]*m//g' || true)"
  line="$(printf '%s\n' "$logs" | grep -F 'contextual retrieval is ON' | tail -n 1 || true)"
  if [ -z "$line" ]; then
    fail "D2 the startup log has no 'contextual retrieval is ON' line (is the CR-T13 image running?)"
  else
    for needle in '"model":"gpt-4o-mini"' '"provider_hint":"azure-openai"' '"min_residency":"eu"' '"prompt_version":"ctx-v1"'; do
      printf '%s' "$line" | grep -Fq "$needle" || fail "D2 the ON line does not carry $needle"
    done
    pass "D2 the log says contextual retrieval is ON, with its model, provider, floor and prompt"
  fi
  # Here-strings here and below, not `printf | grep -q`: a container log can be megabytes, grep -q
  # quits at its first match, and the producer then dies of SIGPIPE, which `pipefail` reports as a
  # failed pipeline (a true match read as false).
  if grep -Fq 'contextual retrieval is OFF' <<<"$logs"; then
    fail "D2 the log of this container run says the kill switch is thrown"
  fi
  if grep -Fq 'contextual retrieval ENABLED' <<<"$logs"; then
    pass "D2 the consumer logged 'contextual retrieval ENABLED'"
  else
    fail "D2 the consumer did not log 'contextual retrieval ENABLED' (signed event consumers off, or the consumer did not start)"
  fi

  # D3: /readyz, parsed.
  body="$(docker exec "$container" curl -s -o - -w '\n%{http_code}' http://127.0.0.1:9202/readyz 2>/dev/null || true)"
  code="${body##*$'\n'}"
  json="${body%$'\n'*}"
  if [ "$code" != "200" ]; then
    fail "D3 /readyz answered '$code', want 200"
  else
    if printf '%s' "$json" | node -e '
      let s = ""; process.stdin.on("data", (d) => (s += d)).on("end", () => {
        const j = JSON.parse(s); const c = j.contextual_retrieval || {};
        const ok = c.state === "on" && c.model === "gpt-4o-mini" && j.checks && j.checks.contextual_retrieval === "on";
        process.exit(ok ? 0 : 1);
      });'; then
      pass "D3 /readyz answers 200 and reports contextual_retrieval on, model gpt-4o-mini"
    else
      fail "D3 /readyz does not report contextual_retrieval as on with the model (body: $json)"
    fi
  fi

  # D4: the environment file carries neither key.
  if [ -f "$env_file" ]; then
    present="$(grep -Ec '^(CONTEXTUAL_RETRIEVAL_ENABLED|CONTEXTUAL_RETRIEVAL_MODEL)=' "$env_file" || true)"
    if [ "$present" = "0" ]; then
      pass "D4 $(basename "$env_file") carries neither CONTEXTUAL_RETRIEVAL_ENABLED nor CONTEXTUAL_RETRIEVAL_MODEL: D1 to D3 are the defaults"
    elif [ "${EXPECT_ENV_FILE_WITHOUT_CR_KEYS:-0}" = "1" ]; then
      fail "D4 $(basename "$env_file") still carries $present of CONTEXTUAL_RETRIEVAL_ENABLED and CONTEXTUAL_RETRIEVAL_MODEL"
    else
      printf 'note: D4 %s carries %s of those keys; D1 to D3 are the defaults only if the launcher environment emptied them\n' "$(basename "$env_file")" "$present"
    fi
  elif [ "${EXPECT_ENV_FILE_WITHOUT_CR_KEYS:-0}" = "1" ]; then
    fail "D4 there is no environment file at $env_file to inspect"
  fi
}

# Runs the engine once with no network. Sets OUT (combined output, colour removed) and RC.
OUT=""
RC=0
run_engine() {
  RC=0
  local dummy=(
    DATABASE_URL=postgres://nobody:nothing@db.invalid:5432/nothing
    NATS_URL=nats://nats.invalid:4222
    QDRANT_URL=http://qdrant.invalid:6334
    RUST_LOG=embedding_engine=info
  )
  if [ -n "${CR_BOOT_BIN:-}" ]; then
    OUT="$(env "${dummy[@]}" "$@" timeout 30 "$CR_BOOT_BIN" 2>&1)" || RC=$?
  else
    local args=() assignment
    for assignment in "${dummy[@]}" "$@"; do args+=(-e "$assignment"); done
    OUT="$(docker run --rm --network none "${args[@]}" "$IMAGE" 2>&1)" || RC=$?
  fi
  OUT="$(printf '%s' "$OUT" | sed 's/\x1b\[[0-9;]*m//g')"
}

reached_database() { printf '%s' "$OUT" | grep -Fq 'error communicating with database'; }

expect_refused_before_the_database() { # LABEL NEEDLE...
  local label="$1"
  shift
  if [ "$RC" -ne 1 ]; then fail "$label: exit status $RC, want 1"; fi
  if reached_database; then fail "$label: it reached the database, so the boot contract did not stop it"; fi
  local needle
  for needle in "$@"; do
    printf '%s' "$OUT" | grep -Fq "$needle" || fail "$label: the message does not say '$needle' (got: $(printf '%s' "$OUT" | head -c 300))"
  done
}

step_boot_contract() {
  if [ -n "${CR_BOOT_BIN:-}" ]; then
    [ -x "$CR_BOOT_BIN" ] || { printf 'CR_BOOT_BIN %s is not executable\n' "$CR_BOOT_BIN" >&2; exit 2; }
    need timeout
  else
    need docker
    : "${IMAGE:?set IMAGE=<embedding-engine image> or CR_BOOT_BIN=<binary>}"
  fi
  local reason='provider incident 2026-10-07: restricted text seen'

  run_engine
  if reached_database \
    && printf '%s' "$OUT" | grep -Fq 'contextual retrieval is ON' \
    && printf '%s' "$OUT" | grep -Fq '"model":"gpt-4o-mini"' \
    && printf '%s' "$OUT" | grep -Fq '"provider_hint":"azure-openai"' \
    && printf '%s' "$OUT" | grep -Fq '"min_residency":"eu"' \
    && printf '%s' "$OUT" | grep -Fq '"prompt_version":"ctx-v1"'; then
    pass "B1 nothing set: ON with gpt-4o-mini, azure-openai, eu, ctx-v1, and on to the database"
  else
    fail "B1 nothing set did not log ON with the defaults and proceed to the database (got: $(printf '%s' "$OUT" | head -c 400))"
  fi

  before="$failures"
  run_engine CONTEXTUAL_RETRIEVAL_ENABLED=false
  expect_refused_before_the_database "B2 off without a reason" CONTEXT_RETRIEVAL_KILL_SWITCH_REASON CONTEXTUAL_RETRIEVAL_ENABLED
  if [ "$failures" -eq "$before" ]; then pass "B2 the switch off without a reason is refused before the database, naming both variables"; fi

  run_engine CONTEXTUAL_RETRIEVAL_ENABLED=false "CONTEXT_RETRIEVAL_KILL_SWITCH_REASON=$reason"
  if reached_database \
    && printf '%s' "$OUT" | grep -Fq 'contextual retrieval is OFF' \
    && printf '%s' "$OUT" | grep -Fq "$reason"; then
    pass "B3 the switch off with a reason: logged OFF with the reason, and on to the database"
  else
    fail "B3 the switch off with a reason did not log OFF with the reason and proceed (got: $(printf '%s' "$OUT" | head -c 400))"
  fi

  before="$failures"
  run_engine CONTEXTUAL_RETRIEVAL_ENABLED=false CONTEXT_RETRIEVAL_KILL_SWITCH_REASON=todo
  expect_refused_before_the_database "B4 a reason that is too short" CONTEXT_RETRIEVAL_KILL_SWITCH_REASON
  if [ "$failures" -eq "$before" ]; then pass "B4 a reason that is too short is refused before the database"; fi

  before="$failures"
  run_engine CONTEXTUAL_RETRIEVAL_ENABLED=flase
  expect_refused_before_the_database "B5 a value that is not a boolean" CONTEXTUAL_RETRIEVAL_ENABLED flase
  if [ "$failures" -eq "$before" ]; then pass "B5 a typo is refused before the database and the variable is named"; fi

  run_engine CONTEXTUAL_RETRIEVAL_ENABLED=
  if reached_database \
    && printf '%s' "$OUT" | grep -Fq 'contextual retrieval is ON' \
    && ! printf '%s' "$OUT" | grep -Fq 'contextual retrieval is OFF'; then
    pass "B6 an empty switch is the default, on"
  else
    fail "B6 an empty switch was not read as on (got: $(printf '%s' "$OUT" | head -c 400))"
  fi

  before="$failures"
  run_engine CONTEXTUAL_RETRIEVAL_MODEL=
  expect_refused_before_the_database "B7 a blank model while on" CONTEXTUAL_RETRIEVAL_MODEL
  if [ "$failures" -eq "$before" ]; then pass "B7 a blank model while on is refused before the database"; fi
}

case "$step" in
  defaults) step_defaults ;;
  boot-contract) step_boot_contract ;;
  *)
    printf 'set CR_LIVE_STEP to defaults or boot-contract (see the header of this script)\n' >&2
    exit 2
    ;;
esac

if [ "$failures" -ne 0 ]; then
  printf 'contextual config live proof (%s): %d check(s) failed\n' "$step" "$failures" >&2
  exit 1
fi
echo "PASS: contextual config live proof ($step)"
