#!/usr/bin/env bash
# Seed a VERIFIED test account for authenticated E2E journeys (proof-ladder L4).
#
# Local auth-core runs with REQUIRE_EMAIL_VERIFICATION=true but has no
# deliverable mailbox, so freshly signed-up accounts are stuck at
# EMAIL_NOT_VERIFIED. This script signs up through the real auth-core API
# (so password hashing follows Better Auth) and then flips email_verified
# directly in the auth_service database.
#
# Usage:
#   bash scripts/seed-verified-test-user.sh
#   E2E_EMAIL=me@verevon.dev E2E_PASSWORD='S3cret-pass' bash scripts/seed-verified-test-user.sh
set -euo pipefail

AUTH_CORE_URL="${AUTH_CORE_URL:-http://localhost:3011}"
E2E_EMAIL="${E2E_EMAIL:-e2e@verevon.dev}"
E2E_PASSWORD="${E2E_PASSWORD:-e2e-Verevon-Pass-123}"
E2E_NAME="${E2E_NAME:-Verevon E2E}"
PG_CONTAINER="${PG_CONTAINER:-controlplane-postgres}"
PG_USER="${PG_USER:-coresystem}"
PG_DATABASE="${PG_DATABASE:-auth_service}"

signup_body=$(mktemp)
trap 'rm -f "$signup_body"' EXIT

echo "1/3 Signing up ${E2E_EMAIL} via ${AUTH_CORE_URL} ..."
signup_status=$(curl -s -o "$signup_body" -w '%{http_code}' \
  -X POST "${AUTH_CORE_URL}/api/auth/sign-up/email" \
  -H 'content-type: application/json' \
  -d "{\"email\":\"${E2E_EMAIL}\",\"password\":\"${E2E_PASSWORD}\",\"name\":\"${E2E_NAME}\"}")

if [[ "$signup_status" == "200" || "$signup_status" == "201" ]]; then
  echo "    signup ok (${signup_status})"
elif grep -qi "exist" "$signup_body"; then
  echo "    account already exists — continuing"
else
  echo "    signup failed (${signup_status}): $(cat "$signup_body")" >&2
  exit 1
fi

echo "2/3 Marking ${E2E_EMAIL} as verified in ${PG_DATABASE} ..."
updated=$(docker exec "$PG_CONTAINER" psql -U "$PG_USER" -d "$PG_DATABASE" -tA \
  -c "UPDATE \"user\" SET email_verified = true WHERE email = '${E2E_EMAIL}' RETURNING id;")
if [[ -z "$updated" ]]; then
  echo "    no user row found for ${E2E_EMAIL} in ${PG_DATABASE}" >&2
  exit 1
fi
echo "    verified user id: ${updated}"

echo "3/3 Proving signin returns a session ..."
signin_status=$(curl -s -o "$signup_body" -w '%{http_code}' \
  -X POST "${AUTH_CORE_URL}/api/auth/sign-in/email" \
  -H 'content-type: application/json' \
  -d "{\"email\":\"${E2E_EMAIL}\",\"password\":\"${E2E_PASSWORD}\"}")
if [[ "$signin_status" != "200" ]]; then
  echo "    signin still failing (${signin_status}): $(cat "$signup_body")" >&2
  exit 1
fi
echo "    signin ok — verified E2E account ready: ${E2E_EMAIL}"
