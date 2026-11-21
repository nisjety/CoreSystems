#!/usr/bin/env bash
set -euo pipefail

# Contract test for scripts/report-user-memory-space-origin.sh and its SQL (depth register D3-266).
# It never talks to a database: a stub psql on PATH records how it is called and what it is given.
# The checks: the SQL is one read-only transaction that selects only org_id and counts, its user-scope
# predicates are the ones session-core's room-origin filter uses, and the wrapper takes the URL only from
# SESSION_CORE_DATABASE_URL, never prints it, fails closed when it is unset, and returns psql's own status.

SCRIPTS_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SQL="$SCRIPTS_DIR/report-user-memory-space-origin.sql"
SH="$SCRIPTS_DIR/report-user-memory-space-origin.sh"
ORIGIN_RS="$SCRIPTS_DIR/../rust/services/session-core/src/space/memory_scope.rs"

TMP="$(mktemp -d "${TMPDIR:-/tmp}/report-user-memory-space-origin-test.XXXXXX")"
trap 'rm -rf "$TMP"' EXIT

fail() {
  echo "report-user-memory-space-origin test: FAIL: $*" >&2
  exit 1
}

trim() { sed 's/^[[:space:]]*//;s/[[:space:]]*$//'; }

[[ -r "$SQL" ]] || fail "missing $SQL"
[[ -r "$SH" ]] || fail "missing $SH"
[[ -x "$SH" ]] || fail "$SH is not executable"
[[ -r "$ORIGIN_RS" ]] || fail "missing $ORIGIN_RS"
bash -n "$SH" || fail "wrapper has a syntax error"

# 1. The SQL, comments removed.
sed 's/--.*$//' "$SQL" | tr -d '\r' >"$TMP/code.sql"
first="$(grep -v '^[[:space:]]*$' "$TMP/code.sql" | head -1 | trim)"
last="$(grep -v '^[[:space:]]*$' "$TMP/code.sql" | tail -1 | trim)"
[[ "$first" == "BEGIN READ ONLY;" ]] || fail "the first statement must be BEGIN READ ONLY; (got: $first)"
[[ "$last" == "ROLLBACK;" ]] || fail "the last statement must be ROLLBACK; (got: $last)"

if grep -iEq '\b(insert|update|delete|merge|create|drop|alter|truncate|grant|revoke|copy|vacuum|commit|savepoint|set|into|temp|temporary|lock)\b' "$TMP/code.sql"; then
  fail "the report contains a write or DDL keyword"
fi

# The outer SELECT (from its own line to FROM user_rows) names only org_id and the four counts.
awk '/^SELECT$/{p=1} /^FROM user_rows$/{p=0} p' "$TMP/code.sql" >"$TMP/outer.sql"
[[ -s "$TMP/outer.sql" ]] || fail "no outer SELECT found"
if grep -Eiq '\b(content|key|topic|source_links|thread_id|user_id|owner|session_id|id|link|scope|memory)\b' "$TMP/outer.sql"; then
  fail "the outer SELECT names a content, key, link, scope or id column"
fi
for column in org_id in_space_withheld thread_absent_unprovable no_thread_link total_rows; do
  grep -qw "$column" "$TMP/outer.sql" || fail "the outer SELECT must report $column"
done

# The user-scope rows and the 'thread:' predicates are the ones the legacy room-origin filter uses.
grep -Fq "m.scope = 'user'" "$TMP/code.sql" || fail "the report must select scope = 'user' rows"
grep -Fq "cardinality(m.source_links) > 0" "$TMP/code.sql" || fail "the report must require a non-empty source_links"
for predicate in "sl.link LIKE 'thread:%'" "substr(sl.link, 8)" "ct.space_id IS NOT NULL" "ct.space_kind IS DISTINCT FROM 'personal'"; do
  grep -Fq "$predicate" "$TMP/code.sql" || fail "the report lacks the predicate: $predicate"
  grep -Fq "$predicate" "$ORIGIN_RS" || fail "session-core's room-origin filter no longer has: $predicate"
done

# 2. The wrapper, with a stub psql that records its arguments and exits with STUB_PSQL_RC.
STUBBIN="$TMP/bin"
mkdir -p "$STUBBIN"
cat >"$STUBBIN/psql" <<'STUB'
#!/usr/bin/env bash
printf '%s\n' "$@" >>"$PSQL_LOG"
exit "${STUB_PSQL_RC:-0}"
STUB
chmod +x "$STUBBIN/psql"
export PSQL_LOG="$TMP/psql-calls.log"

# 2a. Unset URL: no report, no psql call, the variable is named but its value is not.
rc=0
env -u SESSION_CORE_DATABASE_URL PATH="$STUBBIN:$PATH" bash "$SH" >"$TMP/a.out" 2>"$TMP/a.err" || rc=$?
[[ "$rc" == "2" ]] || fail "an unset URL must exit 2 (got $rc)"
grep -q "SESSION_CORE_DATABASE_URL is not set" "$TMP/a.err" || fail "an unset URL must name the variable on stderr"
[[ ! -s "$PSQL_LOG" ]] || fail "an unset URL must not call psql"

# 2b. A set URL: psql gets -X, ON_ERROR_STOP=1, -f <the report>, and the URL as the database argument.
URL="postgres://reporter:fixture-pass-aaaaaaaaaaaaaaaaaaaaaaaaaaaa@127.0.0.1:5432/session_core?sslmode=disable"
rc=0
env SESSION_CORE_DATABASE_URL="$URL" PATH="$STUBBIN:$PATH" bash "$SH" >"$TMP/b.out" 2>"$TMP/b.err" || rc=$?
[[ "$rc" == "0" ]] || fail "a clean psql run must exit 0 (got $rc)"
mapfile -t calls <"$PSQL_LOG"
[[ "${#calls[@]}" == "6" ]] || fail "psql must be called with six arguments (got ${#calls[@]})"
[[ "${calls[0]}" == "-X" && "${calls[1]}" == "-v" && "${calls[2]}" == "ON_ERROR_STOP=1" && "${calls[3]}" == "-f" ]] ||
  fail "psql must run as psql -X -v ON_ERROR_STOP=1 -f <report>"
[[ "$(basename "${calls[4]}")" == "report-user-memory-space-origin.sql" ]] || fail "psql must read the report SQL file"
[[ "${calls[5]}" == "$URL" ]] || fail "psql must receive the URL from the environment as its database argument"
grep -q "D3-266" "$TMP/b.out" || fail "the header must name register row D3-266"
grep -q "thread_absent_unprovable must be 0" "$TMP/b.out" || fail "the header must state that thread_absent_unprovable must be 0"
if grep -q "fixture-pass-aaaa" "$TMP/b.out" "$TMP/b.err"; then
  fail "the wrapper must never print the database URL or its password"
fi

# 2c. psql fails: the wrapper returns psql's own status.
rc=0
env SESSION_CORE_DATABASE_URL="$URL" STUB_PSQL_RC=3 PATH="$STUBBIN:$PATH" bash "$SH" >"$TMP/c.out" 2>"$TMP/c.err" || rc=$?
[[ "$rc" == "3" ]] || fail "a failing psql must make the wrapper exit with its status (got $rc)"
grep -q "psql exited with status 3" "$TMP/c.err" || fail "a failing psql must be reported on stderr"
if grep -q "fixture-pass-aaaa" "$TMP/c.out" "$TMP/c.err"; then
  fail "the wrapper must never print the database URL or its password on failure"
fi

echo "report-user-memory-space-origin test: ok"
