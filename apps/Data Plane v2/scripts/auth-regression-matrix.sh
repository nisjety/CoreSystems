#!/usr/bin/env bash
set -euo pipefail
# A caller's xtrace setting must never make bearer values visible.
set +x

# Data Plane v2 HTTP authorization regression matrix.
#
# This harness is intentionally limited to an isolated/local Compose project.
# It prints only route-family labels and HTTP status codes: never bearer values,
# organization identifiers, URLs containing tenant identifiers, or bodies.
# Quickwit is exercised only through its tenant-scoped dry-run preview.

PASS=0
FAIL=0

pass() {
  printf '  PASS %-24s %-28s status=%s\n' "$1" "$2" "$3"
  PASS=$((PASS + 1))
}

fail() {
  printf '  FAIL %-24s %-28s status=%s\n' "$1" "$2" "$3" >&2
  FAIL=$((FAIL + 1))
}

configuration_error() {
  printf 'configuration error: %s\n' "$1" >&2
  exit 2
}

require_env() {
  local name=$1
  [ -n "${!name:-}" ] || configuration_error "$name is required"
}

valid_org_id() {
  local value=$1
  [ ${#value} -le 128 ] && [[ "$value" =~ ^[A-Za-z0-9][A-Za-z0-9._:-]*$ ]]
}

valid_bearer() {
  local value=$1
  [ ${#value} -le 8192 ] && [[ "$value" =~ ^[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+$ ]]
}

for required in \
  DPV2_TEST_ORG_ID \
  DPV2_SPOOF_ORG_ID \
  DPV2_USER_BEARER \
  DPV2_QUALITY_BEARER \
  DPV2_QUICKWIT_ADMIN_BEARER; do
  require_env "$required"
done

valid_org_id "$DPV2_TEST_ORG_ID" || configuration_error "DPV2_TEST_ORG_ID has an invalid format"
valid_org_id "$DPV2_SPOOF_ORG_ID" || configuration_error "DPV2_SPOOF_ORG_ID has an invalid format"
[ "$DPV2_TEST_ORG_ID" != "$DPV2_SPOOF_ORG_ID" ] || configuration_error "test and spoof organizations must differ"
valid_bearer "$DPV2_USER_BEARER" || configuration_error "DPV2_USER_BEARER must be a compact JWT"
valid_bearer "$DPV2_QUALITY_BEARER" || configuration_error "DPV2_QUALITY_BEARER must be a compact JWT"
valid_bearer "$DPV2_QUICKWIT_ADMIN_BEARER" || configuration_error "DPV2_QUICKWIT_ADMIN_BEARER must be a compact JWT"

TRANSPORT=${DPV2_MATRIX_TRANSPORT:-compose}
case "$TRANSPORT" in
  compose)
    command -v docker >/dev/null 2>&1 || configuration_error "docker is required for compose transport"
    ;;
  external)
    command -v curl >/dev/null 2>&1 || configuration_error "curl is required for external transport"
    ;;
  *)
    configuration_error "DPV2_MATRIX_TRANSPORT must be compose or external"
    ;;
esac

if [ -n "${DPV2_COMPOSE_PROJECT:-}" ] && ! [[ "$DPV2_COMPOSE_PROJECT" =~ ^[A-Za-z0-9][A-Za-z0-9_.-]{0,127}$ ]]; then
  configuration_error "DPV2_COMPOSE_PROJECT has an invalid format"
fi

is_loopback_url() {
  case "$1" in
    http://localhost:*|http://127.0.0.1:*|http://\[::1\]:*) return 0 ;;
    *) return 1 ;;
  esac
}

base_url() {
  local service=$1
  local port=$2
  local env_name=$3

  if [ "$TRANSPORT" = "compose" ]; then
    printf 'http://127.0.0.1:%s' "$port"
    return
  fi

  local value=${!env_name:-}
  [ -n "$value" ] || configuration_error "$env_name is required for external transport"
  is_loopback_url "$value" || configuration_error "$env_name must be an explicit loopback HTTP endpoint"
  printf '%s' "${value%/}"
}

GRAPH_BASE=$(base_url graph-index 9203 DPV2_GRAPH_URL)
QUALITY_BASE=$(base_url data-quality 8013 DPV2_QUALITY_URL)
ORCHESTRATOR_BASE=$(base_url data-orchestrator 8012 DPV2_ORCHESTRATOR_URL)
DOCUMENTS_BASE=$(base_url documents-api 8010 DPV2_DOCUMENTS_URL)
RETRIEVAL_BASE=$(base_url retrieval-engine 8004 DPV2_RETRIEVAL_URL)
WIKI_BASE=$(base_url wiki-store 8011 DPV2_WIKI_URL)
QUICKWIT_BASE=$(base_url quickwit-adapter 9204 DPV2_QUICKWIT_URL)

curl_escape() {
  local value=$1
  value=${value//\\/\\\\}
  value=${value//\"/\\\"}
  printf '%s' "$value"
}

run_transport() {
  local service=$1
  if [ "$TRANSPORT" = "compose" ]; then
    local -a compose_command=(docker compose)
    if [ -n "${DPV2_COMPOSE_PROJECT:-}" ]; then
      compose_command+=(--project-name "$DPV2_COMPOSE_PROJECT")
    fi
    "${compose_command[@]}" exec -T "$service" curl --config - 2>/dev/null
  else
    curl --config - 2>/dev/null
  fi
}

request_status() {
  local service=$1
  local method=$2
  local url=$3
  local bearer=$4
  local org_header=$5
  local body=$6
  local escaped_body
  escaped_body=$(curl_escape "$body")

  {
    printf 'silent\n'
    printf 'output = "/dev/null"\n'
    printf 'write-out = "%%{http_code}"\n'
    printf 'connect-timeout = 3\n'
    printf 'max-time = 30\n'
    printf 'request = "%s"\n' "$method"
    printf 'url = "%s"\n' "$url"
    printf 'header = "Accept: application/json"\n'
    if [ -n "$bearer" ]; then
      printf 'header = "Authorization: Bearer %s"\n' "$bearer"
    fi
    if [ -n "$org_header" ]; then
      printf 'header = "X-Org-ID: %s"\n' "$org_header"
    fi
    if [ -n "$body" ]; then
      printf 'header = "Content-Type: application/json"\n'
      printf 'data = "%s"\n' "$escaped_body"
    fi
  } | run_transport "$service"
}

assert_status() {
  local family=$1
  local shape=$2
  local expected=$3
  local service=$4
  local method=$5
  local url=$6
  local bearer=$7
  local org_header=$8
  local body=$9
  local status

  status=$(request_status "$service" "$method" "$url" "$bearer" "$org_header" "$body" || true)
  case "$status" in
    [1-5][0-9][0-9]) ;;
    *) status=transport-error ;;
  esac

  if [ "$expected" = "403-or-404" ]; then
    if [ "$status" = "403" ] || [ "$status" = "404" ]; then
      pass "$family" "$shape" "$status"
    else
      fail "$family" "$shape" "$status"
    fi
  elif [ "$status" = "$expected" ]; then
    pass "$family" "$shape" "$status"
  else
    fail "$family" "$shape" "$status"
  fi
}

run_family() {
  local family=$1
  local service=$2
  local method=$3
  local own_url=$4
  local spoof_url=$5
  local bearer=$6
  local own_body=$7
  local spoof_body=$8

  assert_status "$family" "no credentials" 401 "$service" "$method" "$own_url" "" "" "$own_body"
  assert_status "$family" "forged org header" 401 "$service" "$method" "$spoof_url" "" "$DPV2_SPOOF_ORG_ID" "$spoof_body"
  # The valid-bearer shape deliberately omits X-Org-ID. Services must derive
  # tenant identity from verified claims rather than requiring a legacy header.
  assert_status "$family" "valid bearer" 200 "$service" "$method" "$own_url" "$bearer" "" "$own_body"
  assert_status "$family" "bearer plus spoofed org" 403-or-404 "$service" "$method" "$spoof_url" "$bearer" "$DPV2_SPOOF_ORG_ID" "$spoof_body"
}

printf 'Data Plane v2 isolated HTTP authorization matrix\n'
printf 'Transport: %s (response bodies suppressed)\n\n' "$TRANSPORT"

run_family \
  graph \
  graph-index \
  GET \
  "$GRAPH_BASE/v1/graphs/$DPV2_TEST_ORG_ID" \
  "$GRAPH_BASE/v1/graphs/$DPV2_SPOOF_ORG_ID" \
  "$DPV2_USER_BEARER" \
  "" \
  ""

run_family \
  quality \
  data-quality \
  GET \
  "$QUALITY_BASE/v1/cost/summary" \
  "$QUALITY_BASE/v1/cost/summary" \
  "$DPV2_QUALITY_BEARER" \
  "" \
  ""

run_family \
  orchestrator \
  data-orchestrator \
  GET \
  "$ORCHESTRATOR_BASE/v1/orchestrator/stale-embeddings" \
  "$ORCHESTRATOR_BASE/v1/orchestrator/stale-embeddings" \
  "$DPV2_USER_BEARER" \
  "" \
  ""

run_family \
  documents \
  documents-api \
  GET \
  "$DOCUMENTS_BASE/v1/documents/" \
  "$DOCUMENTS_BASE/v1/documents/" \
  "$DPV2_USER_BEARER" \
  "" \
  ""

RETRIEVAL_OWN=$(printf '{"org_id":"%s","query":"authorization regression probe","top_k":1,"zdr_mode":"ephemeral"}' "$DPV2_TEST_ORG_ID")
RETRIEVAL_SPOOF=$(printf '{"org_id":"%s","query":"authorization regression probe","top_k":1,"zdr_mode":"ephemeral"}' "$DPV2_SPOOF_ORG_ID")
run_family \
  retrieval \
  retrieval-engine \
  POST \
  "$RETRIEVAL_BASE/v1/retrieve" \
  "$RETRIEVAL_BASE/v1/retrieve" \
  "$DPV2_USER_BEARER" \
  "$RETRIEVAL_OWN" \
  "$RETRIEVAL_SPOOF"

run_family \
  wiki \
  wiki-store \
  GET \
  "$WIKI_BASE/v1/wiki/pages" \
  "$WIKI_BASE/v1/wiki/pages" \
  "$DPV2_USER_BEARER" \
  "" \
  ""

QUICKWIT_OWN=$(printf '{"org_id":"%s","dry_run":true,"clear":false,"global":false,"break_glass":false}' "$DPV2_TEST_ORG_ID")
QUICKWIT_SPOOF=$(printf '{"org_id":"%s","dry_run":true,"clear":false,"global":false,"break_glass":false}' "$DPV2_SPOOF_ORG_ID")
run_family \
  quickwit-admin-preview \
  quickwit-adapter \
  POST \
  "$QUICKWIT_BASE/admin/rebuild" \
  "$QUICKWIT_BASE/admin/rebuild" \
  "$DPV2_QUICKWIT_ADMIN_BEARER" \
  "$QUICKWIT_OWN" \
  "$QUICKWIT_SPOOF"

printf '\nResults: %s passed, %s failed\n' "$PASS" "$FAIL"
[ "$FAIL" -eq 0 ]
