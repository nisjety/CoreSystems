#!/usr/bin/env bash
set -euo pipefail

# Read-only cross-plane runtime preflight. This is deliberately narrower than
# an end-to-end release probe: it proves that the expected local containers and
# authority inputs exist, while keeping missing runtime evidence explicit. It
# never prints environment values, creates credentials, or mutates a service.

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MODEL_EVIDENCE="$ROOT_DIR/apps/Model Plane/docs/MODEL_PLANE_RELEASE_EVIDENCE.md"

usage() {
  cat <<'EOF'
Usage: scripts/coresystem-cross-plane-preflight.sh

Inspect the local CoreSystem Docker stack and release-evidence markers in a
read-only mode without reading or printing credential values. Exit 0 means the
local topology and required named inputs are present. Exit 2 means a release
gate remains open.
EOF
}

if [[ "${1:-}" == "--help" || "${1:-}" == "-h" ]]; then
  usage
  exit 0
fi
if [[ $# -ne 0 ]]; then
  usage >&2
  exit 64
fi

command -v docker >/dev/null 2>&1 || {
  echo "BLOCKED docker: command is unavailable"
  exit 2
}

if expiry="$(date -u -v+15M '+%Y-%m-%dT%H:%M:%SZ' 2>/dev/null)"; then
  :
else
  expiry="$(date -u -d '+15 minutes' '+%Y-%m-%dT%H:%M:%SZ')"
fi

blockers=()

find_container() {
  local project="$1" service="$2" fallback="$3" name
  name="$(docker ps \
    --filter "label=com.docker.compose.project=$project" \
    --filter "label=com.docker.compose.service=$service" \
    --format '{{.Names}}' | head -n 1)"
  if [[ -n "$name" ]]; then
    printf '%s\n' "$name"
  else
    printf '%s\n' "$fallback"
  fi
}

container_exists() {
  docker inspect "$1" >/dev/null 2>&1
}

container_health() {
  local name="$1" state health
  state="$(docker inspect --format '{{.State.Status}}' "$name" 2>/dev/null || printf 'missing')"
  health="$(docker inspect --format '{{if .State.Health}}{{.State.Health.Status}}{{else}}no-healthcheck{{end}}' "$name" 2>/dev/null || printf 'missing')"
  printf '%s/%s\n' "$state" "$health"
}

env_state() {
  local name="$1" key="$2"
  docker inspect --format '{{range .Config.Env}}{{println .}}{{end}}' "$name" 2>/dev/null |
    awk -F= -v wanted="$key" '$1 == wanted { found=1; print (length($0) > length($1) + 1 ? "set" : "empty"); exit } END { if (!found) print "missing" }'
}

# Probe only the non-secret shape of the active service registries. The
# credential fields are parsed but never emitted. This catches the easy-to-miss
# failure mode where a token variable is present but the User Core/Auth Core
# registry does not grant that exact principal, audience, and scope.
principal_scope_state() {
  local name="$1" registry_key="$2" principal="$3" audience="$4" scope="$5"
  docker inspect --format '{{range .Config.Env}}{{println .}}{{end}}' "$name" 2>/dev/null |
    python3 -c '
import json
import sys

registry_key, principal, audience, scope = sys.argv[1:]
raw = sys.stdin.read()
for line in raw.splitlines():
    if not line.startswith(registry_key + "="):
        continue
    value = line.split("=", 1)[1]
    try:
        registry = json.loads(value)
    except Exception:
        print("malformed")
        raise SystemExit(0)

    if registry_key == "USER_CORE_SERVICE_CREDENTIALS":
        entries = registry if isinstance(registry, list) else []
        for entry in entries:
            if not isinstance(entry, dict):
                continue
            if entry.get("principal") != principal or entry.get("audience") != audience:
                continue
            if scope in (entry.get("scopes") or []):
                print("present")
                raise SystemExit(0)
        print("missing")
        raise SystemExit(0)

    entry = registry.get(principal) if isinstance(registry, dict) else None
    if not isinstance(entry, dict) or audience not in (entry.get("audiences") or []):
        print("missing")
        raise SystemExit(0)
    by_audience = entry.get("scopesByAudience") or {}
    scopes = by_audience.get(audience, entry.get("scopes") or [])
    print("present" if scope in scopes else "missing")
    raise SystemExit(0)

print("registry_missing")
' "$registry_key" "$principal" "$audience" "$scope" 2>/dev/null || printf 'probe_error\n'
}

check_container() {
  local label="$1" name="$2" health
  if ! container_exists "$name"; then
    blockers+=("${label}_container_missing")
    printf 'BLOCKED %-18s container=%s state=missing\n' "$label" "$name"
    return
  fi
  health="$(container_health "$name")"
  printf 'CHECKED %-18s container=%s state=%s\n' "$label" "$name" "$health"
  if [[ "$health" != running/healthy && "$health" != running/no-healthcheck ]]; then
    blockers+=("${label}_container_unhealthy")
  fi
}

check_key() {
  local label="$1" name="$2" key="$3" state
  state="$(env_state "$name" "$key")"
  printf 'CONFIG  %-18s key=%s state=%s\n' "$label" "$key" "$state"
  if [[ "$state" != set ]]; then
    blockers+=("${label}_${key}_${state}")
  fi
}

check_principal_scope() {
  local label="$1" name="$2" registry_key="$3" principal="$4" audience="$5" scope="$6" state
  state="$(principal_scope_state "$name" "$registry_key" "$principal" "$audience" "$scope")"
  printf 'AUTHZ   %-18s principal=%s audience=%s scope=%s state=%s\n' "$label" "$principal" "$audience" "$scope" "$state"
  if [[ "$state" != present ]]; then
    blockers+=("${label}_${principal}_${audience}_${scope}_${state}")
  fi
}

check_evidence_open() {
  local label="$1" pattern="$2"
  if [[ ! -f "$MODEL_EVIDENCE" ]]; then
    printf 'EVIDENCE %-16s state=missing\n' "$label"
    blockers+=("${label}_evidence_missing")
  elif rg -q -- "$pattern" "$MODEL_EVIDENCE"; then
    printf 'EVIDENCE %-16s state=open\n' "$label"
    blockers+=("${label}_evidence_open")
  else
    printf 'EVIDENCE %-16s state=recorded\n' "$label"
  fi
}

printf 'cross-plane dev preflight; expires=%s\n' "$expiry"

control_user="$(find_container control-plane user-core user-service)"
control_session="$(find_container control-plane session-core session-core-service)"
control_audit="$(find_container control-plane audit-core audit-core-service)"
control_auth="$(find_container control-plane auth-core auth-service)"
application_conversation="$(find_container application-plane conversation-core-go conversation-core-go)"
application_notifications="$(find_container application-plane notification-core notification-core)"
application_convex="$(find_container application-plane convex-gateway convex-gateway)"
data_documents="$(find_container data-plane-v2 documents-api data-plane-v2-documents-api-1)"
data_retrieval="$(find_container data-plane-v2 retrieval-engine data-plane-v2-retrieval-engine-1)"
data_orchestrator="$(find_container data-plane-v2 data-orchestrator data-plane-v2-data-orchestrator-1)"
ingestion_integration="$(find_container ingestion-plane integration-api integration-api)"
ingestion_quarry_control="$(find_container ingestion-plane quarry-control quarry-control)"
ingestion_quarry_orchestrator="$(find_container ingestion-plane quarry-orchestrator quarry-orchestrator)"
ingestion_imports="$(find_container ingestion-plane imports-api imports-api)"
frontend_gateway="$(find_container frontend-plane-verevonv3 gateway verevon-gateway-rs)"
frontend_web="$(find_container frontend-plane-verevonv3 frontend frontend-plane-verevonv3-frontend-1)"
model_temporal="$(find_container model-plane temporal model-plane-temporal-1)"
model_nats="$(find_container model-plane nats model-plane-nats-1)"
model_execution="$(find_container model-plane execution-core model-plane-execution-core-1)"

check_container control-user "$control_user"
check_container control-session "$control_session"
check_container control-audit "$control_audit"
check_container control-auth "$control_auth"
check_container application-conversation "$application_conversation"
check_container application-notifications "$application_notifications"
check_container application-convex "$application_convex"
check_container data-documents "$data_documents"
check_container data-retrieval "$data_retrieval"
check_container data-orchestrator "$data_orchestrator"
check_container ingestion-integration "$ingestion_integration"
check_container ingestion-quarry-control "$ingestion_quarry_control"
check_container ingestion-quarry-orchestrator "$ingestion_quarry_orchestrator"
check_container ingestion-imports "$ingestion_imports"
check_container frontend-gateway "$frontend_gateway"
check_container frontend-web "$frontend_web"
check_container model-temporal "$model_temporal"
check_container model-nats "$model_nats"
check_container model-execution "$model_execution"

# Authority and owner-effect inputs are presence checks only. Values are never
# emitted, so this remains safe to run against the internal dev stack.
check_key control-user "$control_user" CONTROL_SPACE_DECISION_KEY_ID
check_key control-user "$control_user" CONTROL_SPACE_DECISION_PRIVATE_KEY_BASE64
check_key application-conversation "$application_conversation" CONTROL_RUN_ACTION_DECISION_KEY_ID
check_key application-conversation "$application_conversation" CONTROL_RUN_ACTION_DECISION_PUBLIC_KEY_BASE64
check_key application-conversation "$application_conversation" CONTROL_RUN_ACTION_AUTHORITY_URL
check_key application-conversation "$application_conversation" CONVERSATION_CONTROL_RUN_ACTION_AUTHORITY_TOKEN
check_key application-conversation "$application_conversation" CONTROL_OWNER_EFFECT_RESERVATION_URL
check_key application-conversation "$application_conversation" CONVERSATION_CONTROL_OWNER_EFFECT_RESERVATION_TOKEN
check_key application-notifications "$application_notifications" NOTIFICATION_DELIVERY_MODE

# The dedicated Conversation Core owner-action reporter is a separate R-2
# release input from the generic execution-core health reporter. Presence is
# all this probe records; the service credential is never printed.
check_key application-conversation "$application_conversation" CAPABILITY_CORE_HTTP_URL
check_key application-conversation "$application_conversation" CONVERSATION_CAPABILITY_HEALTH_AUTH_CORE_URL
check_key application-conversation "$application_conversation" CONVERSATION_CAPABILITY_HEALTH_SERVICE_ID
check_key application-conversation "$application_conversation" CONVERSATION_CAPABILITY_HEALTH_SERVICE_API_KEY

# The Model-side owner adapter is a separate release input from the browser
# route. Keep its exact four-hop bindings visible without printing values;
# missing any one keeps tickets.create out of the Model allowlist.
check_key model-execution "$model_execution" CONTROL_PLANE_USER_CORE_URL
check_key model-execution "$model_execution" EXECUTION_CORE_CONTROL_RUN_ACTION_TOKEN
check_key model-execution "$model_execution" CONVERSATION_CORE_AGENT_ACTION_URL
check_key model-execution "$model_execution" CONVERSATION_EXECUTION_CORE_SERVICE_TOKEN
check_key model-execution "$model_execution" EXECUTION_CORE_CONTROL_MODEL_ACTION_VIEW_TOKEN

# Registry checks deliberately remain separate from environment presence. A
# deployment must register the exact service principal and scope at the
# audience that receives the request; a broad or differently-audienced token is
# not an acceptable substitute. These probes print names/states only.
check_principal_scope control-user "$control_user" USER_CORE_SERVICE_CREDENTIALS capability-core user-core spaces:schedule:reauthorize
check_principal_scope control-user "$control_user" USER_CORE_SERVICE_CREDENTIALS orchestrator-core user-core spaces:schedule:execute
check_principal_scope control-user "$control_user" USER_CORE_SERVICE_CREDENTIALS orchestrator-core user-core spaces:schedule:step
check_principal_scope control-user "$control_user" USER_CORE_SERVICE_CREDENTIALS execution-core user-core spaces:agent-action:reauthorize
check_principal_scope control-user "$control_user" USER_CORE_SERVICE_CREDENTIALS execution-core user-core spaces:agent-action:view
check_principal_scope control-user "$control_user" USER_CORE_SERVICE_CREDENTIALS conversation-core user-core spaces:agent-action:current-authority
check_principal_scope control-user "$control_user" USER_CORE_SERVICE_CREDENTIALS conversation-core user-core spaces:agent-action:reservation
check_principal_scope control-auth "$control_auth" PLANE_SERVICE_PRINCIPALS_JSON execution-core capability-core capability:health:global:write
check_principal_scope control-auth "$control_auth" PLANE_SERVICE_PRINCIPALS_JSON conversation-core capability-core capability:read
check_principal_scope control-auth "$control_auth" PLANE_SERVICE_PRINCIPALS_JSON conversation-core capability-core capability:owner-action:health:write

# These rows are intentionally open in the current evidence register. Keeping
# them machine-readable prevents a healthy local topology from being mistaken
# for a candidate, provider/ZDR, approval, or delivery proof.
check_evidence_open provider_zdr 'One provider deployment is independently ZDR-attested.*\| absent \| absent \| Release blocker'
check_evidence_open approval_continuation 'Approval delivery resumes the exact approved action.*\| absent \| absent \|'
check_evidence_open candidate_observation 'Service-owned scheduled step.*\| absent \| absent \|'
# Keep the two product-facing release gates explicit as well. Source and
# disposable-Postgres proofs are useful evidence, but they do not prove the
# deployed Application delivery worker/HA path or a causal owner receipt in
# the Space Activity projection.
check_evidence_open application_delivery_ha 'Durable feed reconciliation, HA replay, provider/ZDR attestation, and candidate observation remain open'
check_evidence_open capability_owner_reporter 'dedicated owner-reporter stale/outage, provider, candidate, and rollback evidence remain required'
check_evidence_open owner_effect_observation 'No owner-effect receipt'

if ((${#blockers[@]} > 0)); then
  printf 'STATUS blocked; cross-plane release gates remain fail-closed\n'
  printf 'BLOCKER %s\n' "${blockers[@]}"
  exit 2
fi

printf 'STATUS ready-for-cross-plane-proof; this does not establish candidate or provider/ZDR evidence\n'
