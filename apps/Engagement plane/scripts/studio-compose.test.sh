#!/bin/sh
set -eu

studio_test_dir=$(mktemp -d)
trap 'rm -r "$studio_test_dir"' EXIT HUP INT TERM
studio_test_root=$(CDPATH= cd "$(dirname "$0")/.." && pwd)
studio_fixture="$studio_test_dir/project"
mkdir -p "$studio_fixture/scripts" "$studio_fixture/social-core" "$studio_fixture/leads-core" "$studio_fixture/visibility-core"
cp "$studio_test_root/scripts/engagement-compose.sh" "$studio_fixture/scripts/engagement-compose.sh"
printf '%s\n' 'INTERNAL_API_KEY=social-test-key' 'STUDIO_SOCIAL_NATS_PASSWORD=social-test-pass' > "$studio_fixture/social-core/.env"
printf '%s\n' 'INTERNAL_API_KEY=leads-test-key' 'STUDIO_LEADS_NATS_PASSWORD=leads-test-pass' > "$studio_fixture/leads-core/.env"
printf '%s\n' 'INTERNAL_API_KEY=marketing-test-key' > "$studio_fixture/visibility-core/.env"
ln -s "$studio_test_root/scripts/test-fixtures/mock-docker.sh" "$studio_test_dir/docker"
export STUDIO_DOCKER_TEST_LOG="$studio_test_dir/docker.log"
export STUDIO_DOCKER_TEST_NETWORK="$studio_test_dir/network"
PATH="$studio_test_dir:$PATH"
export PATH
unset INTERNAL_API_KEY APPLICATION_SOCIAL_NATS_PASSWORD APPLICATION_LEADS_NATS_PASSWORD

"$studio_fixture/scripts/engagement-compose.sh" config --quiet
if grep -q -- '--env-file ./anbud-core/.env' "$STUDIO_DOCKER_TEST_LOG"; then
  echo "Anbud's env file must not enter the base Studio stack" >&2
  exit 1
fi
"$studio_fixture/scripts/engagement-compose.sh" build
if grep -q '^network ' "$STUDIO_DOCKER_TEST_LOG"; then
  echo "config/build must not change Docker networks" >&2
  exit 1
fi

printf '%s' synthetic-approved-bearer > "$studio_test_dir/approved-bearer"
chmod 600 "$studio_test_dir/approved-bearer"
if STUDIO_SOCIAL_INTEGRATION_BEARER_FILE="$studio_test_dir/approved-bearer" \
  "$studio_fixture/scripts/engagement-compose.sh" config --quiet 2>/dev/null; then
  echo "partial Social tenant authorization was accepted" >&2
  exit 1
fi
STUDIO_SOCIAL_INTEGRATION_BEARER_FILE="$studio_test_dir/approved-bearer" \
  STUDIO_SOCIAL_INTEGRATION_ORGANIZATION_ID=synthetic-org \
  STUDIO_LEADS_INTEGRATION_BEARER_FILE="$studio_test_dir/approved-bearer" \
  STUDIO_LEADS_INTEGRATION_ORGANIZATION_ID=synthetic-org \
  STUDIO_MARKETING_INTEGRATION_BEARER_FILE="$studio_test_dir/approved-bearer" \
  STUDIO_MARKETING_GOOGLE_CONNECTION_ID=synthetic-google-connection \
  "$studio_fixture/scripts/engagement-compose.sh" config --quiet
for overlay in social-integrations leads-integrations marketing-gsc; do
  if ! grep -q -- "-f ./docker-compose.$overlay.studio.yml" "$STUDIO_DOCKER_TEST_LOG"; then
    echo "missing authorized integration overlay: $overlay" >&2
    exit 1
  fi
done
if grep -q synthetic-approved-bearer "$STUDIO_DOCKER_TEST_LOG"; then
  echo "bearer content entered Compose command arguments" >&2
  exit 1
fi
chmod 644 "$studio_test_dir/approved-bearer"
if STUDIO_SOCIAL_INTEGRATION_BEARER_FILE="$studio_test_dir/approved-bearer" \
  STUDIO_SOCIAL_INTEGRATION_ORGANIZATION_ID=synthetic-org \
  "$studio_fixture/scripts/engagement-compose.sh" config --quiet 2>/dev/null; then
  echo "world-readable integration bearer was accepted" >&2
  exit 1
fi

mkdir -p "$studio_fixture/.local/secrets"
chmod 700 "$studio_fixture/.local" "$studio_fixture/.local/secrets"
if STUDIO_WEB_VITALS_ENABLED=1 "$studio_fixture/scripts/engagement-compose.sh" config --quiet 2>/dev/null; then
  echo "Web Vitals storage accepted a missing protected Dragonfly config" >&2
  exit 1
fi
if STUDIO_WEB_VITALS_ENABLED=unexpected "$studio_fixture/scripts/engagement-compose.sh" config --quiet 2>/dev/null; then
  echo "Web Vitals storage accepted an unknown activation mode" >&2
  exit 1
fi
printf '%s\n' '--requirepass=synthetic-web-vitals-password-123456789' > "$studio_fixture/.local/secrets/web_vitals_dragonfly_config"
chmod 644 "$studio_fixture/.local/secrets/web_vitals_dragonfly_config"
if STUDIO_WEB_VITALS_ENABLED=1 "$studio_fixture/scripts/engagement-compose.sh" config --quiet 2>/dev/null; then
  echo "Web Vitals storage accepted a world-readable Dragonfly config" >&2
  exit 1
fi
chmod 600 "$studio_fixture/.local/secrets/web_vitals_dragonfly_config"
STUDIO_WEB_VITALS_ENABLED=1 "$studio_fixture/scripts/engagement-compose.sh" config --quiet
if ! grep -q -- '-f ./docker-compose.web-vitals.studio.yml' "$STUDIO_DOCKER_TEST_LOG"; then
  echo "Web Vitals storage overlay was not selected" >&2
  exit 1
fi
if grep -q synthetic-web-vitals-password "$STUDIO_DOCKER_TEST_LOG"; then
  echo "Web Vitals storage credential entered Docker arguments" >&2
  exit 1
fi
printf '%s' synthetic-marketing-service-key > "$studio_fixture/.local/secrets/marketing_plane_service_key"
printf '%s\n' '{"version":1,"organizationId":"synthetic-org","googleConnectionId":"synthetic-google-connection"}' > "$studio_fixture/.local/marketing-integration.json"
chmod 600 "$studio_fixture/.local/secrets/marketing_plane_service_key" "$studio_fixture/.local/marketing-integration.json"
"$studio_fixture/scripts/engagement-compose.sh" config --quiet
for overlay in marketing-bearer marketing-live; do
  if ! grep -q -- "-f ./docker-compose.$overlay.studio.yml" "$STUDIO_DOCKER_TEST_LOG"; then
    echo "saved Marketing configuration did not restore $overlay" >&2
    exit 1
  fi
done
if STUDIO_MARKETING_INTEGRATION_BEARER_FILE="$studio_fixture/.local/secrets/marketing_plane_service_key" \
  "$studio_fixture/scripts/engagement-compose.sh" config --quiet 2>/dev/null; then
  echo "static and managed Marketing bearers were accepted together" >&2
  exit 1
fi
for invalid_volume in unrelated-volume 'engagement-plane_marketing-../other'; do
  printf '%s\n' "{\"version\":1,\"organizationId\":\"synthetic-org\",\"googleConnectionId\":\"synthetic-google-connection\",\"dataVolume\":\"$invalid_volume\"}" > "$studio_fixture/.local/marketing-integration.json"
  if "$studio_fixture/scripts/engagement-compose.sh" config --quiet 2>/dev/null; then
    echo "an unrelated or unsafe Marketing projection volume was accepted" >&2
    exit 1
  fi
done
printf '%s\n' '{"version":1,"organizationId":"synthetic-org","googleConnectionId":"synthetic-google-connection","dataVolume":"engagement-plane_marketing-workspace-synthetic-org"}' > "$studio_fixture/.local/marketing-integration.json"
"$studio_fixture/scripts/engagement-compose.sh" config --quiet
printf '%s\n' '{"version":1,"organizationId":"*","googleConnectionId":"synthetic-google-connection"}' > "$studio_fixture/.local/marketing-integration.json"
if "$studio_fixture/scripts/engagement-compose.sh" config --quiet 2>/dev/null; then
  echo "unbounded organization in saved Marketing configuration was accepted" >&2
  exit 1
fi
rm "$studio_fixture/.local/marketing-integration.json"

if STUDIO_PLANE_DB_PASSWORD='unsafe@value' "$studio_fixture/scripts/engagement-compose.sh" config --quiet 2>/dev/null; then
  echo "a URL-unsafe database password must be rejected before Compose" >&2
  exit 1
fi
printf '%s\n' 'STUDIO_PLANE_DB_PASSWORD=validation-only' >> "$studio_fixture/visibility-core/.env"
if "$studio_fixture/scripts/engagement-compose.sh" config --quiet 2>/dev/null; then
  echo "infrastructure passwords in core env files must be rejected" >&2
  exit 1
fi
printf '%s\n' 'INTERNAL_API_KEY=marketing-test-key' > "$studio_fixture/visibility-core/.env"
if STUDIO_PLANE_DB_USER='unsafe@user' "$studio_fixture/scripts/engagement-compose.sh" config --quiet 2>/dev/null; then
  echo "a URL-unsafe database user must be rejected before Compose" >&2
  exit 1
fi

printf '%s\n' 'INTERNAL_API_KEY=social-test-key' 'STUDIO_SOCIAL_NATS_PASSWORD=unsafe@pass' > "$studio_fixture/social-core/.env"
if "$studio_fixture/scripts/engagement-compose.sh" config --quiet 2>/dev/null; then
  echo "unsafe NATS passwords must be rejected before Compose" >&2
  exit 1
fi
printf '%s\n' 'INTERNAL_API_KEY=social-test-key' 'INTERNAL_API_KEY=second-test-key' 'STUDIO_SOCIAL_NATS_PASSWORD=social-test-pass' > "$studio_fixture/social-core/.env"
if "$studio_fixture/scripts/engagement-compose.sh" config --quiet 2>/dev/null; then
  echo "duplicate projected keys must be rejected" >&2
  exit 1
fi
printf '%s\n' 'INTERNAL_API_KEY=${UNTRUSTED_KEY}' 'STUDIO_SOCIAL_NATS_PASSWORD=social-test-pass' > "$studio_fixture/social-core/.env"
if "$studio_fixture/scripts/engagement-compose.sh" config --quiet 2>/dev/null; then
  echo "interpolated projected keys must be rejected" >&2
  exit 1
fi
printf '%s\n' 'INTERNAL_API_KEY="social-test-key"' 'STUDIO_SOCIAL_NATS_PASSWORD=social-test-pass' > "$studio_fixture/social-core/.env"
if "$studio_fixture/scripts/engagement-compose.sh" config --quiet 2>/dev/null; then
  echo "quoted projected keys must be rejected" >&2
  exit 1
fi
printf '%s\n' 'INTERNAL_API_KEY=social-test-key' 'STUDIO_SOCIAL_NATS_PASSWORD=social-test-pass' > "$studio_fixture/social-core/.env"

"$studio_fixture/scripts/engagement-compose.sh" up -d
grep -q '^network inspect inter-plane-bus$' "$STUDIO_DOCKER_TEST_LOG"
grep -q '^network create --driver bridge ' "$STUDIO_DOCKER_TEST_LOG"
test -f "$STUDIO_DOCKER_TEST_NETWORK"

"$studio_fixture/scripts/engagement-compose.sh" --profile marketing up -d
if [ "$(grep -c '^network create ' "$STUDIO_DOCKER_TEST_LOG")" -ne 1 ]; then
  echo "existing inter-plane-bus must not be recreated" >&2
  exit 1
fi

printf '%s\n' 'INTERNAL_API_KEY=leads-test-key' 'STUDIO_LEADS_NATS_PASSWORD=social-test-pass' > "$studio_fixture/leads-core/.env"
if "$studio_fixture/scripts/engagement-compose.sh" up -d 2>/dev/null; then
  echo "shared NATS passwords must block startup" >&2
  exit 1
fi

printf '%s\n' 'INTERNAL_API_KEY=social-test-key' 'STUDIO_LEADS_NATS_PASSWORD=leads-test-pass' > "$studio_fixture/leads-core/.env"
if "$studio_fixture/scripts/engagement-compose.sh" up -d 2>/dev/null; then
  echo "shared internal keys must block startup" >&2
  exit 1
fi
if "$studio_fixture/scripts/engagement-compose.sh" start 2>/dev/null ||
   "$studio_fixture/scripts/engagement-compose.sh" restart 2>/dev/null; then
  echo "shared internal keys must block start and restart" >&2
  exit 1
fi
printf '%s\n' 'INTERNAL_API_KEY=leads-test-key' 'STUDIO_LEADS_NATS_PASSWORD=leads-test-pass' 'BILLING_CORE_URL=http://billing-core:8080' > "$studio_fixture/leads-core/.env"
if ! "$studio_fixture/scripts/engagement-compose.sh" up -d 2>/dev/null; then
  echo "partial Leads billing configuration should start with gated actions unavailable" >&2
  exit 1
fi

mkdir -p "$studio_test_dir/secrets"
printf '%s\n' 'local-social-key' > "$studio_test_dir/secrets/social_internal_api_key"
printf '%s\n' 'local-leads-key' > "$studio_test_dir/secrets/leads_internal_api_key"
printf '%s\n' 'local-marketing-key' > "$studio_test_dir/secrets/marketing_internal_api_key"
printf '%s\n' 'local-db-password' > "$studio_test_dir/secrets/studio_db_password"
printf '%s\n' 'local-dragonfly-password' > "$studio_test_dir/secrets/studio_dragonfly_password"
printf '%s\n' 'local-provisioner-password' > "$studio_test_dir/secrets/studio_nats_provisioner_password"
printf '%s\n' 'local-grafana-password' > "$studio_test_dir/secrets/studio_grafana_password"
chmod 600 "$studio_test_dir/secrets/"*
printf '%s\n' 'INTERNAL_API_KEY=shared-template-key' 'STUDIO_SOCIAL_NATS_PASSWORD=social-test-pass' > "$studio_fixture/social-core/.env"
printf '%s\n' 'INTERNAL_API_KEY=shared-template-key' 'STUDIO_LEADS_NATS_PASSWORD=leads-test-pass' > "$studio_fixture/leads-core/.env"
printf '%s\n' 'INTERNAL_API_KEY=shared-template-key' > "$studio_fixture/visibility-core/.env"
if ! STUDIO_LOCAL_SECRETS_DIR="$studio_test_dir/secrets" "$studio_fixture/scripts/engagement-compose.sh" up -d; then
  echo "protected local secret files should override shared template keys" >&2
  exit 1
fi

echo "studio-compose launcher network contract: ok"
