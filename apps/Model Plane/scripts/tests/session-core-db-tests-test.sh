#!/usr/bin/env bash
set -euo pipefail

# Contract test for the disposable-Postgres harness. It starts no Docker and no
# cargo. It checks the text the harness must keep, and it runs the harness's
# --check-log mode on fixture logs: that mode decides whether a cargo run proves
# anything. A stand-in docker function fails the test if any check reaches Docker.

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
HARNESS="$ROOT_DIR/scripts/tests/session-core-db-tests.sh"
WORK="$(mktemp -d)"
export WORK
trap 'rm -rf "$WORK"' EXIT

fail() {
  echo "session-core DB test harness contract: $1" >&2
  exit 1
}

docker() {
  printf 'docker was called\n' >>"$WORK/docker-called"
  return 99
}
export -f docker

[[ -f "$HARNESS" ]] || fail "the harness is missing at $HARNESS"
if grep -q $'\r' "$HARNESS"; then
  fail 'the harness must use LF line endings'
fi

source_text="$(cat "$HARNESS")"
for required in \
  'postgres:16.14-alpine3.24' \
  '-p 127.0.0.1::5432' \
  'docker rm -f -v' \
  'trap cleanup EXIT' \
  'pg_isready -h 127.0.0.1' \
  'session_core_test' \
  'unset DATABASE_URL' \
  '--include-ignored' \
  '--test-threads=1' \
  'provisioned_broker' \
  '--show-output' \
  'skipping: DATABASE_URL unset' \
  'panicked' \
  'MIN_PASSED' \
  'test result: ok' \
  '--check-log'; do
  grep -F -- "$required" <<<"$source_text" >/dev/null || fail "the harness is missing required text: $required"
done

for forbidden in \
  'model-plane-postgres' \
  'docker compose' \
  'docker build' \
  'system prune' \
  'volume prune' \
  '--volumes' \
  'set -x' \
  'local-proof-only' \
  ' -P '; do
  if grep -F -- "$forbidden" <<<"$source_text" >/dev/null; then
    fail "the harness must not contain: $forbidden"
  fi
done

# Fixture logs. Passing tests print nothing unless --show-output is set, so a
# skip message reaches the log only through that flag.
write_log() {
  local name="$1"
  shift
  printf '%s\n' "$@" >"$WORK/$name.log"
}

write_log proven \
  'running 4 tests' \
  'test loop_checkpoint::tests::save_is_compare_and_set ... ok' \
  'test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.20s'
write_log short \
  'test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.90s'
write_log two-binaries \
  'test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.40s' \
  'test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.60s'
write_log none-ran \
  'running 0 tests' \
  'test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s'
write_log skip-seen \
  'skipping: DATABASE_URL unset' \
  'test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.00s'
write_log panic-seen \
  "thread 'tokio-runtime-worker' panicked at src/lib.rs:1:1:" \
  'boom' \
  'test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.00s'
write_log failed-run \
  'test result: FAILED. 1 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.00s'

expect_status() {
  local want="$1" name="$2" minimum="$3" got=0
  bash "$HARNESS" --check-log "$WORK/$name.log" "$minimum" >"$WORK/$name.out" 2>&1 || got=$?
  [[ "$got" == "$want" ]] || fail "--check-log $name with minimum $minimum: want exit $want, got $got"
}

expect_status 0 proven 4
expect_status 4 short 4
expect_status 0 two-binaries 5
expect_status 4 none-ran 1
expect_status 4 skip-seen 1
expect_status 4 panic-seen 1
expect_status 4 failed-run 1
expect_status 2 missing 1

bad_status=0
bash "$HARNESS" 'loop checkpoint' >"$WORK/bad-filter.out" 2>&1 || bad_status=$?
[[ "$bad_status" == 2 ]] || fail "a filter with a space must exit 2, got $bad_status"
grep -q 'invalid filter' "$WORK/bad-filter.out" || fail 'a bad filter must be named as invalid'

zero_status=0
bash "$HARNESS" 'loop_checkpoint=0' >"$WORK/zero-min.out" 2>&1 || zero_status=$?
[[ "$zero_status" == 2 ]] || fail "a MIN_PASSED of 0 must exit 2, got $zero_status"

[[ ! -e "$WORK/docker-called" ]] || fail 'the argument and log checks reached docker'

printf 'session-core DB test harness contract: ok\n'
