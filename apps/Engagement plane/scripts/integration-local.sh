#!/bin/sh
# Curated projection for the persistent local Integration Core. Never source a
# provider .env as shell code or inject the whole file into the container.
set -eu

root=$(CDPATH= cd "$(dirname "$0")/.." && pwd)
source_env=${INTEGRATION_LOCAL_SOURCE_ENV:-"$root/../Ingestion Plane/integration-corev2/.env"}
secret_dir=${STUDIO_LOCAL_SECRETS_DIR:-"$root/.local/secrets"}
db_password_file="$secret_dir/integration_db_password"
google_secret="$secret_dir/integration_google_client_secret"

file_mode() {
  stat -f '%Lp' "$1" 2>/dev/null || stat -c '%a' "$1"
}

require_private_file() {
  if [ ! -f "$1" ] || [ -L "$1" ] || [ "$(file_mode "$1")" != 600 ]; then
    echo "$2 must be a regular mode-0600 file" >&2
    exit 1
  fi
}

read_literal() {
  awk -v wanted="$2" '
    {
      separator = index($0, "=")
      if (separator == 0 || substr($0, 1, separator - 1) != wanted) next
      count++
      value = substr($0, separator + 1)
      sub(/\r$/, "", value)
    }
    END {
      if (count != 1 || value == "") exit 2
      print value
    }
  ' "$1"
}

require_private_file "$source_env" "Integration source env"
require_private_file "$db_password_file" "Integration database password"
require_private_file "$google_secret" "Google client secret"
if [ "$(basename "$secret_dir")" != secrets ] || [ -L "$secret_dir" ] ||
   [ "$(file_mode "$secret_dir")" != 700 ]; then
  echo "The local secrets directory must be a non-symlink mode-0700 directory named secrets" >&2
  exit 1
fi

project() {
  if ! value=$(read_literal "$2" "$3"); then
    echo "$3 is missing or duplicated in its protected source" >&2
    exit 1
  fi
  export "$1=$value"
}

project_optional_id() {
  if ! value=$(awk -v wanted="$2" '
    index($0, "=") && substr($0, 1, index($0, "=") - 1) == wanted {
      count++; value = substr($0, index($0, "=") + 1); sub(/\r$/, "", value)
    }
    END { if (count > 1 || value ~ /[^0-9]/) exit 2; if (count) print value }
  ' "$source_env"); then
    echo "$2 must be a unique numeric ID when present" >&2
    exit 1
  fi
  export "$1=$value"
}

verify_provider_secret() {
  require_private_file "$2" "$1 protected secret"
  provider_secret=$(read_literal "$source_env" "$1") || {
    echo "$1 is missing or duplicated in the protected Integration source" >&2
    exit 1
  }
  if [ "$provider_secret" != "$(cat "$2")" ]; then
    echo "$1 protected secret does not match the canonical Integration configuration" >&2
    exit 1
  fi
  unset provider_secret
}

if ! INTEGRATION_LOCAL_DB_PASSWORD=$(tr -d '\r\n' < "$db_password_file"); then
  echo "Could not read Integration database password" >&2
  exit 1
fi
export INTEGRATION_LOCAL_DB_PASSWORD
project INTEGRATION_LOCAL_INTERNAL_API_KEY "$source_env" INTERNAL_API_KEY
project INTEGRATION_LOCAL_AUTH_CORE_INTERNAL_API_KEY "$source_env" AUTH_CORE_INTERNAL_API_KEY
project INTEGRATION_LOCAL_ORG_SERVICE_TOKEN "$source_env" INTEGRATION_ORG_CORE_SERVICE_TOKEN
project INTEGRATION_LOCAL_BILLING_SERVICE_TOKEN "$source_env" INTEGRATION_BILLING_CORE_SERVICE_TOKEN
project INTEGRATION_LOCAL_AUDIT_SERVICE_TOKEN "$source_env" INTEGRATION_AUDIT_CORE_SERVICE_TOKEN
project INTEGRATION_LOCAL_VAULT_KEY "$source_env" INTEGRATION_CREDENTIALS_ENCRYPTION_KEY
project INTEGRATION_LOCAL_WRITE_ATTESTATION_KEYS_JSON "$source_env" INTEGRATION_PROVIDER_WRITE_ATTESTATION_KEYS_JSON
project INTEGRATION_LOCAL_GOOGLE_CLIENT_ID "$source_env" GOOGLE_CLIENT_ID
# Verify the pair comes from the same canonical Integration configuration.
# Never silently combine an updated client ID with a stale mounted secret.
verify_provider_secret GOOGLE_CLIENT_SECRET "$google_secret"
source_public_base=$(read_literal "$source_env" INTEGRATION_PUBLIC_BASE_URL) || {
  echo "INTEGRATION_PUBLIC_BASE_URL is missing or duplicated in the Integration source" >&2
  exit 1
}
if [ "$source_public_base" != http://localhost:3026 ]; then
  echo "Integration public base must match the registered intergration-core callback origin http://localhost:3026" >&2
  exit 1
fi
INTEGRATION_LOCAL_GOOGLE_SECRET_FILE="$google_secret"
export INTEGRATION_LOCAL_GOOGLE_SECRET_FILE

case "$INTEGRATION_LOCAL_DB_PASSWORD" in
  ""|*[!A-Za-z0-9._~-]*)
    echo "Integration database credential is not URL-safe" >&2
    exit 1
    ;;
esac

case "${INTEGRATION_LOCAL_MICROSOFT:-0}" in
  0) ;;
  1)
    project INTEGRATION_LOCAL_MICROSOFT_CLIENT_ID "$source_env" AZURE_CLIENT_ID
    project INTEGRATION_LOCAL_MICROSOFT_TENANT_ID "$source_env" AZURE_TENANT_ID
    if grep -q '^MICROSOFT_CLIENT_AUTH_MODE=' "$source_env"; then
      project INTEGRATION_LOCAL_MICROSOFT_AUTH_MODE "$source_env" MICROSOFT_CLIENT_AUTH_MODE
    else
      INTEGRATION_LOCAL_MICROSOFT_AUTH_MODE=public
      export INTEGRATION_LOCAL_MICROSOFT_AUTH_MODE
    fi
    case "$INTEGRATION_LOCAL_MICROSOFT_AUTH_MODE" in public|confidential) ;; *) echo "Invalid Microsoft client auth mode" >&2; exit 1 ;; esac
    INTEGRATION_LOCAL_MICROSOFT_SECRET_FILE="$secret_dir/integration_microsoft_client_secret"
    verify_provider_secret AZURE_CLIENT_SECRET "$INTEGRATION_LOCAL_MICROSOFT_SECRET_FILE"
    export INTEGRATION_LOCAL_MICROSOFT_SECRET_FILE
    set -- -f "$root/docker-compose.integration-microsoft.local.yml" "$@"
    ;;
  *) echo "INTEGRATION_LOCAL_MICROSOFT must be 0 or 1" >&2; exit 1 ;;
esac

case "${INTEGRATION_LOCAL_META:-0}" in
  0) ;;
  1)
    project_optional_id INTEGRATION_LOCAL_META_APP_CLIENT_ID META_APP_CLIENT_ID
    : "${INTEGRATION_LOCAL_META_APP_CLIENT_ID:?META_APP_CLIENT_ID is required for the Meta overlay}"
    INTEGRATION_LOCAL_META_APP_SECRET_FILE="$secret_dir/integration_meta_app_client_secret"
    verify_provider_secret META_APP_CLIENT_SECRET "$INTEGRATION_LOCAL_META_APP_SECRET_FILE"
    export INTEGRATION_LOCAL_META_APP_SECRET_FILE
    project_optional_id INTEGRATION_LOCAL_META_JS_SDK_APP_ID META_JS_SDK_APP_ID
    project_optional_id INTEGRATION_LOCAL_META_BUSINESS_LOGIN_CONFIG_ID META_BUSINESS_LOGIN_CONFIG_ID
    project_optional_id INTEGRATION_LOCAL_META_BUSINESS_LOGIN_SUPPORT_MESSAGING_CONFIG_ID META_BUSINESS_LOGIN_SUPPORT_MESSAGING_CONFIG_ID
    project_optional_id INTEGRATION_LOCAL_META_BUSINESS_LOGIN_CONVERSIONS_CONFIG_ID META_BUSINESS_LOGIN_CONVERSIONS_CONFIG_ID
    project_optional_id INTEGRATION_LOCAL_META_BUSINESS_LOGIN_STUDIO_ADS_CONFIG_ID META_BUSINESS_LOGIN_STUDIO_ADS_CONFIG_ID
    set -- -f "$root/docker-compose.integration-meta.local.yml" "$@"
    project_optional_id INTEGRATION_LOCAL_FACEBOOK_CLIENT_ID FACEBOOK_CLIENT_ID
    if [ -n "$INTEGRATION_LOCAL_FACEBOOK_CLIENT_ID" ]; then
      INTEGRATION_LOCAL_FACEBOOK_SECRET_FILE="$secret_dir/integration_facebook_client_secret"
      verify_provider_secret FACEBOOK_CLIENT_SECRET "$INTEGRATION_LOCAL_FACEBOOK_SECRET_FILE"
      export INTEGRATION_LOCAL_FACEBOOK_SECRET_FILE
      set -- -f "$root/docker-compose.integration-facebook.local.yml" "$@"
    fi
    ;;
  *) echo "INTEGRATION_LOCAL_META must be 0 or 1" >&2; exit 1 ;;
esac

cd "$root"
case "${INTEGRATION_LOCAL_CANONICAL:-0}" in
  0) ;;
  1)
    ingestion_root="$root/../Ingestion Plane"
    # Compose interpolation is separate from service env_file. Reuse the
    # existing stores; never generate/rotate credentials during this migration.
    if [ -n "${INTEGRATION_CANONICAL_ENV_FILE:-}" ]; then
      require_private_file "$INTEGRATION_CANONICAL_ENV_FILE" "Canonical interpolation file"
      set -- --env-file "$INTEGRATION_CANONICAL_ENV_FILE" "$@"
    else
      shared_env="$secret_dir/ingestion_plane_compose"
      require_private_file "$shared_env" "Ingestion shared interpolation store"
      generated_env="$ingestion_root/.env.generated-secrets"
      require_private_file "$generated_env" "Existing Ingestion generated store"
      set -- --env-file "$ingestion_root/Quarry-v2/.env" \
        --env-file "$ingestion_root/imports-core/.env" \
        --env-file "$ingestion_root/finspo-core/.env" \
        --env-file "$ingestion_root/autocomplete-core/.env" \
        --env-file "$source_env" --env-file "$generated_env" --env-file "$shared_env" "$@"
    fi
    ;;
  *) echo "INTEGRATION_LOCAL_CANONICAL must be 0 or 1" >&2; exit 1 ;;
esac
case "${INTEGRATION_LOCAL_PAGESPEED:-0}" in
  0) ;;
  1)
    INTEGRATION_LOCAL_PAGESPEED_KEY_FILE="$secret_dir/integration_pagespeed_api_key"
    require_private_file "$INTEGRATION_LOCAL_PAGESPEED_KEY_FILE" "PageSpeed API key"
    key_bytes=$(wc -c < "$INTEGRATION_LOCAL_PAGESPEED_KEY_FILE" | tr -d ' ')
    if [ "$key_bytes" -lt 1 ] || [ "$key_bytes" -gt 512 ]; then
      echo "PageSpeed API key file must contain 1–512 bytes" >&2
      exit 1
    fi
    export INTEGRATION_LOCAL_PAGESPEED_KEY_FILE
    set -- -f "$root/docker-compose.integration-pagespeed.local.yml" "$@"
    ;;
  *) echo "INTEGRATION_LOCAL_PAGESPEED must be 0 or 1" >&2; exit 1 ;;
esac
case "${INTEGRATION_LOCAL_WEB_VITALS:-0}" in
  0) ;;
  1)
    INTEGRATION_LOCAL_WEB_VITALS_BINDINGS_FILE="$secret_dir/integration_web_vitals_bindings.json"
    INTEGRATION_LOCAL_WEB_VITALS_TOKEN_FILE="$secret_dir/integration_web_vitals_triodelab_development_export_token"
    require_private_file "$INTEGRATION_LOCAL_WEB_VITALS_BINDINGS_FILE" "Web Vitals bindings"
    require_private_file "$INTEGRATION_LOCAL_WEB_VITALS_TOKEN_FILE" "Web Vitals export token"
    vitals_token_bytes=$(wc -c < "$INTEGRATION_LOCAL_WEB_VITALS_TOKEN_FILE" | tr -d ' ')
    if [ "$vitals_token_bytes" -lt 32 ] || [ "$vitals_token_bytes" -gt 256 ]; then
      echo "Web Vitals export token must contain 32–256 bytes" >&2
      exit 1
    fi
    export INTEGRATION_LOCAL_WEB_VITALS_BINDINGS_FILE INTEGRATION_LOCAL_WEB_VITALS_TOKEN_FILE
    set -- -f "$root/docker-compose.integration-web-vitals.local.yml" "$@"
    ;;
  *) echo "INTEGRATION_LOCAL_WEB_VITALS must be 0 or 1" >&2; exit 1 ;;
esac
case "${INTEGRATION_LOCAL_ALGOLIA_OAUTH:-0}" in
  0) ;;
  1)
    INTEGRATION_LOCAL_ALGOLIA_OAUTH_CLIENT_FILE="$secret_dir/integration_algolia_oauth_client.json"
    require_private_file "$INTEGRATION_LOCAL_ALGOLIA_OAUTH_CLIENT_FILE" "Owned Algolia public OAuth client"
    algolia_client_bytes=$(wc -c < "$INTEGRATION_LOCAL_ALGOLIA_OAUTH_CLIENT_FILE" | tr -d ' ')
    if [ "$algolia_client_bytes" -lt 1 ] || [ "$algolia_client_bytes" -gt 2048 ]; then
      echo "Algolia client registration must contain 1–2048 bytes" >&2; exit 1
    fi
    export INTEGRATION_LOCAL_ALGOLIA_OAUTH_CLIENT_FILE
    set -- -f "$root/docker-compose.integration-algolia-oauth.local.yml" "$@"
    ;;
  *) echo "INTEGRATION_LOCAL_ALGOLIA_OAUTH must be 0 or 1" >&2; exit 1 ;;
esac
case "${INTEGRATION_LOCAL_SITE_SOURCES:-0}" in
  0) ;;
  1)
    INTEGRATION_LOCAL_SITE_SOURCES_BINDINGS_FILE="$secret_dir/integration_marketing_providers_bindings.json"
    INTEGRATION_LOCAL_COOKIEBOT_KEY_FILE="$secret_dir/integration_cookiebot_data_api_key"
    INTEGRATION_LOCAL_ALGOLIA_KEY_FILE="$secret_dir/integration_algolia_analytics_api_key"
    require_private_file "$INTEGRATION_LOCAL_SITE_SOURCES_BINDINGS_FILE" "Provider bindings"
    require_private_file "$INTEGRATION_LOCAL_COOKIEBOT_KEY_FILE" "Cookiebot Data API key"
    require_private_file "$INTEGRATION_LOCAL_ALGOLIA_KEY_FILE" "Algolia analytics key"
    for provider_file in "$INTEGRATION_LOCAL_COOKIEBOT_KEY_FILE" "$INTEGRATION_LOCAL_ALGOLIA_KEY_FILE"; do
      provider_bytes=$(wc -c < "$provider_file" | tr -d ' ')
      if [ "$provider_bytes" -gt 1024 ]; then
        echo "Provider key placeholders must contain at most 1024 bytes" >&2; exit 1
      fi
    done
    export INTEGRATION_LOCAL_SITE_SOURCES_BINDINGS_FILE INTEGRATION_LOCAL_COOKIEBOT_KEY_FILE INTEGRATION_LOCAL_ALGOLIA_KEY_FILE
    set -- -f "$root/docker-compose.integration-site-sources.local.yml" "$@"
    ;;
  *) echo "INTEGRATION_LOCAL_SITE_SOURCES must be 0 or 1" >&2; exit 1 ;;
esac
case "${INTEGRATION_LOCAL_CRUX:-0}" in
  0) ;;
  1)
    INTEGRATION_LOCAL_CRUX_KEY_FILE="$secret_dir/integration_crux_api_key"
    require_private_file "$INTEGRATION_LOCAL_CRUX_KEY_FILE" "CrUX API key"
    crux_key_bytes=$(wc -c < "$INTEGRATION_LOCAL_CRUX_KEY_FILE" | tr -d ' ')
    if [ "$crux_key_bytes" -lt 1 ] || [ "$crux_key_bytes" -gt 512 ]; then
      echo "CrUX API key file must contain 1–512 bytes" >&2
      exit 1
    fi
    export INTEGRATION_LOCAL_CRUX_KEY_FILE
    set -- -f "$root/docker-compose.integration-crux.local.yml" "$@"
    ;;
  *) echo "INTEGRATION_LOCAL_CRUX must be 0 or 1" >&2; exit 1 ;;
esac
case "${INTEGRATION_LOCAL_CLARITY:-0}" in
  0) ;;
  1)
    INTEGRATION_LOCAL_CLARITY_BINDINGS_FILE="$secret_dir/integration_clarity_bindings.json"
    INTEGRATION_LOCAL_CLARITY_TOKEN_FILE="$secret_dir/integration_clarity_export_token"
    require_private_file "$INTEGRATION_LOCAL_CLARITY_BINDINGS_FILE" "Clarity binding manifest"
    require_private_file "$INTEGRATION_LOCAL_CLARITY_TOKEN_FILE" "Clarity export token"
    clarity_bytes=$(wc -c < "$INTEGRATION_LOCAL_CLARITY_TOKEN_FILE" | tr -d ' ')
    if [ "$clarity_bytes" -lt 1 ] || [ "$clarity_bytes" -gt 8192 ]; then
      echo "Clarity token file must contain 1–8192 bytes" >&2
      exit 1
    fi
    export INTEGRATION_LOCAL_CLARITY_BINDINGS_FILE INTEGRATION_LOCAL_CLARITY_TOKEN_FILE
    set -- -f "$root/docker-compose.integration-clarity.local.yml" "$@"
    ;;
  *) echo "INTEGRATION_LOCAL_CLARITY must be 0 or 1" >&2; exit 1 ;;
esac
case "${INTEGRATION_LOCAL_CLARITY_TRIODELAB:-0}" in
  0) ;;
  1)
    if [ "${INTEGRATION_LOCAL_CLARITY:-0}" != 1 ]; then
      echo "INTEGRATION_LOCAL_CLARITY_TRIODELAB requires INTEGRATION_LOCAL_CLARITY=1" >&2
      exit 1
    fi
    INTEGRATION_LOCAL_CLARITY_TRIODELAB_TOKEN_FILE="$secret_dir/integration_clarity_triodelab_export_token"
    require_private_file "$INTEGRATION_LOCAL_CLARITY_TRIODELAB_TOKEN_FILE" "Triodelab Clarity export token"
    triodelab_clarity_bytes=$(wc -c < "$INTEGRATION_LOCAL_CLARITY_TRIODELAB_TOKEN_FILE" | tr -d ' ')
    if [ "$triodelab_clarity_bytes" -lt 1 ] || [ "$triodelab_clarity_bytes" -gt 8192 ]; then
      echo "Triodelab Clarity token file must contain 1–8192 bytes" >&2
      exit 1
    fi
    export INTEGRATION_LOCAL_CLARITY_TRIODELAB_TOKEN_FILE
    set -- -f "$root/docker-compose.integration-clarity-triodelab.local.yml" "$@"
    ;;
  *) echo "INTEGRATION_LOCAL_CLARITY_TRIODELAB must be 0 or 1" >&2; exit 1 ;;
esac
case "${INTEGRATION_LOCAL_TRENDS:-0}" in
  0) ;;
  1)
    if [ "${INTEGRATION_LOCAL_CANONICAL:-0}" != 1 ]; then
      echo "Development Trends requires the canonical Ingestion stack" >&2; exit 1
    fi
    set -- -f "$root/docker-compose.integration-trends-dev.local.yml" "$@"
    ;;
  *) echo "INTEGRATION_LOCAL_TRENDS must be 0 or 1" >&2; exit 1 ;;
esac
if [ "${INTEGRATION_LOCAL_CANONICAL:-0}" = 1 ]; then
  case "${SHARED_NATS_ENABLED:-0}" in
    0) ;;
    1)
      SHARED_NATS_SECRETS_DIR=${SHARED_NATS_SECRETS_DIR:-"$root/.local/secrets"}
      sh "$root/scripts/shared-nats-preflight.sh" "$SHARED_NATS_SECRETS_DIR/shared_nats_ingestion_leaf_password"
      export SHARED_NATS_SECRETS_DIR
      set -- -f "$ingestion_root/docker-compose.shared-nats.yml" "$@"
      ;;
    *) echo "SHARED_NATS_ENABLED must be 0 or 1" >&2; exit 1 ;;
  esac
  cd "$ingestion_root"
  exec docker compose -f "$ingestion_root/docker-compose.yml" -f "$ingestion_root/docker-compose.engagement-local.yml" "$@"
fi
if [ "${INTEGRATION_LOCAL_PUBLISH:-0}" = 1 ]; then
  if [ "${INTEGRATION_LOCAL_HOST_PORT:-3026}" != 3026 ]; then
    echo "The published OAuth callback must use registered port 3026" >&2
    exit 1
  fi
  exec docker compose -f ./docker-compose.integration-persistent.local.yml -f ./docker-compose.integration-persistent.local.public.yml "$@"
fi
exec docker compose -f ./docker-compose.integration-persistent.local.yml "$@"
