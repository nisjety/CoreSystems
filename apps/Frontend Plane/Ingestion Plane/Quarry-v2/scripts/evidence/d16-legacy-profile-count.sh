#!/usr/bin/env bash
# d16-legacy-profile-count.sh: count the saved browser profiles that have no owner, and what
# the owner check would do to them under `enforce` (decision D16 Q2, WS17.P2).
#
# It runs scripts/evidence/d16-legacy-profile-count.sql through psql. That file is read only
# (one READ ONLY transaction, counts and tenant ids, no payload, no owner id). This wrapper
# is a RECIPE for the stack session: it was syntax-checked offline (`--check`) and has NOT
# been run against any stack.
#
# Usage:
#   d16-legacy-profile-count.sh --check                 offline: bash syntax and a read-only guard
#   d16-legacy-profile-count.sh [--since <timestamp>]   run it
#
# Inputs (environment; the DSN never appears on the command line or in the output):
#   QUARRY_PROFILES_DSN_FILE   a file whose only content is the libpq connection string of the
#                              quarry Postgres (a user that can SELECT quarry_profiles)
# --since is the time the run-start owner stamp reached the live edge (ISO 8601, UTC). Without
# it, every ownerless row counts as written before the stamp.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
sql="$here/d16-legacy-profile-count.sql"

check() {
  bash -n "${BASH_SOURCE[0]}"
  [ -f "$sql" ] || { echo "FAIL missing $sql" >&2; return 1; }
  # The SQL must open a READ ONLY transaction and contain no statement that writes.
  grep -qE '^BEGIN READ ONLY;' "$sql" || { echo "FAIL no BEGIN READ ONLY" >&2; return 1; }
  # Comments are removed first, so a word in prose does not trip the guard.
  if sed -e 's/--.*$//' "$sql" \
    | grep -iE '\b(insert|update|delete|truncate|drop|alter|create|grant|revoke|copy|vacuum|reindex)\b' >/dev/null; then
    echo "FAIL the SQL names a write statement" >&2
    return 1
  fi
  # No payload, name or owner id may be selected.
  if sed -e 's/--.*$//' "$sql" | grep -iE '\b(snapshot|envelope)\b|\bname\b' >/dev/null; then
    echo "FAIL the SQL selects a payload or a name column" >&2
    return 1
  fi
  echo "OK d16-legacy-profile-count (syntax and read-only guard)"
}

since=""
while [ $# -gt 0 ]; do
  case "$1" in
    --check) check; exit $? ;;
    --since) since="${2:?--since needs a timestamp}"; shift 2 ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done

check >/dev/null
: "${QUARRY_PROFILES_DSN_FILE:?set QUARRY_PROFILES_DSN_FILE to a file holding the Postgres connection string}"
[ -r "$QUARRY_PROFILES_DSN_FILE" ] || { echo "cannot read QUARRY_PROFILES_DSN_FILE" >&2; exit 2; }
command -v psql >/dev/null || { echo "psql is required" >&2; exit 2; }

args=(-X -v ON_ERROR_STOP=1)
if [ -n "$since" ]; then
  case "$since" in
    *[!0-9TZ:.+-]*) echo "--since must be an ISO 8601 timestamp" >&2; exit 2 ;;
  esac
  args+=(-v "since='$since'")
fi
psql "$(cat "$QUARRY_PROFILES_DSN_FILE")" "${args[@]}" -f "$sql"
