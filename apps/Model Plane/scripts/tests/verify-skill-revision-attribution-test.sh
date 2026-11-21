#!/usr/bin/env bash
set -euo pipefail

# Contract test for scripts/verify-skill-revision-attribution.sh with a stub docker. The read is one
# SELECT in a read-only transaction that binds --since as a psql variable and names no content column.
# Zero rows and attributed rows pass (exit 0). An unattributed row fails (exit 1) and prints only its
# count and its earliest timestamp. A usage error, an unreadable database or a row the check does not
# understand exits 2, and is never a pass. It never talks to a real daemon.

SRC="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/verify-skill-revision-attribution.sh"
TMP="$(mktemp -d "${TMPDIR:-/tmp}/verify-skill-revision-attribution-test.XXXXXX")"
trap 'rm -rf "$TMP"' EXIT

failed=0
fail() { echo "FAIL: $*" >&2; failed=1; }

mkdir -p "$TMP/bin"
CALLS="$TMP/calls.log"
cat >"$TMP/bin/docker" <<'STUB'
#!/usr/bin/env bash
args="$(echo "$*" | tr '\n' ' ')"
case "$args" in
  "exec -i "*)
    # The statement arrives on stdin; it is logged with the arguments so the tests can read it.
    echo "$args stdin: $(tr '\n' ' ')" >>"$CALLS"
    if [[ -n "${STUB_DB_ERROR:-}" ]]; then
      echo 'ERROR:  relation "skill_revisions" does not exist' >&2
      exit 3
    fi
    printf '%b' "${STUB_ROWS:-}"
    ;;
  *) echo "$args" >>"$CALLS" ;;
esac
STUB
chmod +x "$TMP/bin/docker"
export CALLS PATH="$TMP/bin:$PATH"

SINCE="2026-11-19T00:00:00Z"
run() { : >"$CALLS"; bash "$SRC" "$@" >"$TMP/out" 2>&1; }

# expect_exit <status> <arguments...>: runs the check and compares its exit status.
expect_exit() {
  local want="$1" got=0
  shift
  run "$@" || got=$?
  [[ "$got" == "$want" ]] || { cat "$TMP/out" >&2; fail "exit $got, expected $want, for: $*"; }
}

# Usage and input refusals: exit 2 before any docker call.
expect_exit 2
[[ ! -s "$CALLS" ]] || fail "a missing --since must not call docker"
expect_exit 2 --since
expect_exit 2 --since "2026-11-19; drop table skill_revisions"
expect_exit 2 --bogus
expect_exit 2 --since "$SINCE" extra
[[ ! -s "$CALLS" ]] || fail "a refused argument must not call docker"

# Zero rows pass.
STUB_ROWS="" expect_exit 0 --since "$SINCE"
grep -q "verdict: PASS, unattributed=0" "$TMP/out" || fail "zero rows must pass"

# Attributed rows only pass, with one line per actor_kind and no timestamp on those lines.
STUB_ROWS='service|2|2026-11-19 10:00:00+00\nuser|4|2026-11-19 09:00:00.5+00\n' expect_exit 0 --since "$SINCE"
grep -qx "actor_kind=user rows=4" "$TMP/out" || fail "each actor_kind must be counted on its own line"
grep -qx "actor_kind=service rows=2" "$TMP/out" || fail "service revisions must be counted"
grep -q "verdict: PASS" "$TMP/out" || fail "attributed rows must pass"

# An unattributed row fails, and the verdict carries the count and the earliest timestamp.
STUB_ROWS='user|4|2026-11-19 09:00:00+00\nunattributed|2|2026-11-20 08:15:30.123+00\n' expect_exit 1 --since "$SINCE"
grep -q "verdict: FAIL, unattributed=2 earliest_created_at=2026-11-20 08:15:30.123+00" "$TMP/out" ||
  fail "an unattributed row must fail with its count and its earliest timestamp"
grep -qx "actor_kind=unattributed rows=2" "$TMP/out" || fail "the unattributed count must be printed by actor_kind"

# A database that cannot be read is never a pass.
STUB_DB_ERROR=1 expect_exit 2 --since "$SINCE"
grep -q "no verdict" "$TMP/out" || fail "an unreadable database must say that there is no verdict"
grep -q "verdict: PASS" "$TMP/out" && fail "an unreadable database must never print a pass"

# A row the check does not understand is never a pass.
STUB_ROWS='mystery|1|2026-11-20 00:00:00+00\n' expect_exit 2 --since "$SINCE"
grep -q "verdict: PASS" "$TMP/out" && fail "an unknown actor_kind must never print a pass"

# The read is read-only, binds the date as a psql variable and names no content column.
STUB_ROWS="" expect_exit 0 --since "$SINCE"
grep -qF "PGOPTIONS=-c default_transaction_read_only=on" "$CALLS" || fail "the read must run in a read-only transaction"
grep -qF -- "-v since=$SINCE" "$CALLS" || fail "the --since value must reach psql as the variable since"
grep -qF "select actor_kind, count(*), min(created_at) from skill_revisions where created_at >= :'since' group by 1 order by 1" "$CALLS" ||
  fail "the statement must bind :'since' and group by actor_kind"
sql_line="$(grep -F "stdin:" "$CALLS" | tail -n 1)"
for word in content name owner org_id skill_id before after reason actor_id run_id thread_id; do
  grep -qw -- "$word" <<<"$sql_line" && fail "the statement must not name $word"
done
grep -qiE '\b(insert|update|delete|drop|alter|truncate|create|grant|revoke)\b' <<<"$sql_line" &&
  fail "the statement must not write"

[[ "$failed" == "0" ]] || exit 1
echo "verify-skill-revision-attribution test: ok"
