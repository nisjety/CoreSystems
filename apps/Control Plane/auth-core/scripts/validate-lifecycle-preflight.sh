#!/bin/sh
set -eu

# Read-only release gate for historical authority gaps. It deliberately never
# selects candidate owners, creates memberships, or prints tenant identifiers.
run_psql() {
  if [ -n "${PSQL_DOCKER_CONTAINER:-}" ]; then
    case "$PSQL_DOCKER_CONTAINER" in
      *[!A-Za-z0-9_.-]*)
        echo 'AUTH_LIFECYCLE_PREFLIGHT_INVALID: unsafe fixture container' >&2
        exit 44
        ;;
    esac
    database_url_without_query="${DATABASE_URL%%\?*}"
    fixture_database="${database_url_without_query##*/}"
    case "$fixture_database" in
      ''|*[!A-Za-z0-9_-]*)
        echo 'AUTH_LIFECYCLE_PREFLIGHT_INVALID: unsafe fixture database' >&2
        exit 44
        ;;
    esac
    docker exec -e "PGOPTIONS=${PGOPTIONS:-}" "$PSQL_DOCKER_CONTAINER" \
      psql -U "${PSQL_DOCKER_USER:-control_lifecycle}" \
      -d "$fixture_database" "$@"
    return
  fi
  if [ -n "${DATABASE_URL:-}" ]; then
    psql --dbname="$DATABASE_URL" "$@"
  else
    psql -h "${DB_HOST:-controlplane-postgres}" \
      -U "${DB_USER:-aquatiq}" -d "${DB_NAME:-auth_service}" "$@"
  fi
}

counts="$(run_psql -X -A -t -F '|' -v ON_ERROR_STOP=1 -c "
  SELECT
    (SELECT COUNT(*)::BIGINT FROM owner_invariant_preflight_report),
    (SELECT COUNT(*)::BIGINT
     FROM organization authority_org
     WHERE NOT EXISTS (
       SELECT 1
       FROM member canonical_owner
       WHERE canonical_owner.organization_id = authority_org.id
         AND 'owner' = ANY(
           regexp_split_to_array(
             COALESCE(canonical_owner.role, ''),
             '[[:space:]]*,[[:space:]]*'
           )
         )
     )),
    (SELECT COUNT(*)::BIGINT
     FROM accepted_invitation_membership_gap_report);
")"

old_ifs="$IFS"
IFS='|'
set -- $counts
IFS="$old_ifs"
owner_report_count="${1:-invalid}"
ownerless_count="${2:-invalid}"
invitation_gap_count="${3:-invalid}"

case "$owner_report_count:$ownerless_count:$invitation_gap_count" in
  *[!0-9:]*|'')
    echo 'AUTH_LIFECYCLE_PREFLIGHT_INVALID: database returned malformed counts' >&2
    exit 44
    ;;
esac

if [ "$owner_report_count" -ne 0 ] || [ "$ownerless_count" -ne 0 ]; then
  echo "OWNER_INVARIANT_PREFLIGHT_FAILED: ${owner_report_count} review issue(s), ${ownerless_count} ownerless organization(s); inspect owner_invariant_preflight_report" >&2
  exit 42
fi
if [ "$invitation_gap_count" -ne 0 ]; then
  echo "INVITATION_MEMBERSHIP_PREFLIGHT_FAILED: ${invitation_gap_count} accepted invitation membership gap(s); inspect accepted_invitation_membership_gap_report" >&2
  exit 43
fi

echo 'Auth lifecycle preflight passed'
