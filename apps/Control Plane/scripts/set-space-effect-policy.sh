#!/usr/bin/env bash
# Provision or update an org's space_effect_policies row through the real
# Control Plane API instead of a raw SQL patch.
#
# space_effect_policies is deny-by-default: no row means every scoped Space
# decision (personal AND shared thread creation, retrieval, imports,
# schedules, agent actions) is denied for that org, by design ("A missing
# policy row is an authorization denial, never an implicit permissive
# default" — internal/spaces/personal_thread_decision.go). The authenticated
# endpoint this script calls (PUT /api/v1/internal/spaces/effect-policy,
# gated on the control-space-policy service principal) already existed and
# was already wired with a live credential (CONTROL_SPACE_POLICY_TOKEN) —
# this script exists only because nothing previously called it, which is why
# the one org that has a row today was seeded by hand with psql instead.
#
# This script never invents privacy/compliance defaults: every field below
# is a required argument or environment variable. Confirm purpose,
# lawful_basis, privacy_class, retention_class, residency, deletion_scope,
# and zero_data_retention against the org's actual contract/DPA before
# running this — they are compliance facts, not engineering defaults.
#
# Usage:
#   CONTROL_SPACE_POLICY_TOKEN=... bash scripts/set-space-effect-policy.sh \
#     --org-id org_123 \
#     --privacy-policy-ref privacy:org_123:1 \
#     --purpose assistant_collaboration \
#     --lawful-basis contract \
#     --privacy-class internal \
#     --retention-class standard \
#     --residency swedencentral \
#     --deletion-scope space \
#     [--third-party-processing-allowed] \
#     [--zero-data-retention] \
#     [--thread-create-entitled] [--retrieval-read-entitled] \
#     [--import-write-entitled] [--agent-action-entitled] [--schedule-fire-entitled]
set -euo pipefail

USER_CORE_URL="${USER_CORE_URL:-http://localhost:3021}"
: "${CONTROL_SPACE_POLICY_TOKEN:?CONTROL_SPACE_POLICY_TOKEN is required (the control-space-policy service credential)}"

org_id=""
privacy_policy_ref=""
purpose=""
lawful_basis=""
privacy_class=""
retention_class=""
residency=""
deletion_scope=""
third_party_processing_allowed=false
zero_data_retention=false
thread_create_entitled=false
retrieval_read_entitled=false
import_write_entitled=false
agent_action_entitled=false
schedule_fire_entitled=false

while [[ $# -gt 0 ]]; do
  case "$1" in
    --org-id) org_id="$2"; shift 2 ;;
    --privacy-policy-ref) privacy_policy_ref="$2"; shift 2 ;;
    --purpose) purpose="$2"; shift 2 ;;
    --lawful-basis) lawful_basis="$2"; shift 2 ;;
    --privacy-class) privacy_class="$2"; shift 2 ;;
    --retention-class) retention_class="$2"; shift 2 ;;
    --residency) residency="$2"; shift 2 ;;
    --deletion-scope) deletion_scope="$2"; shift 2 ;;
    --third-party-processing-allowed) third_party_processing_allowed=true; shift ;;
    --zero-data-retention) zero_data_retention=true; shift ;;
    --thread-create-entitled) thread_create_entitled=true; shift ;;
    --retrieval-read-entitled) retrieval_read_entitled=true; shift ;;
    --import-write-entitled) import_write_entitled=true; shift ;;
    --agent-action-entitled) agent_action_entitled=true; shift ;;
    --schedule-fire-entitled) schedule_fire_entitled=true; shift ;;
    *) echo "unknown argument: $1" >&2; exit 1 ;;
  esac
done

for required in org_id privacy_policy_ref purpose lawful_basis privacy_class retention_class residency deletion_scope; do
  if [[ -z "${!required}" ]]; then
    echo "missing required --${required//_/-}" >&2
    exit 1
  fi
done

body=$(mktemp)
trap 'rm -f "$body"' EXIT

status=$(curl -s -o "$body" -w '%{http_code}' \
  -X PUT "${USER_CORE_URL}/api/v1/internal/spaces/effect-policy" \
  -H 'content-type: application/json' \
  -H "X-Service-Id: control-space-policy" \
  -H "X-Service-Token: ${CONTROL_SPACE_POLICY_TOKEN}" \
  -d "$(cat <<JSON
{
  "org_id": "${org_id}",
  "privacy_policy_ref": "${privacy_policy_ref}",
  "purpose": "${purpose}",
  "lawful_basis": "${lawful_basis}",
  "privacy_class": "${privacy_class}",
  "third_party_processing_allowed": ${third_party_processing_allowed},
  "retention_class": "${retention_class}",
  "residency": "${residency}",
  "deletion_scope": "${deletion_scope}",
  "zero_data_retention": ${zero_data_retention},
  "thread_create_entitled": ${thread_create_entitled},
  "retrieval_read_entitled": ${retrieval_read_entitled},
  "import_write_entitled": ${import_write_entitled},
  "agent_action_entitled": ${agent_action_entitled},
  "schedule_fire_entitled": ${schedule_fire_entitled}
}
JSON
)")

if [[ "$status" != "200" ]]; then
  echo "effect-policy upsert failed (${status}): $(cat "$body")" >&2
  exit 1
fi

echo "effect-policy upserted for org ${org_id}: $(cat "$body")"
