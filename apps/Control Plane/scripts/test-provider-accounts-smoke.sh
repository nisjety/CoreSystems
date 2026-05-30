#!/usr/bin/env bash

set -euo pipefail

GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

AUTH_URL="${AUTH_URL:-http://localhost:3011}"
AUTH_CONTAINER="${AUTH_CONTAINER:-auth-service}"
POSTGRES_CONTAINER="${POSTGRES_CONTAINER:-controlplane-postgres}"
POSTGRES_DB="${POSTGRES_DB:-user_service}"
POSTGRES_USER="${POSTGRES_USER:-aquatiq}"
PROVIDER="${PROVIDER:-microsoft}"

if ! command -v curl >/dev/null 2>&1; then
  echo -e "${RED}✗ curl is required${NC}"
  exit 1
fi

if ! command -v jq >/dev/null 2>&1; then
  echo -e "${RED}✗ jq is required${NC}"
  exit 1
fi

echo -e "${BLUE}===============================================${NC}"
echo -e "${BLUE}Provider Accounts Smoke Test${NC}"
echo -e "${BLUE}Validates provider_accounts upsert + id storage${NC}"
echo -e "${BLUE}===============================================${NC}"

TS="$(date +%s)"
EMAIL="smoke.provider.${TS}@example.com"
PASSWORD="SmokePass123!"
NAME="Provider Smoke ${TS}"
PROVIDER_ACCOUNT_ID="acct-smoke-${TS}"
TARGET_EMAIL=""
TARGET_USER_ID=""

echo -e "${YELLOW}1) Creating test user via auth signUp${NC}"
SIGNUP_RESPONSE="$(curl -sS -X POST "${AUTH_URL}/api/v2/auth/signUp" \
  -H "Content-Type: application/json" \
  -d "{\"email\":\"${EMAIL}\",\"password\":\"${PASSWORD}\",\"name\":\"${NAME}\"}")"

USER_ID="$(echo "${SIGNUP_RESPONSE}" | jq -r '.user.id // empty')"
if [[ -z "${USER_ID}" ]]; then
  echo -e "${RED}✗ Failed to create test user${NC}"
  echo "${SIGNUP_RESPONSE}"
  exit 1
fi

echo -e "${GREEN}✓ User created${NC}: ${EMAIL} (${USER_ID})"
TARGET_EMAIL="${EMAIL}"
TARGET_USER_ID="${USER_ID}"

echo -e "${YELLOW}1b) Waiting for user-core sync (users table)${NC}"
USER_SYNCED="false"
for _ in {1..20}; do
  USER_COUNT="$(docker exec "${POSTGRES_CONTAINER}" psql -U "${POSTGRES_USER}" -d "${POSTGRES_DB}" -t -A -c \
    "SELECT COUNT(*) FROM users WHERE email='${EMAIL}';" | tr -d '[:space:]')"
  if [[ "${USER_COUNT}" == "1" ]]; then
    USER_SYNCED="true"
    break
  fi
  sleep 1
done

if [[ "${USER_SYNCED}" != "true" ]]; then
   echo -e "${YELLOW}⚠ Signup user not synced in time; falling back to an existing user-core user${NC}"
  FALLBACK_ROW="$(docker exec "${POSTGRES_CONTAINER}" psql -U "${POSTGRES_USER}" -d "${POSTGRES_DB}" -t -A -F '|' -c "SELECT id, email FROM users WHERE email IS NOT NULL ORDER BY created_at ASC LIMIT 1;")"
  if [[ -z "${FALLBACK_ROW}" ]]; then
    echo -e "${RED}✗ No existing user found in user-core for fallback${NC}"
    exit 1
  fi
  TARGET_USER_ID="$(echo "${FALLBACK_ROW}" | cut -d'|' -f1)"
  TARGET_EMAIL="$(echo "${FALLBACK_ROW}" | cut -d'|' -f2)"
  echo -e "${YELLOW}Using fallback user${NC}: ${TARGET_EMAIL} (${TARGET_USER_ID})"
else
  echo -e "${GREEN}✓ User synced in user-core${NC}"
fi

echo -e "${YELLOW}2) Publishing auth.user.provider_linked event${NC}"
docker exec "${AUTH_CONTAINER}" node -e '
const { connect, StringCodec } = require("nats");
(async () => {
  const email = process.argv[1];
  const userId = process.argv[2];
  const provider = process.argv[3];
  const providerAccountId = process.argv[4];

  const nc = await connect({
    servers: process.env.NATS_URL || "nats://controlplane-nats:4222",
    token: process.env.NATS_TOKEN || "nats",
  });

  const sc = StringCodec();
  const evt = {
    type: "auth.user.provider_linked",
    userId,
    email,
    provider,
    providerAccountId,
    tenantId: "tenant-smoke",
    microsoftTenantId: "tenant-smoke",
    emailFromProvider: email,
    scopesGranted: ["openid", "profile", "email"],
    tokenRef: providerAccountId,
    profileHints: {
      displayName: "Provider Smoke",
      locale: "nb-NO",
      timezone: "Europe/Oslo",
    },
    timestamp: new Date().toISOString(),
  };

  nc.publish("auth.user.provider_linked", sc.encode(JSON.stringify(evt)));
  await nc.flush();
  await nc.close();
  console.log("published");
})().catch((err) => {
  console.error(err.message);
  process.exit(1);
});
' "${TARGET_EMAIL}" "${TARGET_USER_ID}" "${PROVIDER}" "${PROVIDER_ACCOUNT_ID}"

sleep 2

echo -e "${YELLOW}3) Asserting provider_accounts row exists${NC}"
ROW="$(docker exec "${POSTGRES_CONTAINER}" psql -U "${POSTGRES_USER}" -d "${POSTGRES_DB}" -t -A -F '|' -c \
  "SELECT id, provider, provider_user_id, email, token_ref FROM provider_accounts WHERE provider='${PROVIDER}' AND provider_user_id='${PROVIDER_ACCOUNT_ID}' ORDER BY updated_at DESC LIMIT 1;")"

if [[ -z "${ROW}" ]]; then
  echo -e "${RED}✗ No row found in provider_accounts for provider_user_id=${PROVIDER_ACCOUNT_ID}${NC}"
  exit 1
fi

ID="$(echo "${ROW}" | cut -d'|' -f1)"
ROW_PROVIDER="$(echo "${ROW}" | cut -d'|' -f2)"
ROW_PROVIDER_UID="$(echo "${ROW}" | cut -d'|' -f3)"
ROW_EMAIL="$(echo "${ROW}" | cut -d'|' -f4)"
ROW_TOKEN_REF="$(echo "${ROW}" | cut -d'|' -f5)"

if [[ -z "${ID}" || "${ID}" == "NULL" ]]; then
  echo -e "${RED}✗ provider_accounts.id is null/empty (regression)${NC}"
  echo "Row: ${ROW}"
  exit 1
fi

if [[ "${ROW_PROVIDER_UID}" != "${PROVIDER_ACCOUNT_ID}" ]]; then
  echo -e "${RED}✗ provider_user_id mismatch${NC}"
  echo "Expected: ${PROVIDER_ACCOUNT_ID}"
  echo "Actual:   ${ROW_PROVIDER_UID}"
  exit 1
fi

echo -e "${GREEN}✓ provider_accounts insert/upsert verified${NC}"
echo "  id=${ID}"
echo "  provider=${ROW_PROVIDER}"
echo "  provider_user_id=${ROW_PROVIDER_UID}"
echo "  email=${ROW_EMAIL}"
echo "  token_ref=${ROW_TOKEN_REF}"

echo -e "${YELLOW}4) Checking recent user-service logs for upsert failures${NC}"
FAIL_LOGS="$(docker logs user-service --tail 200 2>&1 | grep -E 'Failed to upsert provider account on link|null value in column "id" of relation "provider_accounts"' || true)"
if [[ -n "${FAIL_LOGS}" ]]; then
  echo -e "${RED}✗ Found provider upsert failure markers in recent logs${NC}"
  echo "${FAIL_LOGS}"
  exit 1
fi

echo -e "${GREEN}✓ No provider upsert failure markers in recent logs${NC}"
echo -e "${GREEN}Smoke test passed${NC}"
