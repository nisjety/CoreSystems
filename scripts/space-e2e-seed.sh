#!/usr/bin/env bash
# Space E2E seed (slice M0-T0, plan section 6.0 "T0 item: seed script").
#
# Seeds, on the LOCAL dev stack only:
#   org X  "Space E2E Org X"  : A owner; B, C, E members (Space roles editor/viewer/none are
#                               granted later through the Space membership routes)
#   org Y  "Space E2E Org Y"  : D owner (a different org)
#   org Z  "ZDR-TEST space e2e": F owner (zero-data-retention org, see step 5)
# and writes Playwright storage states E2E_STORAGE_STATE_A..F plus a secret-free
# space-e2e-seed.json under apps/Frontend Plane/verevonv3/tests/e2e/.auth/ (git-ignored).
#
# It extends, and does not replace:
#   apps/Control Plane/scripts/seed-verified-test-user.sh      (sign-up + verified e-mail)
#   apps/Control Plane/scripts/set-space-effect-policy.sh      (effect policy through the real API)
#   apps/Frontend Plane/verevonv3/apps/gateway/scripts/zdr-seed-test-org.mjs  (ZDR on a ZDR-TEST org)
#
# No AI model is selected, requested or entitled by this script, and the specs must not use
# Verevon-routed models (verevon, verevon-*, auto, default): see tests/e2e/space-fixtures.ts.
#
# Usage (from the repo root; nothing is sent without --apply):
#   bash scripts/space-e2e-seed.sh                 # prints the plan
#   bash scripts/space-e2e-seed.sh --apply [--ensure-room] [--skip-policy] [--skip-zdr]
#
# Environment (values are never printed):
#   E2E_BASE_URL                   same-origin SPA/gateway URL on loopback (default http://localhost:5173)
#   AUTH_CORE_URL                  default http://localhost:3011
#   SPACE_E2E_PASSWORD             shared dev password; if unset one is generated once and kept in
#                                  tests/e2e/.auth/space-e2e-password (git-ignored)
#   CONTROL_SPACE_POLICY_TOKEN     required for step 4 (effect policy); step skipped when unset
#   ORG_CORE_SERVICE_TOKEN         required for step 5 (ZDR on org Z); step skipped when unset
#   SPACE_E2E_POLICY_EXTRA_FLAGS   extra set-space-effect-policy.sh flags, e.g. the entitlement
#                                  bits a later workstream adds. The PUT replaces the whole policy,
#                                  so this is the single place to list every bit a milestone needs.
#   SPACE_E2E_PRIVACY_*            dev TEST values for the policy compliance fields (see below)
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
FV="$ROOT/apps/Frontend Plane/verevonv3"
CP="$ROOT/apps/Control Plane"
AUTH_DIR="$FV/tests/e2e/.auth"
# node and Playwright want native paths (Git Bash gives /c/...); other shells pass through unchanged.
native() { if command -v cygpath >/dev/null 2>&1; then cygpath -m "$1"; else printf '%s' "$1"; fi; }

apply=0; ensure_room=0; skip_policy=0; skip_zdr=0
for arg in "$@"; do
  case "$arg" in
    --apply) apply=1 ;;
    --ensure-room) ensure_room=1 ;;
    --skip-policy) skip_policy=1 ;;
    --skip-zdr) skip_zdr=1 ;;
    *) echo "unknown argument: $arg" >&2; exit 1 ;;
  esac
done

BASE_URL="${E2E_BASE_URL:-http://localhost:5173}"
AUTH_CORE_URL="${AUTH_CORE_URL:-http://localhost:3011}"
USER_CORE_URL="${USER_CORE_URL:-http://localhost:3021}"
ORG_CORE_URL="${ORG_CORE_URL:-http://127.0.0.1:18080}"

case "$BASE_URL" in http://localhost:*|http://127.0.0.1:*|http://\[::1\]:*|http://localhost|http://127.0.0.1) ;; *)
  echo "E2E_BASE_URL must be http on loopback (got ${BASE_URL}); the seed never talks to a remote host" >&2; exit 2 ;; esac
case "$AUTH_CORE_URL" in http://localhost:*|http://127.0.0.1:*) ;; *)
  echo "AUTH_CORE_URL must be http on loopback (got ${AUTH_CORE_URL})" >&2; exit 2 ;; esac
case "$USER_CORE_URL" in http://localhost:*|http://127.0.0.1:*) ;; *)
  echo "USER_CORE_URL must be http on loopback (got ${USER_CORE_URL})" >&2; exit 2 ;; esac

if [[ "$apply" != 1 ]]; then
  cat <<PLAN
plan only (pass --apply to send):
  1. sign up + verify six accounts space-e2e-{a..f}@verevon.dev through auth-core (${AUTH_CORE_URL})
  2. gateway (${BASE_URL}): org X (A owner; B, C, E members), org Y (D), org Z "ZDR-TEST space e2e" (F);
     writes storage states E2E_STORAGE_STATE_A..F and space-e2e-seed.json into ${AUTH_DIR}
  3. effect policy on org X (and org Z) through set-space-effect-policy.sh when CONTROL_SPACE_POLICY_TOKEN is set
  4. ZDR switch on org Z through zdr-seed-test-org.mjs when ORG_CORE_SERVICE_TOKEN is set
  5. write the environment contract file (paths only)
no model is selected or entitled by any step
PLAN
  exit 0
fi

mkdir -p "$AUTH_DIR"
chmod 700 "$AUTH_DIR" 2>/dev/null || true

# Shared dev password: from the environment, else generated once, kept only in the git-ignored file.
PASSWORD_FILE="$AUTH_DIR/space-e2e-password"
if [[ -n "${SPACE_E2E_PASSWORD:-}" ]]; then
  ( umask 077; printf '%s' "$SPACE_E2E_PASSWORD" > "$PASSWORD_FILE" )
elif [[ ! -s "$PASSWORD_FILE" ]]; then
  ( umask 077; node -e "process.stdout.write('Sp-'+require('node:crypto').randomBytes(18).toString('base64url')+'-9a')" > "$PASSWORD_FILE" )
fi
PASSWORD="$(cat "$PASSWORD_FILE")"

echo "1/5 Seeding six verified accounts"
for key in A B C D E F; do
  lower="$(printf '%s' "$key" | tr 'A-Z' 'a-z')"
  AUTH_CORE_URL="$AUTH_CORE_URL" E2E_EMAIL="space-e2e-${lower}@verevon.dev" E2E_PASSWORD="$PASSWORD" \
    E2E_NAME="Space E2E ${key}" bash "$CP/scripts/seed-verified-test-user.sh" >/dev/null
  echo "    user ${key} verified"
done

echo "2/5 Orgs, memberships and storage states through the gateway"
AUTH_NATIVE="$(native "$AUTH_DIR")"
session_args=(--apply --base-url "$BASE_URL" --password-file "$(native "$PASSWORD_FILE")" --out-dir "$AUTH_NATIVE")
[[ "$ensure_room" == 1 ]] && session_args+=(--ensure-room)
node "$(native "$ROOT")/scripts/space-e2e-seed-session.mjs" "${session_args[@]}"

SUMMARY="$AUTH_NATIVE/space-e2e-seed.json"
field() { node -e "const s=require(process.argv[1]);const p=process.argv[2].split('.');let c=s;for(const k of p)c=c?.[k];process.stdout.write(c==null?'':String(c))" "$SUMMARY" "$1"; }
ORG_X="$(field orgs.X.id)"; ORG_Z="$(field orgs.Z.id)"; USER_F="$(field users.F.user_id)"

echo "3/5 Effect policy (dev TEST values; every bit a milestone needs goes in SPACE_E2E_POLICY_EXTRA_FLAGS)"
if [[ "$skip_policy" == 1 ]]; then
  echo "    skipped (--skip-policy)"
elif [[ -z "${CONTROL_SPACE_POLICY_TOKEN:-}" ]]; then
  echo "    skipped: CONTROL_SPACE_POLICY_TOKEN is not set; shared Space features stay denied by design until it runs"
else
  # Compliance fields are test values for disposable dev orgs, not production defaults.
  # shellcheck disable=SC2206
  extra=(${SPACE_E2E_POLICY_EXTRA_FLAGS:-})
  base_flags=(--thread-create-entitled --retrieval-read-entitled --import-write-entitled
    --agent-action-entitled --schedule-fire-entitled --thread-read-entitled)
  for target in "X:$ORG_X:" "Z:$ORG_Z:--zero-data-retention"; do
    IFS=: read -r label org zdr_flag <<<"$target"
    [[ -n "$org" ]] || { echo "    org ${label} id missing from the seed summary" >&2; exit 1; }
    zdr_args=(); [[ -n "$zdr_flag" ]] && zdr_args=("$zdr_flag")
    USER_CORE_URL="$USER_CORE_URL" bash "$CP/scripts/set-space-effect-policy.sh" \
      --org-id "$org" \
      --privacy-policy-ref "privacy:${org}:1" \
      --purpose "${SPACE_E2E_PRIVACY_PURPOSE:-assistant_collaboration}" \
      --lawful-basis "${SPACE_E2E_PRIVACY_LAWFUL_BASIS:-contract}" \
      --privacy-class "${SPACE_E2E_PRIVACY_CLASS:-internal}" \
      --retention-class "${SPACE_E2E_PRIVACY_RETENTION_CLASS:-standard}" \
      --residency "${SPACE_E2E_PRIVACY_RESIDENCY:-swedencentral}" \
      --deletion-scope "${SPACE_E2E_PRIVACY_DELETION_SCOPE:-space}" \
      "${base_flags[@]}" "${extra[@]}" "${zdr_args[@]}" >/dev/null
    echo "    org ${label} effect policy set"
  done
fi

echo "4/5 ZDR switch on org Z"
if [[ "$skip_zdr" == 1 ]]; then
  echo "    skipped (--skip-zdr)"
elif [[ -z "${ORG_CORE_SERVICE_TOKEN:-}" || -z "$USER_F" ]]; then
  echo "    skipped: ORG_CORE_SERVICE_TOKEN (or F's user id) is missing. Run by hand, see docs/ZDR_OPERATOR_SEED_RUNBOOK.md:"
  echo "      node apps/gateway/scripts/zdr-seed-test-org.mjs --org-id ${ORG_Z} --user-id <F user id> --expect-name \"ZDR-TEST space e2e\" --apply"
else
  ( cd "$FV" && node apps/gateway/scripts/zdr-seed-test-org.mjs --base-url "$ORG_CORE_URL" \
      --org-id "$ORG_Z" --user-id "$USER_F" --expect-name "ZDR-TEST space e2e" --apply ) \
    || echo "    WARNING: the ZDR switch was refused (plan without ZDR, or no attested provider): org Z stays ZDR-off until fixed"
fi

echo "5/5 Environment contract (paths only, no secrets)"
{
  for key in A B C D E F; do echo "export E2E_STORAGE_STATE_${key}=\"${AUTH_NATIVE}/space-e2e-state-${key}.json\""; done
  echo "export E2E_SPACE_REF=\"$(field space_ref)\""
  echo "export E2E_REQUIRE_SPACE=1"
  echo "export E2E_BASE_URL=\"${BASE_URL}\""
} > "$AUTH_DIR/space-e2e.env.sh"
echo "    wrote ${AUTH_DIR}/space-e2e.env.sh (source it; set E2E_SPACE_REF by hand if it is empty)"
echo "    org X=${ORG_X} org Y=$(field orgs.Y.id) org Z=${ORG_Z}"
