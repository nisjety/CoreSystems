#!/usr/bin/env bash
set -uo pipefail

# Read-only governance check for skill revision attribution (depth register D1-645).
#
#   scripts/verify-skill-revision-attribution.sh --since <ISO date or timestamp>
#
# session-core logs every change to a skill as one skill_revisions row (migration 0064). A writer that
# names no actor is still logged, as actor_kind 'unattributed', so registry edits made before the
# attribution deploy of capability-core land there. This check counts the rows per actor_kind from
# --since (the attribution deploy time) on, and fails while any of them is unattributed.
#
# Exit status: 0 no unattributed revision since --since (zero rows pass); 1 at least one, with the
# count and the earliest created_at of those; 2 a usage error, or a database that could not be read or
# did not answer as expected. A failure is never a pass.
#
# It reads, never writes: one SELECT, run with default_transaction_read_only=on through docker exec,
# with the container and database names of scripts/tests/zdr-live-proof.sh (the same MP_* variables and
# defaults). The output is actor_kind, row counts and one timestamp. The statement reads actor_kind and
# created_at only (created_at in the filter and in min); it selects no skill text, names, owner ids or
# org ids.

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
  echo "usage: verify-skill-revision-attribution.sh --since <ISO date or timestamp>"
  echo "exit 0: no unattributed skill revision since --since; 1: some; 2: usage error or unreadable database"
  exit 0
fi

SINCE=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --since)
      [[ $# -ge 2 && -n "$2" ]] || { echo "verify-skill-revision-attribution: --since needs a value" >&2; exit 2; }
      SINCE="$2"
      shift
      ;;
    *) echo "verify-skill-revision-attribution: unknown argument '$1'" >&2; exit 2 ;;
  esac
  shift
done
[[ -n "$SINCE" ]] || { echo "verify-skill-revision-attribution: --since <ISO date or timestamp> is required" >&2; exit 2; }
SINCE_RE='^[0-9]{4}-[0-9]{2}-[0-9]{2}([T ][0-9:.+Z-]{1,40})?$'
[[ "$SINCE" =~ $SINCE_RE ]] ||
  { echo "verify-skill-revision-attribution: --since must be an ISO date or timestamp, for example 2026-11-20T09:00:00Z" >&2; exit 2; }

command -v docker >/dev/null 2>&1 || { echo "verify-skill-revision-attribution: docker is not available" >&2; exit 2; }
MP_PREFIX="${MP_CONTAINER_PREFIX:-model-plane}"
MP_POSTGRES="${MP_POSTGRES_CONTAINER:-$MP_PREFIX-postgres-1}"
MP_DB="${MP_SESSION_CORE_DB:-session_core}"
MP_DB_USER="${MP_POSTGRES_USER:-postgres}"

# One statement. psql expands :'since' only in a script, not in -c, so the statement goes on stdin and
# the value is bound as a psql variable, never spliced into the text. min(created_at) is the earliest
# revision of each actor_kind; only the unattributed one is printed.
SQL="select actor_kind, count(*), min(created_at) from skill_revisions where created_at >= :'since' group by 1 order by 1"
if ! rows="$(printf '%s;\n' "$SQL" | docker exec -i -e PGOPTIONS='-c default_transaction_read_only=on' "$MP_POSTGRES" \
  psql -U "$MP_DB_USER" -d "$MP_DB" -At -F '|' -X -v ON_ERROR_STOP=1 -v "since=$SINCE")"; then
  echo "verify-skill-revision-attribution: could not read skill_revisions from container $MP_POSTGRES, database $MP_DB; no verdict" >&2
  exit 2
fi

unattributed=0
earliest=""
while IFS='|' read -r kind count first; do
  [[ -n "$kind" ]] || continue
  if ! [[ "$kind" =~ ^(user|service|system|unattributed)$ && "$count" =~ ^[0-9]+$ ]]; then
    echo "verify-skill-revision-attribution: the read returned a row this check does not understand; no verdict" >&2
    exit 2
  fi
  echo "actor_kind=$kind rows=$count"
  if [[ "$kind" == "unattributed" ]]; then
    unattributed="$count"
    earliest="$first"
  fi
done <<<"$rows"

if [[ "$unattributed" -gt 0 ]]; then
  echo "verdict: FAIL, unattributed=$unattributed earliest_created_at=$earliest"
  exit 1
fi
echo "verdict: PASS, unattributed=0"
exit 0
