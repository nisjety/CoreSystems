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
#     [--import-write-entitled] [--agent-action-entitled] [--schedule-fire-entitled] \
#     [--thread-read-entitled] [--sandbox-capability-entitled] \
#     [--process-registry-entitled]
#
# --thread-read-entitled is what lets a personal Space's owner read the result
# thread of their own schedule (and a room's current recipients read the room's
# conversation): without it every Control thread-read decision is refused.
#
# Entitlements are sent ONLY when their flag is passed. An entitlement flag that
# is not passed is left out of the request, and Control keeps the stored value
# (a brand-new row starts with every entitlement off). A re-run can therefore
# never clear a bit it did not mention; to turn a bit off use the org-admin
# console, not this script. Every change is recorded (actor
# service:control-space-policy) in space_effect_policy_events and audited
# through the user-core audit outbox. Creating the row (the privacy attestation)
# is an operator act and stays here.
#
# Where to run it: user-core listens on 3012 and has no host port mapping, so
# no localhost:<port> works from the host. Run this inside the Control Plane
# network, e.g.
#   docker compose run --rm <service-with-curl> bash scripts/set-space-effect-policy.sh ...
# or `docker exec` into a Control container that has curl, where the default
# USER_CORE_URL (http://user-core:3012) resolves. Override USER_CORE_URL only
# if your network names the service differently.
set -euo pipefail

USER_CORE_URL="${USER_CORE_URL:-http://user-core:3012}"
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
# Entitlement flags stay empty unless passed: an empty value is "not stated" and
# the key is left out of the request body.
thread_create_entitled=""
retrieval_read_entitled=""
import_write_entitled=""
agent_action_entitled=""
schedule_fire_entitled=""
thread_read_entitled=""
sandbox_capability_entitled=""
process_registry_entitled=""

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
    --thread-read-entitled) thread_read_entitled=true; shift ;;
    --sandbox-capability-entitled) sandbox_capability_entitled=true; shift ;;
    --process-registry-entitled) process_registry_entitled=true; shift ;;
    *) echo "unknown argument: $1" >&2; exit 1 ;;
  esac
done

for required in org_id privacy_policy_ref purpose lawful_basis privacy_class retention_class residency deletion_scope; do
  if [[ -z "${!required}" ]]; then
    echo "missing required --${required//_/-}" >&2
    exit 1
  fi
done

# Send an entitlement key only when its flag was passed.
entitlement_json=""
for key in thread_create retrieval_read import_write agent_action schedule_fire thread_read sandbox_capability process_registry; do
  var="${key}_entitled"
  if [[ -n "${!var}" ]]; then
    entitlement_json+=$',\n  "'"${key}"'_entitled": '"${!var}"
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
  "zero_data_retention": ${zero_data_retention}${entitlement_json}
}
JSON
)")

if [[ "$status" != "200" ]]; then
  echo "effect-policy upsert failed (${status}): $(cat "$body")" >&2
  exit 1
fi

echo "effect-policy upserted for org ${org_id}: $(cat "$body")"
