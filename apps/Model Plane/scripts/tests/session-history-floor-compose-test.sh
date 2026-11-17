#!/usr/bin/env bash
# WS04-S11 compose contract for SESSION_CORE_SPACE_HISTORY_FLOOR (the since_join history floor).
#
# session-core reads this switch with the strict literal reader (mp_contracts loop_fold::switch_from,
# used by space::audience_window): only the exact value `true` turns the floor on. An old `1`, `yes`
# or `on` in a deployment's env file leaves the floor OFF, so late joiners see pre-join history. The
# compose declarations therefore use the strict vocabulary only:
#   * docker-compose.yml declares the switch once, defaulting to the literal `false` (full history);
#   * docker-compose.override.yml (dev only) declares it once as the literal "true";
#   * any other deploy compose file that declares it must use the same `false` default.
# This script reads compose files only: it reads no .env file and runs no docker command.
#
#   bash "apps/Model Plane/scripts/tests/session-history-floor-compose-test.sh"               check the real files
#   bash "apps/Model Plane/scripts/tests/session-history-floor-compose-test.sh" --self-test   prove the check can fail
set -euo pipefail

SWITCH="SESSION_CORE_SPACE_HISTORY_FLOOR"
WANT_OFF="SESSION_CORE_SPACE_HISTORY_FLOOR: \${SESSION_CORE_SPACE_HISTORY_FLOOR:-false}"
WANT_DEV="SESSION_CORE_SPACE_HISTORY_FLOOR: \"true\""

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
DEPLOY="$SCRIPT_DIR/../../deploy"

fail() {
  echo "session history floor compose contract: $*" >&2
  exit 1
}

# The switch's declarations in one compose file, one per line and trimmed. Comment lines, trailing
# comments and carriage returns are dropped. Prints nothing when the file does not declare it.
declarations() {
  tr -d '\r' < "$1" \
    | grep -v -E '^[[:space:]]*#' \
    | grep -E "^[[:space:]]*(- *)?${SWITCH}[:=]" \
    | sed -e 's/[[:space:]]#.*$//' -e 's/^[[:space:]]*//' -e 's/[[:space:]]*$//' \
    || true
}

# Passes when the file declares the switch exactly once, as $2.
declares_exactly() {
  [ "$(declarations "$1")" = "$2" ]
}

# Passes when the file declares nothing, or declares the switch only as the off default.
declares_at_most_off() {
  local got
  got="$(declarations "$1")"
  [ -z "$got" ] || [ "$got" = "$WANT_OFF" ]
}

# Checks the pins over the compose files in $1. Prints the first drift and returns 1.
check_deploy() {
  local deploy="$1" file name
  for file in "$deploy/docker-compose.yml" "$deploy/docker-compose.override.yml"; do
    [ -f "$file" ] || { echo "$file not found" >&2; return 1; }
  done
  if ! declares_exactly "$deploy/docker-compose.yml" "$WANT_OFF"; then
    echo "docker-compose.yml must declare the switch exactly as: $WANT_OFF" >&2
    echo "  found: $(declarations "$deploy/docker-compose.yml" | tr '\n' ' ')" >&2
    return 1
  fi
  if ! declares_exactly "$deploy/docker-compose.override.yml" "$WANT_DEV"; then
    echo "docker-compose.override.yml must declare the switch exactly as: $WANT_DEV" >&2
    echo "  found: $(declarations "$deploy/docker-compose.override.yml" | tr '\n' ' ')" >&2
    return 1
  fi
  for file in "$deploy"/*.yml "$deploy"/*.yaml; do
    [ -f "$file" ] || continue
    name="$(basename "$file")"
    case "$name" in
      docker-compose.yml | docker-compose.override.yml) continue ;;
    esac
    if ! declares_at_most_off "$file"; then
      echo "$name may declare the switch only as: $WANT_OFF" >&2
      echo "  found: $(declarations "$file" | tr '\n' ' ')" >&2
      return 1
    fi
  done
  return 0
}

# Writes a fixture deploy directory: $1 the directory, $2 the base file's declaration lines, $3 the
# override's, $4 a production file's (none is written when empty).
make_fixture() {
  mkdir -p "$1"
  printf 'services:\n  session-core:\n    environment:\n%s\n' "$2" > "$1/docker-compose.yml"
  printf 'services:\n  session-core:\n    environment:\n%s\n' "$3" > "$1/docker-compose.override.yml"
  if [ -n "$4" ]; then
    printf 'services:\n  session-core:\n    environment:\n%s\n' "$4" > "$1/docker-compose.production.yml"
  fi
}

# expect_case pass|fail NAME BASE OVERRIDE PRODUCTION: the check must give the stated verdict.
expect_case() {
  local want="$1" name="$2" dir="$WORK/$2"
  make_fixture "$dir" "$3" "$4" "$5"
  if check_deploy "$dir" 2>/dev/null; then
    [ "$want" = pass ] || fail "self-test: $name passed but must fail"
  else
    [ "$want" = fail ] || fail "self-test: $name failed but must pass"
  fi
}

self_test() {
  local off="      $WANT_OFF" dev="      $WANT_DEV"
  WORK="$(mktemp -d)"
  trap 'rm -rf "$WORK"' EXIT

  expect_case pass pinned "$off" "$dev" ""
  expect_case pass commented_drift_ignored_production_off "$off"$'\n'"      # SESSION_CORE_SPACE_HISTORY_FLOOR: \${SESSION_CORE_SPACE_HISTORY_FLOOR:-1}" "$dev" "$off"
  expect_case fail base_default_one "      SESSION_CORE_SPACE_HISTORY_FLOOR: \${SESSION_CORE_SPACE_HISTORY_FLOOR:-1}" "$dev" ""
  expect_case fail base_default_yes "      SESSION_CORE_SPACE_HISTORY_FLOOR: \${SESSION_CORE_SPACE_HISTORY_FLOOR:-yes}" "$dev" ""
  expect_case fail base_default_on "      SESSION_CORE_SPACE_HISTORY_FLOOR: \${SESSION_CORE_SPACE_HISTORY_FLOOR:-on}" "$dev" ""
  expect_case fail base_default_zero "      SESSION_CORE_SPACE_HISTORY_FLOOR: \${SESSION_CORE_SPACE_HISTORY_FLOOR:-0}" "$dev" ""
  expect_case fail base_default_true "      SESSION_CORE_SPACE_HISTORY_FLOOR: \${SESSION_CORE_SPACE_HISTORY_FLOOR:-true}" "$dev" ""
  expect_case fail base_undeclared "      OTHER_SWITCH: \"false\"" "$dev" ""
  expect_case fail base_declared_twice "$off"$'\n'"$off" "$dev" ""
  expect_case fail override_quoted_one "$off" "      SESSION_CORE_SPACE_HISTORY_FLOOR: \"1\"" ""
  expect_case fail override_bare_one "$off" "      SESSION_CORE_SPACE_HISTORY_FLOOR: 1" ""
  expect_case fail override_undeclared "$off" "      OTHER_SWITCH: \"true\"" ""
  expect_case fail production_on "$off" "$dev" "      SESSION_CORE_SPACE_HISTORY_FLOOR: \"true\""
  expect_case fail production_one "$off" "$dev" "      SESSION_CORE_SPACE_HISTORY_FLOOR: \${SESSION_CORE_SPACE_HISTORY_FLOOR:-1}"
  echo "session history floor compose self-test: ok"
}

if [ "${1:-}" = "--self-test" ]; then
  self_test
  exit 0
fi

check_deploy "$DEPLOY" || fail "the compose declarations of $SWITCH drifted from the strict literal pins"
echo "ok: docker-compose.yml defaults $SWITCH to the literal false"
echo "ok: docker-compose.override.yml sets $SWITCH to the literal \"true\""
echo "ok: no other deploy compose file declares $SWITCH except as the false default"
echo "SESSION_CORE_SPACE_HISTORY_FLOOR compose contract passed"
