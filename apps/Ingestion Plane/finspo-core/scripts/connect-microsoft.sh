#!/usr/bin/env bash
set -Eeuo pipefail

# connect-microsoft.sh — drive the integration-core connect-session flow to
# register a `microsoft-graph` connection for an organization, WITHOUT any
# velion UI. This unblocks finspo end-to-end validation (Phases 2-4).
#
# What it does:
#   1. POSTs /api/v1/providers/<provider>/connect-session to integration-api
#      using the internal API key (bypasses org-scope; the 'pro' plan guard
#      still applies — see notes below).
#   2. Prints the Velion `connectUrl` you open in a browser to complete the
#      Microsoft OAuth consent.
#   3. Optionally polls /api/v1/connections until the connection goes active.
#
# After consent, integration-corev2 records the active connection. finspo's
# token broker (POST /internal/connectors/token) then resolves a Graph token
# for the org.
#
# Required env:
#   INTERNAL_API_KEY   integration-core internal API key (x-internal-api-key)
#   ORG_ID             organization id to attach the connection to
#
# Optional env (sane dev defaults):
#   INTEGRATION_API_URL   default http://localhost:3026
#   PROVIDER              default microsoft-graph
#   USER_ID               default smoke-test-user
#   USER_EMAIL            default smoke@example.com
#   WORKSPACE_ID          default ${ORG_ID}
#   POLL                  set to 1 to poll /connections after printing the link
#   POLL_TIMEOUT_SECONDS  default 300
#
# Usage:
#   INTERNAL_API_KEY=... ORG_ID=org_123 ./connect-microsoft.sh
#   INTERNAL_API_KEY=... ORG_ID=org_123 POLL=1 ./connect-microsoft.sh

INTEGRATION_API_URL="${INTEGRATION_API_URL:-http://localhost:3026}"
PROVIDER="${PROVIDER:-microsoft-graph}"
USER_ID="${USER_ID:-smoke-test-user}"
USER_EMAIL="${USER_EMAIL:-smoke@example.com}"
POLL="${POLL:-0}"
POLL_TIMEOUT_SECONDS="${POLL_TIMEOUT_SECONDS:-300}"

err() { printf '\033[31mERROR:\033[0m %s\n' "$*" >&2; }
info() { printf '\033[36m==>\033[0m %s\n' "$*"; }

require() {
  local name="$1"
  if [[ -z "${!name:-}" ]]; then
    err "$name is required"
    exit 2
  fi
}

require INTERNAL_API_KEY
require ORG_ID
WORKSPACE_ID="${WORKSPACE_ID:-$ORG_ID}"

# json_get <json> <jq-expression> — prefer jq, fall back to python3.
json_get() {
  local json="$1" expr="$2"
  if command -v jq >/dev/null 2>&1; then
    printf '%s' "$json" | jq -r "$expr"
    return
  fi
  if command -v python3 >/dev/null 2>&1; then
    # Translate a tiny subset of jq dot-paths (.a.b) into python lookups.
    local path="${expr#.}"
    python3 - "$json" "$path" <<'PY'
import json, sys
data = json.loads(sys.argv[1] or "{}")
path = sys.argv[2]
cur = data
if path:
    for part in path.split('.'):
        if isinstance(cur, dict) and part in cur:
            cur = cur[part]
        else:
            cur = None
            break
print('' if cur is None else (cur if isinstance(cur, str) else json.dumps(cur)))
PY
    return
  fi
  err "need either jq or python3 to parse JSON responses"
  exit 3
}

info "Requesting connect session for provider '$PROVIDER' (org=$ORG_ID)"
request_body=$(cat <<JSON
{
  "organizationId": "$ORG_ID",
  "workspaceId": "$WORKSPACE_ID",
  "userId": "$USER_ID",
  "userEmail": "$USER_EMAIL"
}
JSON
)

http_response=$(
  curl -sS -w $'\n%{http_code}' \
    -X POST "$INTEGRATION_API_URL/api/v1/providers/$PROVIDER/connect-session" \
    -H "Content-Type: application/json" \
    -H "x-internal-api-key: $INTERNAL_API_KEY" \
    -d "$request_body"
)
status="${http_response##*$'\n'}"
body="${http_response%$'\n'*}"

if [[ "$status" != "201" && "$status" != "200" ]]; then
  err "connect-session returned HTTP $status"
  printf '%s\n' "$body" >&2
  case "$status" in
    401) err "auth failed — check INTERNAL_API_KEY matches integration-core's internal key" ;;
    402|403) err "the 'pro' plan guard rejected this org. Either upgrade org '$ORG_ID' to a 'pro' entitlement in billing/org-core, or connect via velion v1's integrations UI (which runs as the logged-in user)." ;;
    404) err "provider '$PROVIDER' not registered. Confirm AZURE_CLIENT_ID/AZURE_CLIENT_SECRET are set on integration-corev2." ;;
  esac
  exit 1
fi

session_token=$(json_get "$body" '.data.sessionToken')
connect_url=$(json_get "$body" '.data.connectUrl')
expires_at=$(json_get "$body" '.data.expiresAt')

if [[ -z "$connect_url" || "$connect_url" == "null" ]]; then
  err "response did not include data.connectUrl"
  printf '%s\n' "$body" >&2
  exit 1
fi

cat <<BANNER

  ┌──────────────────────────────────────────────────────────────────────┐
  │  Connect session created. Open this URL in a browser and complete the  │
  │  Microsoft consent screen:                                             │
  └──────────────────────────────────────────────────────────────────────┘

  $connect_url

  session token : ${session_token:0:18}…
  expires       : $expires_at

BANNER

if [[ "$POLL" != "1" ]]; then
  info "Set POLL=1 to have this script wait for the connection to go active."
  exit 0
fi

info "Polling $INTEGRATION_API_URL/api/v1/connections for an active '$PROVIDER' connection (timeout ${POLL_TIMEOUT_SECONDS}s)…"
deadline=$((SECONDS + POLL_TIMEOUT_SECONDS))
while (( SECONDS < deadline )); do
  conn_response=$(
    curl -sS -w $'\n%{http_code}' \
      -X GET "$INTEGRATION_API_URL/api/v1/connections" \
      -H "x-internal-api-key: $INTERNAL_API_KEY" \
      -H "X-Org-ID: $ORG_ID" || true
  )
  conn_status="${conn_response##*$'\n'}"
  conn_body="${conn_response%$'\n'*}"

  if [[ "$conn_status" == "200" ]] && printf '%s' "$conn_body" | grep -qi "$PROVIDER"; then
    info "Active connection found:"
    printf '%s\n' "$conn_body"
    info "finspo can now resolve a Graph token for org '$ORG_ID'. Continue the smoke test at step 3 (GET /api/v1/sharepoint/sites)."
    exit 0
  fi
  sleep 5
done

err "Timed out after ${POLL_TIMEOUT_SECONDS}s waiting for an active connection."
err "Complete the consent in the browser, then re-run with POLL=1, or check integration-api logs for the OAuth callback."
exit 1
