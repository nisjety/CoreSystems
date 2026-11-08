#!/usr/bin/env bash
# Turn an organization's room directory on or off through the real Control
# Plane API (PUT /api/v1/internal/spaces/directory-policy, gated on the
# control-space-policy service principal).
#
# space_directory_policies is deny-by-default: no row means the organization has
# no room directory, no room can be made org_public and no room can be joined.
# Enabling it is an operator act, ledgered in space_directory_policy_events
# (one row per real change; a replay changes nothing). Disabling it hides the
# directory and refuses every join, but does not touch existing memberships or
# the visibility stored on rooms.
#
# Usage:
#   CONTROL_SPACE_POLICY_TOKEN=... bash scripts/set-space-directory-policy.sh \
#     --org-id org_123 (--enable | --disable)
#
# Where to run it: user-core has no host port mapping. Run it inside the Control
# Plane network (a container with curl where the default
# USER_CORE_URL=http://user-core:3012 resolves), as for
# set-space-effect-policy.sh.
set -euo pipefail

USER_CORE_URL="${USER_CORE_URL:-http://user-core:3012}"
: "${CONTROL_SPACE_POLICY_TOKEN:?CONTROL_SPACE_POLICY_TOKEN is required (the control-space-policy service credential)}"

org_id=""
enabled=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --org-id) org_id="$2"; shift 2 ;;
    --enable) enabled=true; shift ;;
    --disable) enabled=false; shift ;;
    *) echo "unknown argument: $1" >&2; exit 1 ;;
  esac
done

if [[ -z "$org_id" ]]; then
  echo "missing required --org-id" >&2
  exit 1
fi
if [[ -z "$enabled" ]]; then
  echo "pass exactly one of --enable or --disable" >&2
  exit 1
fi
if [[ ! "$org_id" =~ ^[A-Za-z0-9_.:@-]+$ ]]; then
  echo "--org-id contains characters that are not allowed" >&2
  exit 1
fi

body=$(mktemp)
trap 'rm -f "$body"' EXIT

status=$(curl -s -o "$body" -w '%{http_code}' \
  -X PUT "${USER_CORE_URL}/api/v1/internal/spaces/directory-policy" \
  -H 'content-type: application/json' \
  -H "X-Service-Id: control-space-policy" \
  -H "X-Service-Token: ${CONTROL_SPACE_POLICY_TOKEN}" \
  -d "{\"org_id\": \"${org_id}\", \"org_public_rooms_enabled\": ${enabled}}")

if [[ "$status" != "200" ]]; then
  echo "directory-policy upsert failed (${status}): $(cat "$body")" >&2
  exit 1
fi

echo "directory-policy set for org ${org_id} (org_public_rooms_enabled=${enabled}): $(cat "$body")"
