#!/usr/bin/env bash
# QB-T03 live proof: the retrieval engine admits a SERVICE principal by its
# default-deny route allowlist instead of the user membership lookup that 403'd
# every quarry-edge POST /v1/retrieve (live 2026-10-01).
#
# Run AFTER the retrieval-engine image with QB-T03 is deployed (stage S5). It:
#   1. mints two real service tokens for quarry-edge from Auth Core,
#   2. drives the admission, default-deny, scope and tenant-header cases against
#      the live engine, and
#   3. checks the counters, the access_audit_log rows and the facade logs.
#
# Cost: no model or inference call and no paid provider. The three retrieval
# probes carry zdr_mode "ephemeral", which restricts egress (no Cohere embed, no
# persisted trace, no rerank). Calls made: 2 Auth Core token mints, 3 retrieval
# requests, 6 refused requests, 2 /metrics reads, 3 read-only SQL queries.
#
# Secrets: the quarry-edge credential is read from the quarry-edge container's
# environment into a shell variable and forwarded to `docker exec` by NAME only
# (never on a host command line, never echoed). Tokens are held the same way.
# Nothing below prints either. `set +x` is deliberate; do not enable tracing
# (`bash -x` writes the credential to the trace). Be aware that inside the engine
# container the credential and the tokens sit on `curl`'s argument list for the
# length of each call, visible to anything that can list that container's
# processes; run it only where that container is operated by you.
#
# This script is LIVE: it authenticates to Auth Core with the quarry-edge
# credential and mints real service tokens. It therefore refuses to run unless
# CONFIRM_LIVE=1 is set on purpose.
#
# Usage:
#   CONFIRM_LIVE=1 ORG_ID=<an org id quarry-edge may read> \
#     "apps/Data Plane v2/tests/scripts/service-principal-admission-live-proof.sh"
#
# Overridable: ENGINE_CONTAINER, QUARRY_CONTAINER, PG_CONTAINER, PG_USER,
# AUTH_CORE_URL (as seen from the engine container), ENGINE_URL (inside it).
set -euo pipefail
set +x

# Git Bash on Windows rewrites values that look like POSIX paths (for example
# /v1/retrieve) in the arguments AND environment passed to docker.exe, which
# turns the request URL into a Windows path and makes curl exit 3. Harmless on
# Linux.
export MSYS_NO_PATHCONV=1 MSYS2_ARG_CONV_EXCL='*' MSYS2_ENV_CONV_EXCL='*'

if [ "${CONFIRM_LIVE:-}" != "1" ]; then
  echo "refusing to run: this mints live service tokens. Re-run with CONFIRM_LIVE=1." >&2
  exit 2
fi
ORG_ID=${ORG_ID:-${1:-}}
if [ -z "$ORG_ID" ]; then
  echo "usage: CONFIRM_LIVE=1 ORG_ID=<org id> $0" >&2
  exit 2
fi
# ORG_ID is spliced into the mint's JSON body, the probe bodies and three SQL
# statements below, so only a plain identifier is accepted.
case "$ORG_ID" in
  *[!A-Za-z0-9_-]*)
    echo "ORG_ID may contain only letters, digits, '_' and '-'" >&2
    exit 2
    ;;
esac
ENGINE=${ENGINE_CONTAINER:-data-plane-v2-retrieval-engine-1}
QUARRY=${QUARRY_CONTAINER:-quarry-edge}
PG=${PG_CONTAINER:-data-plane-v2-postgres-1}
PG_USER=${PG_USER:-dataplane}
AUTH_URL=${AUTH_CORE_URL:-http://auth-core:3011}
ENGINE_URL=${ENGINE_URL:-http://127.0.0.1:8004}
SERVICE_ID='service:quarry-edge'
export ORG_ID AUTH_URL ENGINE_URL

failures=0
pass() { printf 'PASS  %s\n' "$1"; }
fail() {
  printf 'FAIL  %s\n' "$1"
  failures=$((failures + 1))
}
info() { printf 'INFO  %s\n' "$1"; }

# ---------------------------------------------------------------- helpers

# One call from inside the engine container (it has curl and the network).
# Prints "<http status> <error code or ->". The body is never printed.
probe() {
  METHOD=$1 URLPATH=$2 TOKEN=$3 BODY=${4-} XORG=${5-}
  export METHOD URLPATH TOKEN BODY XORG
  docker exec -i -e METHOD -e URLPATH -e TOKEN -e BODY -e XORG -e ENGINE_URL "$ENGINE" sh -s <<'EOS'
set -eu
set --
if [ -n "${XORG:-}" ]; then set -- -H "x-org-id: $XORG"; fi
if [ -n "${BODY:-}" ]; then set -- "$@" -d "$BODY"; fi
code=$(curl -s -m 30 -o /tmp/qbt03.body -w '%{http_code}' -X "$METHOD" "$ENGINE_URL$URLPATH" \
  -H "authorization: Bearer $TOKEN" -H 'content-type: application/json' "$@")
err=$(sed -n 's/.*"error":"\([^"]*\)".*/\1/p' /tmp/qbt03.body | head -n 1)
rm -f /tmp/qbt03.body
printf '%s %s\n' "$code" "${err:--}"
EOS
}

# Reads Auth Core's token response on stdin and prints only the token. Exits 3
# (printing nothing) on anything else, so an error body is never echoed.
extract_token() {
  node -e 'let s="";process.stdin.on("data",(d)=>(s+=d)).on("end",()=>{try{const t=JSON.parse(s).token;if(typeof t!=="string"||!t)process.exit(3);process.stdout.write(t)}catch(e){process.exit(3)}})'
}

# A real service token for quarry-edge from Auth Core (the real mint, the real
# registry row). $1 is a JSON array of scopes. Prints the token; nothing else.
mint() {
  SCOPES=$1
  export SCOPES
  docker exec -i -e QKEY -e SCOPES -e ORG_ID -e AUTH_URL "$ENGINE" sh -s <<'EOS' | extract_token
set -eu
curl -s -m 15 -X POST "$AUTH_URL/api/data-plane/internal-token" \
  -H 'x-service-id: quarry-edge' -H "x-service-api-key: $QKEY" \
  -H 'content-type: application/json' \
  -d "{\"orgId\":\"$ORG_ID\",\"scopes\":$SCOPES,\"reason\":\"retrieve verified organization evidence\"}"
EOS
}

metrics() { docker exec "$ENGINE" curl -s -m 10 "$ENGINE_URL/metrics"; }

# Sum of a counter's samples whose line contains every extra fragment given.
metric_sum() {
  local name=$1
  shift
  local text
  text=$(metrics)
  printf '%s\n' "$text" | awk -v name="$name" -v frags="$*" '
    BEGIN { n = split(frags, f, " ") }
    index($0, name) == 1 {
      ok = 1
      for (i = 1; i <= n; i++) if (f[i] != "" && index($0, f[i]) == 0) ok = 0
      if (ok) s += $NF
    }
    END { printf "%d\n", s + 0 }'
}

sql() { docker exec "$PG" psql -U "$PG_USER" -d dataplane -At -F ' | ' -c "$1"; }

# ---------------------------------------------------------------- start state

command -v node >/dev/null || { echo "node is required on the host" >&2; exit 2; }
docker exec "$ENGINE" true >/dev/null
QKEY=$(docker exec "$QUARRY" printenv QUARRY_SERVICE_API_KEY)
[ -n "$QKEY" ] || { echo "quarry-edge credential is not set in $QUARRY" >&2; exit 2; }
export QKEY

START_TS=$(sql "select clock_timestamp()")
START_ISO=$(date -u +%Y-%m-%dT%H:%M:%SZ)
NO_MEMBERSHIP_BEFORE=$(metric_sum dpv2_authz_denials_total 'cause="denied:no_membership"')
info "org=$ORG_ID window starts $START_TS (db clock); denied:no_membership counter before = $NO_MEMBERSHIP_BEFORE"

READ_TOKEN=$(mint '["data:read"]') || { echo "could not mint a data:read service token" >&2; exit 2; }
WRITE_TOKEN=$(mint '["documents:write"]') || { echo "could not mint a documents:write service token" >&2; exit 2; }
info "minted 2 service tokens (not printed)"

BODY=$(printf '{"org_id":"%s","query":"qb-t03 admission probe","zdr_mode":"ephemeral"}' "$ORG_ID")

# ---------------------------------------------------------------- 1. admission

for route in /v1/retrieve /v1/retrieve/hybrid /v1/knowledge/search; do
  out=$(probe POST "$route" "$READ_TOKEN" "$BODY")
  code=${out%% *}
  case "$code" in
    200) pass "POST $route admitted for $SERVICE_ID with data:read (200)" ;;
    401 | 403) fail "POST $route refused ($out): the defect is NOT fixed" ;;
    *) pass "POST $route admitted (guard passed; downstream answered $out)" ;;
  esac
done

# ---------------------------------------------------------------- 2. default deny

expect() { # label expected-output actual-output
  if [ "$3" = "$2" ]; then pass "$1 -> $3"; else fail "$1 -> got '$3', expected '$2'"; fi
}

expect "POST /v1/admin/cleanup/orphans is not a service route" \
  "403 service_principal_route_not_allowed" \
  "$(probe POST /v1/admin/cleanup/orphans "$READ_TOKEN" '{}')"
expect "GET /v1/retrieval/{trace_id} is not a service route" \
  "403 service_principal_route_not_allowed" \
  "$(probe GET /v1/retrieval/qbt03-probe "$READ_TOKEN")"
expect "POST /v1/context/pins is not a service route" \
  "403 service_principal_route_not_allowed" \
  "$(probe POST /v1/context/pins "$READ_TOKEN" '{}')"
expect "POST /v1/cache/semantic/store is not a service route" \
  "403 service_principal_route_not_allowed" \
  "$(probe POST /v1/cache/semantic/store "$READ_TOKEN" '{}')"

# ---------------------------------------------------------------- 3. scope, tenant

expect "a token without data:read is refused on /v1/retrieve" \
  "403 service_principal_scope_missing" \
  "$(probe POST /v1/retrieve "$WRITE_TOKEN" "$BODY")"
expect "x-org-id that differs from the token's org is refused" \
  "403 -" \
  "$(probe POST /v1/retrieve "$READ_TOKEN" "$BODY" org-qbt03-mismatch)"

# Informational: /v1/rerank is registered by QB-T05a. Before it ships the route
# does not exist (404, the guard never runs); once shipped a data:read-only
# token must get 403 service_principal_scope_missing.
info "POST /v1/rerank with a data:read-only token -> $(probe POST /v1/rerank "$READ_TOKEN" '{}') (404 before QB-T05a; 403 service_principal_scope_missing after)"

# ---------------------------------------------------------------- 4. counters

NO_MEMBERSHIP_AFTER=$(metric_sum dpv2_authz_denials_total 'cause="denied:no_membership"')
if [ "$NO_MEMBERSHIP_AFTER" -eq "$NO_MEMBERSHIP_BEFORE" ]; then
  pass "dpv2_authz_denials_total{cause=denied:no_membership} did not move ($NO_MEMBERSHIP_BEFORE -> $NO_MEMBERSHIP_AFTER)"
else
  fail "dpv2_authz_denials_total{cause=denied:no_membership} moved ($NO_MEMBERSHIP_BEFORE -> $NO_MEMBERSHIP_AFTER)"
fi
ADMITTED=$(metric_sum dpv2_service_principal_requests_total "service_id=\"$SERVICE_ID\"" 'outcome="admitted"')
DENIED_ROUTE=$(metric_sum dpv2_service_principal_requests_total "service_id=\"$SERVICE_ID\"" 'outcome="denied:service_route_not_allowed"')
DENIED_SCOPE=$(metric_sum dpv2_service_principal_requests_total "service_id=\"$SERVICE_ID\"" 'outcome="denied:service_scope_missing"')
[ "$ADMITTED" -ge 3 ] && pass "service admitted counter >= 3 ($ADMITTED)" || fail "service admitted counter is $ADMITTED, expected >= 3"
[ "$DENIED_ROUTE" -ge 4 ] && pass "service route-denied counter >= 4 ($DENIED_ROUTE)" || fail "service route-denied counter is $DENIED_ROUTE, expected >= 4"
[ "$DENIED_SCOPE" -ge 1 ] && pass "service scope-denied counter >= 1 ($DENIED_SCOPE)" || fail "service scope-denied counter is $DENIED_SCOPE, expected >= 1"

# ---------------------------------------------------------------- 5. audit rows

ROWS=$(sql "select endpoint, http_status, cause, count(*) from access_audit_log where user_id = '$SERVICE_ID' and org_id = '$ORG_ID' and created_at >= '$START_TS' group by 1,2,3 order by 1,2,3")
printf '%s\n' "$ROWS" | sed 's/^/ROW   /'
NEW_NO_MEMBERSHIP=$(sql "select count(*) from access_audit_log where user_id = '$SERVICE_ID' and cause = 'denied:no_membership' and created_at >= '$START_TS'")
[ "$NEW_NO_MEMBERSHIP" = "0" ] && pass "no new denied:no_membership audit row for $SERVICE_ID" || fail "$NEW_NO_MEMBERSHIP new denied:no_membership audit rows for $SERVICE_ID"
for needle in \
  'POST /v1/admin/cleanup/orphans | 403 | denied:service_route_not_allowed' \
  'POST /v1/retrieve | 403 | denied:service_scope_missing' \
  'POST /v1/retrieve | 403 | denied:header_claims_mismatch'; do
  if printf '%s\n' "$ROWS" | grep -qF "$needle"; then pass "audit row present: $needle"; else fail "audit row missing: $needle"; fi
done
ADMITTED_ROWS=$(sql "select count(*) from access_audit_log where user_id = '$SERVICE_ID' and org_id = '$ORG_ID' and created_at >= '$START_TS' and http_status < 400")
[ "$ADMITTED_ROWS" -ge 1 ] && pass "audit rows for admitted service calls ($ADMITTED_ROWS), user_id=$SERVICE_ID" || fail "no audit row for an admitted service call"

# ---------------------------------------------------------------- 6. no facade call

FACADE_LINES=$(docker logs --since "$START_ISO" "$ENGINE" 2>&1 | grep -c 'user-core authz facade' || true)
if [ "$FACADE_LINES" = "0" ]; then
  pass "no user-core visibility facade call in the engine log during the window"
else
  info "$FACADE_LINES facade log lines in the window; on a quiet engine expect 0 (a user request in the same window also produces them)"
fi

echo
if [ "$failures" -eq 0 ]; then
  echo "QB-T03 live proof: PASS"
else
  echo "QB-T03 live proof: $failures FAILURE(S)"
  exit 1
fi
