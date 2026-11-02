#!/usr/bin/env bash
# SCIM deprovision propagation probe (slice WS05-S9, plan section WS05 D7).
#
# Provisions a test user through the SCIM edge, PATCHes active=false, and polls every
# consumer of the member-removed fact until each one reflects it or the 30 s budget runs
# out. Prints one table row per consumer (result, seconds from the PATCH) and exits 1 when
# any REQUIRED consumer misses the budget. Replays the PATCH afterwards and checks that
# nothing changes a second time.
#
# LOCAL / TEST STACK ONLY. It creates and removes a real membership in the org you name.
# Nothing is sent without --apply.
#
# Usage (from the repo root):
#   bash scripts/verify-deprovision.sh                  # prints the plan
#   bash scripts/verify-deprovision.sh --apply
#
# Environment (values are never printed):
#   SCIM_BASE_URL      SCIM root through the gateway edge (default http://localhost:5173/api/v1/scim/v2)
#   SCIM_TOKEN         org SCIM bearer token created in Settings > Identity (required)
#   SCIM_ORG_ID        the organisation the token belongs to (required; used for the SQL probes)
#   SCIM_USER_NAME     e-mail of the test user; its domain must be verified for the org
#                      (required; use an address you control, it is created if missing)
#   PROPAGATION_BUDGET seconds each consumer gets after the PATCH (default 30)
#   PG_CONTAINER, PG_USER   Postgres container and role (defaults controlplane-postgres, coresystem)
#   AUTH_DB, USER_DB, ORG_DB   database names (defaults auth_service, user_service, org_core)
#
# Optional consumers (each is the path of an executable that exits 0 once the consumer shows
# the user as removed; it receives ORG_ID and USER_ID as arguments; it is run directly, never
# through a shell string). A consumer without its hook is reported SKIPPED, never PASS:
#   PROBE_GATEWAY_CMD  signed-in session of the test user: expect 403 organization_membership_required
#   PROBE_CONVEX_CMD   Convex membership projection no longer lists the user
#   PROBE_SPACE_CMD    a Space decision for the user is refused (any effect-bound issuer route)
#   PROBE_SESSION_CORE_CMD  Session Core consumed space.membership_changed (revocation applied)
set -euo pipefail

apply=0
for arg in "$@"; do
  case "$arg" in
    --apply) apply=1 ;;
    *) echo "unknown argument: $arg" >&2; exit 1 ;;
  esac
done

SCIM_BASE_URL="${SCIM_BASE_URL:-http://localhost:5173/api/v1/scim/v2}"
BUDGET="${PROPAGATION_BUDGET:-30}"
PG_CONTAINER="${PG_CONTAINER:-controlplane-postgres}"
PG_USER="${PG_USER:-coresystem}"
AUTH_DB="${AUTH_DB:-auth_service}"
USER_DB="${USER_DB:-user_service}"
ORG_DB="${ORG_DB:-org_core}"

case "$SCIM_BASE_URL" in http://localhost:*|http://127.0.0.1:*|http://\[::1\]:*|http://localhost|http://127.0.0.1) ;; *)
  echo "refusing: SCIM_BASE_URL must be a loopback address (this probe removes a real membership)" >&2; exit 1 ;;
esac

if [[ "$apply" != 1 ]]; then
  cat <<'PLAN'
verify-deprovision plan (nothing is sent without --apply):
  1. GET  ServiceProviderConfig with the bearer token (edge, token and flag reachable)
  2. find or create the test user (POST /Users), wait for member row and active user-core membership
  3. t0: PATCH /Users/{id} replace active=false
  4. poll until the budget for each consumer:
       auth-core   member row gone, sessions active in this org gone       (required)
       org-core    organization_scim_users.active = false, deprovisioned_at (required)
       user-core   user_org_memberships.status = 'removed'                 (required)
       gateway / Convex / Space decision / Session Core                    (optional hooks)
  5. replay the PATCH: 200, and no row changes a second time
  6. print the propagation table; exit 1 when a required consumer missed the budget
PLAN
  exit 0
fi

: "${SCIM_TOKEN:?SCIM_TOKEN is required}"
: "${SCIM_ORG_ID:?SCIM_ORG_ID is required}"
: "${SCIM_USER_NAME:?SCIM_USER_NAME is required}"
[[ "$SCIM_ORG_ID" =~ ^[A-Za-z0-9_-]{1,128}$ ]] || { echo "SCIM_ORG_ID has an unexpected shape" >&2; exit 1; }
[[ "$SCIM_USER_NAME" =~ ^[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}$ ]] || { echo "SCIM_USER_NAME is not an e-mail" >&2; exit 1; }
[[ "$BUDGET" =~ ^[0-9]{1,3}$ ]] || { echo "PROPAGATION_BUDGET must be whole seconds" >&2; exit 1; }
for h in PROBE_GATEWAY_CMD PROBE_CONVEX_CMD PROBE_SPACE_CMD PROBE_SESSION_CORE_CMD; do
  if [[ -n "${!h:-}" && ! -x "${!h}" ]]; then echo "$h is not an executable file" >&2; exit 1; fi
done

tmp="$(mktemp)"; trap 'rm -f "$tmp"' EXIT

# scim METHOD PATH [JSON]  -> prints the HTTP status; body in $tmp. The token only travels in a header.
scim() {
  local method="$1" path="$2" body="${3:-}"
  local args=(-s -o "$tmp" -w '%{http_code}' -X "$method" "${SCIM_BASE_URL}${path}"
    -H "Authorization: Bearer ${SCIM_TOKEN}" -H 'Accept: application/scim+json')
  if [[ -n "$body" ]]; then args+=(-H 'Content-Type: application/scim+json' -d "$body"); fi
  curl "${args[@]}"
}

# sql DB QUERY(uses :'org' and :'uid' and :'email')  -> one trimmed value
sql() {
  local db="$1" q="$2"
  printf '%s\n' "$q" | docker exec -i "$PG_CONTAINER" psql -U "$PG_USER" -d "$db" -tA -v ON_ERROR_STOP=1 \
    -v org="$SCIM_ORG_ID" -v uid="${USER_ID:-}" -v email="$SCIM_USER_NAME" | tr -d '[:space:]'
}

json_get() { # json_get KEY  (first string value of KEY in $tmp; no jq dependency)
  sed -n "s/.*\"$1\" *: *\"\([^\"]*\)\".*/\1/p" "$tmp" | head -n1
}

echo "1/5 edge reachability"
st="$(scim GET /ServiceProviderConfig)"
[[ "$st" == "200" ]] || { echo "ServiceProviderConfig answered $st (flag off, edge off, or bad token)" >&2; exit 1; }

echo "2/5 test user"
enc_user="$(printf '%s' "$SCIM_USER_NAME" | sed 's/@/%40/g')"
st="$(scim GET "/Users?filter=userName%20eq%20%22${enc_user}%22")"
[[ "$st" == "200" ]] || { echo "list answered $st" >&2; exit 1; }
SCIM_ID="$(json_get id)"
if [[ -z "$SCIM_ID" ]]; then
  st="$(scim POST /Users "{\"schemas\":[\"urn:ietf:params:scim:schemas:core:2.0:User\"],\"userName\":\"${SCIM_USER_NAME}\",\"active\":true}")"
  [[ "$st" == "201" ]] || { echo "create answered $st: $(cat "$tmp")" >&2; exit 1; }
  SCIM_ID="$(json_get id)"
else
  st="$(scim PATCH "/Users/${SCIM_ID}" '{"schemas":["urn:ietf:params:scim:api:messages:2.0:PatchOp"],"Operations":[{"op":"replace","path":"active","value":true}]}')"
  [[ "$st" == "200" ]] || { echo "reactivate answered $st" >&2; exit 1; }
fi
[[ -n "$SCIM_ID" ]] || { echo "no scim id" >&2; exit 1; }

USER_ID=""
for _ in $(seq 1 20); do
  USER_ID="$(sql "$AUTH_DB" "SELECT id FROM \"user\" WHERE lower(email)=lower(:'email');")"
  if [[ -n "$USER_ID" ]]; then
    m="$(sql "$AUTH_DB" "SELECT count(*) FROM member WHERE organization_id=:'org' AND user_id=:'uid';")"
    [[ "$m" == "1" ]] && break
  fi
  sleep 1
done
[[ -n "$USER_ID" && "${m:-0}" == "1" ]] || { echo "member row did not appear in auth-core within 20 s" >&2; exit 1; }
for _ in $(seq 1 20); do
  s="$(sql "$USER_DB" "SELECT status FROM user_org_memberships WHERE org_id=:'org' AND user_id=:'uid';")"
  [[ "$s" == "active" ]] && break
  sleep 1
done
[[ "${s:-}" == "active" ]] || { echo "user-core membership is '${s:-absent}', not active; the precondition for the probe is not met" >&2; exit 1; }

now() { date +%s.%N; }
elapsed() { awk -v a="$1" -v b="$2" 'BEGIN{printf "%.1f", b-a}'; }

declare -A RESULT SECONDS_AT REQUIRED
names=(auth-core org-core user-core gateway convex space-decision session-core)
for n in auth-core org-core user-core; do REQUIRED[$n]=1; done
for n in gateway convex space-decision session-core; do REQUIRED[$n]=0; done
declare -A HOOK=([gateway]="${PROBE_GATEWAY_CMD:-}" [convex]="${PROBE_CONVEX_CMD:-}" [space-decision]="${PROBE_SPACE_CMD:-}" [session-core]="${PROBE_SESSION_CORE_CMD:-}")

check() { # check NAME -> 0 when the consumer shows the removal
  case "$1" in
    auth-core)
      local m ses
      m="$(sql "$AUTH_DB" "SELECT count(*) FROM member WHERE organization_id=:'org' AND user_id=:'uid';")"
      ses="$(sql "$AUTH_DB" "SELECT count(*) FROM session WHERE user_id=:'uid' AND active_organization_id=:'org';")"
      [[ "$m" == "0" && "$ses" == "0" ]] ;;
    org-core)
      local a d
      a="$(sql "$ORG_DB" "SELECT active FROM organization_scim_users WHERE org_id=:'org' AND lower(user_name)=lower(:'email');")"
      d="$(sql "$ORG_DB" "SELECT (deprovisioned_at IS NOT NULL) FROM organization_scim_users WHERE org_id=:'org' AND lower(user_name)=lower(:'email');")"
      [[ "$a" == "f" && "$d" == "t" ]] ;;
    user-core)
      [[ "$(sql "$USER_DB" "SELECT status FROM user_org_memberships WHERE org_id=:'org' AND user_id=:'uid';")" == "removed" ]] ;;
    *)
      "${HOOK[$1]}" "$SCIM_ORG_ID" "$USER_ID" >/dev/null 2>&1 ;;
  esac
}

for n in "${names[@]}"; do
  if [[ -z "${HOOK[$n]:-}" && "${REQUIRED[$n]}" == 0 ]]; then RESULT[$n]="SKIPPED"; SECONDS_AT[$n]="-"; else RESULT[$n]="PENDING"; fi
done

echo "3/5 PATCH active=false"
t0="$(now)"
st="$(scim PATCH "/Users/${SCIM_ID}" '{"schemas":["urn:ietf:params:scim:api:messages:2.0:PatchOp"],"Operations":[{"op":"replace","path":"active","value":false}]}')"
[[ "$st" == "200" ]] || { echo "deprovision PATCH answered $st: $(cat "$tmp")" >&2; exit 1; }
SECONDS_AT[patch]="$(elapsed "$t0" "$(now)")"

echo "4/5 polling consumers (budget ${BUDGET}s each)"
while :; do
  pending=0
  for n in "${names[@]}"; do
    [[ "${RESULT[$n]}" == "PENDING" ]] || continue
    if check "$n"; then
      RESULT[$n]="PASS"; SECONDS_AT[$n]="$(elapsed "$t0" "$(now)")"
    else
      pending=1
    fi
  done
  el="$(elapsed "$t0" "$(now)")"
  if [[ "$pending" == 0 ]]; then break; fi
  if awk -v e="$el" -v b="$BUDGET" 'BEGIN{exit !(e>=b)}'; then break; fi
  sleep 1
done
for n in "${names[@]}"; do
  if [[ "${RESULT[$n]}" == "PENDING" ]]; then RESULT[$n]="MISSED"; SECONDS_AT[$n]=">${BUDGET}"; fi
done

echo "5/5 replay"
before="$(sql "$ORG_DB" "SELECT revision FROM organization_scim_users WHERE org_id=:'org' AND lower(user_name)=lower(:'email');")"
st="$(scim PATCH "/Users/${SCIM_ID}" '{"schemas":["urn:ietf:params:scim:api:messages:2.0:PatchOp"],"Operations":[{"op":"replace","path":"active","value":false}]}')"
after="$(sql "$ORG_DB" "SELECT revision FROM organization_scim_users WHERE org_id=:'org' AND lower(user_name)=lower(:'email');")"
if [[ "$st" == "200" && "$before" == "$after" ]]; then REPLAY="PASS"; else REPLAY="FAIL (status $st, revision $before -> $after)"; fi

echo
printf '%-16s %-9s %-9s %s\n' consumer result seconds required
fail=0
printf '%-16s %-9s %-9s %s\n' "scim-patch" "OK" "${SECONDS_AT[patch]}" "-"
for n in "${names[@]}"; do
  req="no"; [[ "${REQUIRED[$n]}" == 1 ]] && req="yes"
  printf '%-16s %-9s %-9s %s\n' "$n" "${RESULT[$n]}" "${SECONDS_AT[$n]}" "$req"
  if [[ "${REQUIRED[$n]}" == 1 && "${RESULT[$n]}" != "PASS" ]]; then fail=1; fi
  if [[ "${RESULT[$n]}" == "MISSED" ]]; then fail=1; fi
done
printf '%-16s %s\n' "replay" "$REPLAY"
[[ "$REPLAY" == "PASS" ]] || fail=1
echo
echo "Bound, not a guarantee: Space decisions issued before t0 stay valid up to their 2-minute expiry; the table measures propagation, not that window."
echo "The test user (scim id ${SCIM_ID}) stays deprovisioned; DELETE it through your IdP or the SCIM API to clean up."
exit "$fail"
