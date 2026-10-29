#!/bin/sh
set -eu

root=$(CDPATH= cd "$(dirname "$0")/.." && pwd)
fixture_dir=$(mktemp -d)
cleanup() {
  rm -f "$fixture_dir/source.env" "$fixture_dir/secrets/integration_db_password" \
    "$fixture_dir/secrets/integration_google_client_secret" "$fixture_dir/secrets/integration_meta_app_client_secret" \
    "$fixture_dir/secrets/integration_facebook_client_secret" "$fixture_dir/secrets/integration_pagespeed_api_key" \
    "$fixture_dir/secrets/integration_crux_api_key" "$fixture_dir/secrets/integration_clarity_bindings.json" \
    "$fixture_dir/secrets/integration_clarity_export_token" "$fixture_dir/secrets/integration_clarity_triodelab_export_token" \
    "$fixture_dir/secrets/integration_web_vitals_bindings.json" "$fixture_dir/secrets/integration_web_vitals_triodelab_development_export_token" \
    "$fixture_dir/secrets/integration_cookiebot_data_api_key" "$fixture_dir/secrets/integration_algolia_analytics_api_key" \
    "$fixture_dir/secrets/integration_marketing_providers_bindings.json" "$fixture_dir/secrets/integration_algolia_oauth_client.json"
  rmdir "$fixture_dir/secrets" "$fixture_dir"
}
trap cleanup EXIT INT TERM
mkdir "$fixture_dir/secrets"
chmod 700 "$fixture_dir/secrets"
printf '%s\n' \
  'INTERNAL_API_KEY=synthetic-internal-key' \
  'AUTH_CORE_INTERNAL_API_KEY=synthetic-auth-key' \
  'INTEGRATION_ORG_CORE_SERVICE_TOKEN=synthetic-org-service-token-123456789' \
  'INTEGRATION_BILLING_CORE_SERVICE_TOKEN=synthetic-billing-service-token-123456789' \
  'INTEGRATION_AUDIT_CORE_SERVICE_TOKEN=synthetic-audit-service-token-123456789' \
  'INTEGRATION_CREDENTIALS_ENCRYPTION_KEY=synthetic-vault-key' \
  'INTEGRATION_PROVIDER_WRITE_ATTESTATION_KEYS_JSON=[]' \
  'INTEGRATION_PUBLIC_BASE_URL=http://localhost:3026' \
  'GOOGLE_CLIENT_ID=synthetic-client-id' \
  'GOOGLE_CLIENT_SECRET=synthetic-secret' > "$fixture_dir/source.env"
printf '%s' synthetic-db-password > "$fixture_dir/secrets/integration_db_password"
printf '%s' synthetic-secret > "$fixture_dir/secrets/integration_google_client_secret"
chmod 600 "$fixture_dir/source.env" "$fixture_dir/secrets/integration_db_password" "$fixture_dir/secrets/integration_google_client_secret"

INTEGRATION_LOCAL_SOURCE_ENV="$fixture_dir/source.env" \
  STUDIO_LOCAL_SECRETS_DIR="$fixture_dir/secrets" \
  sh "$root/scripts/integration-local.sh" config --quiet

INTEGRATION_LOCAL_SOURCE_ENV="$fixture_dir/source.env" \
  STUDIO_LOCAL_SECRETS_DIR="$fixture_dir/secrets" \
  INTEGRATION_LOCAL_PUBLISH=1 \
  INTEGRATION_LOCAL_HOST_PORT=3026 \
  sh "$root/scripts/integration-local.sh" config --quiet

if INTEGRATION_LOCAL_SOURCE_ENV="$fixture_dir/source.env" \
  STUDIO_LOCAL_SECRETS_DIR="$fixture_dir/secrets" \
  INTEGRATION_LOCAL_PUBLISH=1 \
  INTEGRATION_LOCAL_HOST_PORT=33026 \
  sh "$root/scripts/integration-local.sh" config --quiet >/dev/null 2>&1; then
  echo "unregistered OAuth callback port was accepted" >&2
  exit 1
fi

printf '%s\n' \
  'META_APP_CLIENT_ID=123456789' \
  'META_APP_CLIENT_SECRET=synthetic-meta-secret' \
  'FACEBOOK_CLIENT_ID=987654321' \
  'FACEBOOK_CLIENT_SECRET=synthetic-facebook-secret' >> "$fixture_dir/source.env"
if INTEGRATION_LOCAL_SOURCE_ENV="$fixture_dir/source.env" \
  STUDIO_LOCAL_SECRETS_DIR="$fixture_dir/secrets" INTEGRATION_LOCAL_META=1 \
  sh "$root/scripts/integration-local.sh" config --quiet >/dev/null 2>&1; then
  echo "Meta deployment accepted missing protected provider secrets" >&2
  exit 1
fi
printf '%s' synthetic-meta-secret > "$fixture_dir/secrets/integration_meta_app_client_secret"
printf '%s' synthetic-facebook-secret > "$fixture_dir/secrets/integration_facebook_client_secret"
chmod 600 "$fixture_dir/secrets/integration_meta_app_client_secret" "$fixture_dir/secrets/integration_facebook_client_secret"
INTEGRATION_LOCAL_SOURCE_ENV="$fixture_dir/source.env" \
  STUDIO_LOCAL_SECRETS_DIR="$fixture_dir/secrets" INTEGRATION_LOCAL_META=1 \
  sh "$root/scripts/integration-local.sh" config --format json |
  jq -e '
    .services."integration-api".environment.META_APP_CLIENT_ID == "123456789"
    and .services."integration-api".environment.META_APP_CLIENT_SECRET == ""
    and .services."integration-api".environment.META_APP_CLIENT_SECRET_FILE == "/run/secrets/meta_app_client_secret"
    and .services."integration-api".environment.FACEBOOK_CLIENT_ID == "987654321"
    and .services."integration-api".environment.FACEBOOK_CLIENT_SECRET == ""
    and .services."integration-api".environment.FACEBOOK_CLIENT_SECRET_FILE == "/run/secrets/facebook_client_secret"
    and (.services."integration-api".secrets | length) == 3
  ' >/dev/null

if INTEGRATION_LOCAL_SOURCE_ENV="$fixture_dir/source.env" \
  STUDIO_LOCAL_SECRETS_DIR="$fixture_dir/secrets" INTEGRATION_LOCAL_PAGESPEED=1 \
  sh "$root/scripts/integration-local.sh" config --quiet >/dev/null 2>&1; then
  echo "PageSpeed deployment accepted a missing protected API key" >&2
  exit 1
fi
printf '%s' synthetic-pagespeed-key > "$fixture_dir/secrets/integration_pagespeed_api_key"
chmod 600 "$fixture_dir/secrets/integration_pagespeed_api_key"
INTEGRATION_LOCAL_SOURCE_ENV="$fixture_dir/source.env" \
  STUDIO_LOCAL_SECRETS_DIR="$fixture_dir/secrets" INTEGRATION_LOCAL_PAGESPEED=1 INTEGRATION_LOCAL_META=1 \
  sh "$root/scripts/integration-local.sh" config --format json |
  jq -e '
    .services."integration-api".environment.PAGESPEED_API_KEY_FILE == "/run/secrets/pagespeed_api_key"
    and .services."integration-api".environment.PAGESPEED_API_KEY == null
    and (.services."integration-api".secrets | length) == 4
    and .secrets.pagespeed_api_key.file != null
  ' >/dev/null
chmod 644 "$fixture_dir/secrets/integration_pagespeed_api_key"
if INTEGRATION_LOCAL_SOURCE_ENV="$fixture_dir/source.env" \
  STUDIO_LOCAL_SECRETS_DIR="$fixture_dir/secrets" INTEGRATION_LOCAL_PAGESPEED=1 \
  sh "$root/scripts/integration-local.sh" config --quiet >/dev/null 2>&1; then
  echo "PageSpeed deployment accepted a world-readable API key" >&2
  exit 1
fi

if INTEGRATION_LOCAL_SOURCE_ENV="$fixture_dir/source.env" \
  STUDIO_LOCAL_SECRETS_DIR="$fixture_dir/secrets" INTEGRATION_LOCAL_CRUX=1 \
  sh "$root/scripts/integration-local.sh" config --quiet >/dev/null 2>&1; then
  echo "CrUX deployment accepted a missing protected API key" >&2
  exit 1
fi
printf '%s' synthetic-dedicated-crux-key > "$fixture_dir/secrets/integration_crux_api_key"
chmod 600 "$fixture_dir/secrets/integration_crux_api_key"
INTEGRATION_LOCAL_SOURCE_ENV="$fixture_dir/source.env" \
  STUDIO_LOCAL_SECRETS_DIR="$fixture_dir/secrets" INTEGRATION_LOCAL_CRUX=1 \
  sh "$root/scripts/integration-local.sh" config --format json |
  jq -e '
    .services."integration-api".environment.CRUX_API_KEY_FILE == "/run/secrets/crux_api_key"
    and .services."integration-api".environment.CRUX_API_KEY == null
    and .services."integration-api".environment.PAGESPEED_API_KEY_FILE == null
    and (.services."integration-api".secrets | length) == 2
    and .secrets.crux_api_key.file != null
  ' >/dev/null
chmod 644 "$fixture_dir/secrets/integration_crux_api_key"
if INTEGRATION_LOCAL_SOURCE_ENV="$fixture_dir/source.env" \
  STUDIO_LOCAL_SECRETS_DIR="$fixture_dir/secrets" INTEGRATION_LOCAL_CRUX=1 \
  sh "$root/scripts/integration-local.sh" config --quiet >/dev/null 2>&1; then
  echo "CrUX deployment accepted a world-readable API key" >&2
  exit 1
fi
if INTEGRATION_LOCAL_SOURCE_ENV="$fixture_dir/source.env" \
  STUDIO_LOCAL_SECRETS_DIR="$fixture_dir/secrets" INTEGRATION_LOCAL_CRUX=unexpected \
  sh "$root/scripts/integration-local.sh" config --quiet >/dev/null 2>&1; then
  echo "CrUX deployment accepted an unknown activation mode" >&2
  exit 1
fi

if INTEGRATION_LOCAL_SOURCE_ENV="$fixture_dir/source.env" \
  STUDIO_LOCAL_SECRETS_DIR="$fixture_dir/secrets" INTEGRATION_LOCAL_CLARITY=1 \
  sh "$root/scripts/integration-local.sh" config --quiet >/dev/null 2>&1; then
  echo "Clarity deployment accepted missing protected bindings and token" >&2
  exit 1
fi
printf '%s' '{"version":1,"bindings":[{"organizationId":"test-org","site":"https://example.com/","projectId":"testproject","tokenFile":"clarity_export_token","confirmedAt":"2026-10-21T00:00:00Z"}]}' > "$fixture_dir/secrets/integration_clarity_bindings.json"
printf '%s' 'synthetic.header.signature' > "$fixture_dir/secrets/integration_clarity_export_token"
chmod 600 "$fixture_dir/secrets/integration_clarity_bindings.json" "$fixture_dir/secrets/integration_clarity_export_token"
INTEGRATION_LOCAL_SOURCE_ENV="$fixture_dir/source.env" \
  STUDIO_LOCAL_SECRETS_DIR="$fixture_dir/secrets" INTEGRATION_LOCAL_CLARITY=1 \
  sh "$root/scripts/integration-local.sh" config --format json |
  jq -e '
    .services."integration-api".environment.CLARITY_BINDINGS_FILE == "/run/secrets/clarity_bindings"
    and .services."integration-api".environment.CLARITY_API_TOKEN == null
    and (.services."integration-api".secrets | length) == 3
    and .secrets.clarity_bindings.file != null
    and .secrets.clarity_export_token.file != null
  ' >/dev/null
chmod 644 "$fixture_dir/secrets/integration_clarity_export_token"
if INTEGRATION_LOCAL_SOURCE_ENV="$fixture_dir/source.env" \
  STUDIO_LOCAL_SECRETS_DIR="$fixture_dir/secrets" INTEGRATION_LOCAL_CLARITY=1 \
  sh "$root/scripts/integration-local.sh" config --quiet >/dev/null 2>&1; then
  echo "Clarity deployment accepted a world-readable export token" >&2
  exit 1
fi
if INTEGRATION_LOCAL_SOURCE_ENV="$fixture_dir/source.env" \
  STUDIO_LOCAL_SECRETS_DIR="$fixture_dir/secrets" INTEGRATION_LOCAL_CLARITY=unexpected \
  sh "$root/scripts/integration-local.sh" config --quiet >/dev/null 2>&1; then
  echo "Clarity deployment accepted an unknown activation mode" >&2
  exit 1
fi

chmod 600 "$fixture_dir/secrets/integration_clarity_export_token"
triodelab_token="$fixture_dir/secrets/integration_clarity_triodelab_export_token"
triodelab_config() {
  INTEGRATION_LOCAL_SOURCE_ENV="$fixture_dir/source.env" \
    STUDIO_LOCAL_SECRETS_DIR="$fixture_dir/secrets" \
    INTEGRATION_LOCAL_CLARITY="${test_clarity_mode:-1}" \
    INTEGRATION_LOCAL_CLARITY_TRIODELAB="${test_triodelab_mode:-1}" \
    sh "$root/scripts/integration-local.sh" config --format json
}
reject_triodelab_config() {
  if triodelab_config >/dev/null 2>&1; then
    echo "$1" >&2
    exit 1
  fi
}

reject_triodelab_config "Triodelab Clarity accepted a missing protected export token"
if INTEGRATION_LOCAL_CLARITY_TRIODELAB_TOKEN_FILE="$fixture_dir/secrets/integration_clarity_export_token" \
  triodelab_config >/dev/null 2>&1; then
  echo "Triodelab Clarity accepted an overridden token filename instead of its independent credential" >&2
  exit 1
fi
printf '%s' synthetic-triodelab-token > "$triodelab_token"
chmod 644 "$triodelab_token"
reject_triodelab_config "Triodelab Clarity accepted a world-readable export token"
chmod 600 "$triodelab_token"
test_clarity_mode=0
reject_triodelab_config "Triodelab Clarity accepted activation without the base Clarity overlay"
test_clarity_mode=1
test_triodelab_mode=unexpected
reject_triodelab_config "Triodelab Clarity accepted an unknown activation mode"
test_triodelab_mode=1
printf '' > "$triodelab_token"
reject_triodelab_config "Triodelab Clarity accepted an empty export token"
awk 'BEGIN { for (i=0; i<8193; i++) printf "x" }' > "$triodelab_token"
reject_triodelab_config "Triodelab Clarity accepted an oversized export token"
rm "$triodelab_token"
ln -s integration_clarity_export_token "$triodelab_token"
reject_triodelab_config "Triodelab Clarity accepted a symlink export token"
rm "$triodelab_token"
printf '%s' synthetic-triodelab-token > "$triodelab_token"
chmod 600 "$triodelab_token"
triodelab_config | jq -e --arg token "$triodelab_token" '
  .services."integration-api".environment.CLARITY_BINDINGS_FILE == "/run/secrets/clarity_bindings"
  and .services."integration-api".environment.CLARITY_API_TOKEN == null
  and (.services."integration-api".secrets | length) == 4
  and ([.services."integration-api".secrets[].target] | sort) ==
    (["/run/secrets/google_client_secret", "/run/secrets/clarity_bindings", "/run/secrets/clarity_export_token", "/run/secrets/clarity_triodelab_export_token"] | sort)
  and .secrets.clarity_triodelab_export_token.file == $token
  and .secrets.clarity_export_token.file != null
' >/dev/null
test_triodelab_mode=0
chmod 644 "$triodelab_token"
triodelab_config | jq -e '
  (.services."integration-api".secrets | length) == 3
  and .secrets.clarity_triodelab_export_token == null
  and ([.services."integration-api".secrets[].target] | index("/run/secrets/clarity_triodelab_export_token")) == null
' >/dev/null

if INTEGRATION_LOCAL_SOURCE_ENV="$fixture_dir/source.env" \
  STUDIO_LOCAL_SECRETS_DIR="$fixture_dir/secrets" INTEGRATION_LOCAL_WEB_VITALS=1 \
  sh "$root/scripts/integration-local.sh" config --quiet >/dev/null 2>&1; then
  echo "Web Vitals deployment accepted missing protected bindings" >&2
  exit 1
fi
if INTEGRATION_LOCAL_SOURCE_ENV="$fixture_dir/source.env" \
  STUDIO_LOCAL_SECRETS_DIR="$fixture_dir/secrets" INTEGRATION_LOCAL_WEB_VITALS=unexpected \
  sh "$root/scripts/integration-local.sh" config --quiet >/dev/null 2>&1; then
  echo "Web Vitals deployment accepted an unknown activation mode" >&2
  exit 1
fi
vitals_bindings="$fixture_dir/secrets/integration_web_vitals_bindings.json"
vitals_token="$fixture_dir/secrets/integration_web_vitals_triodelab_development_export_token"
printf '%s' '{"version":1,"installations":[]}' > "$vitals_bindings"
printf '%s' 'synthetic-vitals-export-token-123456789' > "$vitals_token"
chmod 600 "$vitals_bindings" "$vitals_token"
INTEGRATION_LOCAL_SOURCE_ENV="$fixture_dir/source.env" \
  STUDIO_LOCAL_SECRETS_DIR="$fixture_dir/secrets" INTEGRATION_LOCAL_WEB_VITALS=1 \
  sh "$root/scripts/integration-local.sh" config --format json |
  jq -e '
    .services."integration-api".environment.WEB_VITALS_BINDINGS_FILE == "/run/secrets/web_vitals_bindings"
    and .secrets.web_vitals_bindings.file != null
    and .secrets.web_vitals_triodelab_development_export_token.file != null
    and (.services."integration-api".secrets | length) == 3
  ' >/dev/null
chmod 644 "$vitals_token"
if INTEGRATION_LOCAL_SOURCE_ENV="$fixture_dir/source.env" \
  STUDIO_LOCAL_SECRETS_DIR="$fixture_dir/secrets" INTEGRATION_LOCAL_WEB_VITALS=1 \
  sh "$root/scripts/integration-local.sh" config --quiet >/dev/null 2>&1; then
  echo "Web Vitals deployment accepted a world-readable token" >&2
  exit 1
fi
rm "$vitals_bindings" "$vitals_token"

# The provider API keys may be empty while onboarding is incomplete. They must
# still be private regular files and must never become container env values.
site_source_config() {
  INTEGRATION_LOCAL_SOURCE_ENV="$fixture_dir/source.env" \
    STUDIO_LOCAL_SECRETS_DIR="$fixture_dir/secrets" INTEGRATION_LOCAL_SITE_SOURCES="${site_source_mode:-1}" \
    sh "$root/scripts/integration-local.sh" config --format json
}
if site_source_config >/dev/null 2>&1; then
  echo "site sources accepted missing protected files" >&2; exit 1
fi
node "$root/scripts/prepare-site-source-secrets.mjs" "$fixture_dir/secrets" >/dev/null
site_source_config | jq -e '
  .services."integration-api".environment.MARKETING_PROVIDERS_BINDINGS_FILE == "/run/secrets/marketing_providers_bindings"
  and .services."integration-api".environment.COOKIEBOT_API_KEY == null
  and .services."integration-api".environment.ALGOLIA_API_KEY == null
  and .secrets.cookiebot_data_api_key.file != null
  and .secrets.algolia_analytics_api_key.file != null
  and (.services."integration-api".secrets | length) == 4
' >/dev/null
chmod 644 "$fixture_dir/secrets/integration_cookiebot_data_api_key"
if site_source_config >/dev/null 2>&1; then
  echo "site sources accepted a public API key file" >&2; exit 1
fi
chmod 600 "$fixture_dir/secrets/integration_cookiebot_data_api_key"
site_source_mode=unexpected
if site_source_config >/dev/null 2>&1; then
  echo "site sources accepted an invalid activation mode" >&2; exit 1
fi
rm "$fixture_dir/secrets/integration_cookiebot_data_api_key" \
  "$fixture_dir/secrets/integration_algolia_analytics_api_key" \
  "$fixture_dir/secrets/integration_marketing_providers_bindings.json"

algolia_oauth_config() {
  INTEGRATION_LOCAL_SOURCE_ENV="$fixture_dir/source.env" \
    STUDIO_LOCAL_SECRETS_DIR="$fixture_dir/secrets" INTEGRATION_LOCAL_ALGOLIA_OAUTH="${algolia_oauth_mode:-1}" \
    sh "$root/scripts/integration-local.sh" config --format json
}
if algolia_oauth_config >/dev/null 2>&1; then
  echo "Algolia OAuth accepted missing owned client registration" >&2; exit 1
fi
printf '%s' '{"version":1,"clientId":"synthetic-owned-public-client","redirectUri":"http://localhost:3026/api/v1/providers/algolia/callback","resource":"https://mcp.algolia.com/mcp"}' > "$fixture_dir/secrets/integration_algolia_oauth_client.json"
chmod 600 "$fixture_dir/secrets/integration_algolia_oauth_client.json"
algolia_oauth_config | jq -e '
  .services."integration-api".environment.ALGOLIA_MCP_OAUTH_CLIENT_FILE == "/run/secrets/algolia_oauth_client"
  and .services."integration-api".environment.ALGOLIA_CLIENT_SECRET == null
  and .secrets.algolia_oauth_client.file != null
  and (.services."integration-api".secrets | length) == 2
' >/dev/null
chmod 644 "$fixture_dir/secrets/integration_algolia_oauth_client.json"
if algolia_oauth_config >/dev/null 2>&1; then
  echo "Algolia OAuth accepted public registration file" >&2; exit 1
fi
algolia_oauth_mode=unexpected
if algolia_oauth_config >/dev/null 2>&1; then
  echo "Algolia OAuth accepted invalid activation mode" >&2; exit 1
fi

printf '%s' mismatched-secret > "$fixture_dir/secrets/integration_google_client_secret"
if INTEGRATION_LOCAL_SOURCE_ENV="$fixture_dir/source.env" \
  STUDIO_LOCAL_SECRETS_DIR="$fixture_dir/secrets" \
  sh "$root/scripts/integration-local.sh" config --quiet >/dev/null 2>&1; then
  echo "Google client ID and secret from different configurations were accepted" >&2
  exit 1
fi

echo "Integration local launcher contract passed"
