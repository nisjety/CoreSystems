#!/usr/bin/env bash
set -uo pipefail

# Read-only report for depth register D3-266 (EP-8-S4): user-scope agent_memory rows whose origin Space cannot
# be proven from source_links. Run it before the Space memory containment switch goes on.
#
#   SESSION_CORE_DATABASE_URL='postgresql://<operator access>' scripts/report-user-memory-space-origin.sh
#
# The database URL comes from SESSION_CORE_DATABASE_URL and from nowhere else: never a file, never an argument
# this script prints. The SQL in report-user-memory-space-origin.sql runs in a READ ONLY transaction and ends
# with ROLLBACK. It writes nothing and creates nothing. The output is counts per org_id only: no content, keys,
# topics, source_links values, thread ids or user ids.
#
# Exit status: psql's own status (0 when the report ran; non-zero when psql failed; 2 when the URL is unset or
# psql is missing). A report that ran is not a verdict. Column thread_absent_unprovable must be 0, or reviewed
# by the owner, before the containment switch goes on. What to do with a non-zero count (purge, withhold or
# accept) is an owner decision that this script does not make.

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SQL_FILE="$SCRIPT_DIR/report-user-memory-space-origin.sql"

echo "report-user-memory-space-origin: register row D3-266 (EP-8-S4), read-only; column thread_absent_unprovable must be 0, or reviewed by the owner, before the containment switch goes on"

if [[ -z "${SESSION_CORE_DATABASE_URL:-}" ]]; then
  echo "report-user-memory-space-origin: SESSION_CORE_DATABASE_URL is not set; no report" >&2
  exit 2
fi
command -v psql >/dev/null 2>&1 || { echo "report-user-memory-space-origin: psql is not on PATH; no report" >&2; exit 2; }
[[ -r "$SQL_FILE" ]] || { echo "report-user-memory-space-origin: cannot read $SQL_FILE; no report" >&2; exit 2; }

psql -X -v ON_ERROR_STOP=1 -f "$SQL_FILE" "$SESSION_CORE_DATABASE_URL"
rc=$?
if [[ "$rc" -ne 0 ]]; then
  echo "report-user-memory-space-origin: psql exited with status $rc; no report" >&2
fi
exit "$rc"
