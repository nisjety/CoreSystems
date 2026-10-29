#!/bin/sh
# Project env files are used for Compose interpolation. Only explicitly named
# environment variables enter shared containers (notably the NATS broker).
set -eu

STUDIO_COMPOSE_DIR=$(CDPATH= cd "$(dirname "$0")/.." && pwd)
cd "$STUDIO_COMPOSE_DIR"

# The core files contain shared-template fields that their binaries do not
# read. Select only the few values Compose must inject, without sourcing the
# files as shell code or copying secrets into a generated file. We deliberately
# accept only unique, unquoted literal values for projected keys: this avoids
# silently disagreeing with Compose's richer dotenv interpolation rules.
studio_env_value() {
  awk -v wanted="$2" '
    {
      separator = index($0, "=")
      if (separator == 0) next
      key = substr($0, 1, separator - 1)
      gsub(/^[[:space:]]+|[[:space:]]+$/, "", key)
      if (key != wanted) next
      value = substr($0, separator + 1)
      sub(/\r$/, "", value)
      count++
      if (value ~ /[[:space:]#$\\]/ || index(value, "\"") || index(value, "\047")) invalid = 1
      result = value
    }
    END {
      if (count > 1 || invalid) {
        print FILENAME ": " wanted " must be a unique, unquoted literal value" > "/dev/stderr"
        exit 2
      }
      if (count) print result
    }
  ' "$1"
}

# These values are inserted into a PostgreSQL URL or into quoted NATS config.
# Restrict them to URL/config-safe characters rather than allowing a password
# to alter the surrounding syntax. The error names only the setting.
studio_require_safe_component() {
  case "$2" in
    ""|*[!A-Za-z0-9._~-]*)
      echo "$1 must use only letters, digits, dot, underscore, tilde, or hyphen" >&2
      exit 1
      ;;
  esac
}

studio_local_secret() {
  studio_local_secret_file=${STUDIO_LOCAL_SECRETS_DIR:-./.local/secrets}/$1
  if [ ! -e "$studio_local_secret_file" ]; then
    return 0
  fi
  if [ ! -f "$studio_local_secret_file" ] || [ -L "$studio_local_secret_file" ]; then
    echo "$1 must be a regular local secret file" >&2
    exit 1
  fi
  studio_local_secret_mode=$(stat -f '%Lp' "$studio_local_secret_file" 2>/dev/null || stat -c '%a' "$studio_local_secret_file")
  case "$studio_local_secret_mode" in
    400|600) ;;
    *) echo "$1 must have mode 0400 or 0600" >&2; exit 1 ;;
  esac
  studio_local_secret_value=$(cat "$studio_local_secret_file")
  studio_require_safe_component "$1" "$studio_local_secret_value"
  printf '%s' "$studio_local_secret_value"
}

# Shell variables outrank --env-file in Compose. Refuse conflicting exports so
# one core cannot silently inherit another core's credential.
if [ "${INTERNAL_API_KEY+x}" = x ] ||
   [ "${STUDIO_SOCIAL_NATS_PASSWORD+x}" = x ] ||
   [ "${STUDIO_LEADS_NATS_PASSWORD+x}" = x ] ||
   [ "${APPLICATION_SOCIAL_NATS_PASSWORD+x}" = x ] ||
   [ "${APPLICATION_LEADS_NATS_PASSWORD+x}" = x ]; then
  echo "Unset core-local credential exports; this launcher reads each core's .env." >&2
  exit 1
fi

if grep -Eq '^[[:space:]]*(STUDIO_PLANE_DB_PASSWORD|STUDIO_NATS_PROVISIONER_PASSWORD)=' \
  ./social-core/.env ./leads-core/.env ./visibility-core/.env; then
  echo "Place infrastructure passwords in the approved deployment environment, not a core .env" >&2
  exit 1
fi

STUDIO_SOCIAL_INTERNAL_API_KEY=$(studio_env_value ./social-core/.env INTERNAL_API_KEY)
STUDIO_LEADS_INTERNAL_API_KEY=$(studio_env_value ./leads-core/.env INTERNAL_API_KEY)
STUDIO_MARKETING_INTERNAL_API_KEY=$(studio_env_value ./visibility-core/.env INTERNAL_API_KEY)
studio_secret_override=$(studio_local_secret social_internal_api_key)
if [ -n "$studio_secret_override" ]; then STUDIO_SOCIAL_INTERNAL_API_KEY=$studio_secret_override; fi
studio_secret_override=$(studio_local_secret leads_internal_api_key)
if [ -n "$studio_secret_override" ]; then STUDIO_LEADS_INTERNAL_API_KEY=$studio_secret_override; fi
studio_secret_override=$(studio_local_secret marketing_internal_api_key)
if [ -n "$studio_secret_override" ]; then STUDIO_MARKETING_INTERNAL_API_KEY=$studio_secret_override; fi
STUDIO_VISIBILITY_ORG_CORE_TOKEN=$(studio_local_secret visibility_org_core_token)
: "${STUDIO_SOCIAL_INTERNAL_API_KEY:?social-core/.env INTERNAL_API_KEY is empty}"
: "${STUDIO_LEADS_INTERNAL_API_KEY:?leads-core/.env INTERNAL_API_KEY is empty}"
: "${STUDIO_MARKETING_INTERNAL_API_KEY:?visibility-core/.env INTERNAL_API_KEY is empty}"
STUDIO_LEADS_BILLING_CORE_URL=$(studio_env_value ./leads-core/.env BILLING_CORE_URL)
STUDIO_LEADS_BILLING_SERVICE_ID=$(studio_env_value ./leads-core/.env LEADS_BILLING_SERVICE_ID)
STUDIO_LEADS_BILLING_SERVICE_TOKEN=$(studio_env_value ./leads-core/.env LEADS_BILLING_SERVICE_TOKEN)
if [ -n "$STUDIO_LEADS_BILLING_CORE_URL" ] ||
   [ -n "$STUDIO_LEADS_BILLING_SERVICE_ID" ] ||
   [ -n "$STUDIO_LEADS_BILLING_SERVICE_TOKEN" ]; then
  if [ -z "$STUDIO_LEADS_BILLING_CORE_URL" ] ||
     [ -z "$STUDIO_LEADS_BILLING_SERVICE_ID" ] ||
     [ -z "$STUDIO_LEADS_BILLING_SERVICE_TOKEN" ]; then
    STUDIO_LEADS_BILLING_CORE_URL=
    STUDIO_LEADS_BILLING_SERVICE_ID=
    STUDIO_LEADS_BILLING_SERVICE_TOKEN=
    echo "Leads billing is incomplete; gated lead actions remain unavailable" >&2
  fi
fi
STUDIO_SOCIAL_NATS_PASSWORD=$(studio_env_value ./social-core/.env STUDIO_SOCIAL_NATS_PASSWORD)
STUDIO_LEADS_NATS_PASSWORD=$(studio_env_value ./leads-core/.env STUDIO_LEADS_NATS_PASSWORD)
if [ "${STUDIO_PLANE_DB_PASSWORD+x}" != x ]; then
  STUDIO_PLANE_DB_PASSWORD=$(studio_local_secret studio_db_password)
fi
if [ "${STUDIO_PLANE_DRAGONFLY_PASSWORD+x}" != x ]; then
  STUDIO_PLANE_DRAGONFLY_PASSWORD=$(studio_local_secret studio_dragonfly_password)
fi
if [ "${STUDIO_NATS_PROVISIONER_PASSWORD+x}" != x ]; then
  STUDIO_NATS_PROVISIONER_PASSWORD=$(studio_local_secret studio_nats_provisioner_password)
fi
if [ "${GRAFANA_ADMIN_PASSWORD+x}" != x ]; then
  GRAFANA_ADMIN_PASSWORD=$(studio_local_secret studio_grafana_password)
fi
studio_require_safe_component STUDIO_SOCIAL_NATS_PASSWORD "$STUDIO_SOCIAL_NATS_PASSWORD"
studio_require_safe_component STUDIO_LEADS_NATS_PASSWORD "$STUDIO_LEADS_NATS_PASSWORD"
if [ -n "$STUDIO_PLANE_DB_PASSWORD" ]; then
  studio_require_safe_component STUDIO_PLANE_DB_PASSWORD "$STUDIO_PLANE_DB_PASSWORD"
fi
for studio_url_component_name in STUDIO_PLANE_DB_USER STUDIO_PLANE_DB_NAME NOTIFICATION_DB_USER NOTIFICATION_DB_NAME; do
  case "$studio_url_component_name" in
    STUDIO_PLANE_DB_USER) studio_url_component_set=${STUDIO_PLANE_DB_USER+x}; studio_url_component_value=${STUDIO_PLANE_DB_USER-} ;;
    STUDIO_PLANE_DB_NAME) studio_url_component_set=${STUDIO_PLANE_DB_NAME+x}; studio_url_component_value=${STUDIO_PLANE_DB_NAME-} ;;
    NOTIFICATION_DB_USER) studio_url_component_set=${NOTIFICATION_DB_USER+x}; studio_url_component_value=${NOTIFICATION_DB_USER-} ;;
    NOTIFICATION_DB_NAME) studio_url_component_set=${NOTIFICATION_DB_NAME+x}; studio_url_component_value=${NOTIFICATION_DB_NAME-} ;;
  esac
  if [ "$studio_url_component_set" = x ]; then
    studio_require_safe_component "$studio_url_component_name" "$studio_url_component_value"
  fi
done
if [ -n "$STUDIO_NATS_PROVISIONER_PASSWORD" ]; then
  studio_require_safe_component STUDIO_NATS_PROVISIONER_PASSWORD "$STUDIO_NATS_PROVISIONER_PASSWORD"
fi
export STUDIO_SOCIAL_INTERNAL_API_KEY STUDIO_LEADS_INTERNAL_API_KEY STUDIO_MARKETING_INTERNAL_API_KEY STUDIO_VISIBILITY_ORG_CORE_TOKEN
export STUDIO_LEADS_BILLING_CORE_URL STUDIO_LEADS_BILLING_SERVICE_ID STUDIO_LEADS_BILLING_SERVICE_TOKEN
export STUDIO_SOCIAL_NATS_PASSWORD STUDIO_LEADS_NATS_PASSWORD
export STUDIO_PLANE_DB_PASSWORD STUDIO_PLANE_DRAGONFLY_PASSWORD STUDIO_NATS_PROVISIONER_PASSWORD GRAFANA_ADMIN_PASSWORD

# Anbud has an independent database/runtime identity. Never infer these from
# generic shared-template DATABASE_URL or INTERNAL_API_KEY settings.
case "${STUDIO_ANBUD_ENABLED:-0}" in
  0) ;;
  1)
    STUDIO_ANBUD_INTERNAL_API_KEY=$(studio_local_secret anbud_internal_api_key)
    STUDIO_ANBUD_ORG_CORE_TOKEN=$(studio_local_secret anbud_org_core_token)
    STUDIO_ANBUD_DB_PASSWORD=$(studio_local_secret anbud_db_password)
    STUDIO_ANBUD_MIGRATOR_DB_PASSWORD=$(studio_local_secret anbud_migrator_db_password)
    studio_require_safe_component STUDIO_ANBUD_INTERNAL_API_KEY "$STUDIO_ANBUD_INTERNAL_API_KEY"
    studio_require_safe_component STUDIO_ANBUD_ORG_CORE_TOKEN "$STUDIO_ANBUD_ORG_CORE_TOKEN"
    studio_require_safe_component STUDIO_ANBUD_DB_PASSWORD "$STUDIO_ANBUD_DB_PASSWORD"
    studio_require_safe_component STUDIO_ANBUD_MIGRATOR_DB_PASSWORD "$STUDIO_ANBUD_MIGRATOR_DB_PASSWORD"
    if [ "$STUDIO_ANBUD_DB_PASSWORD" = "$STUDIO_ANBUD_MIGRATOR_DB_PASSWORD" ] ||
       [ "$STUDIO_ANBUD_INTERNAL_API_KEY" = "$STUDIO_SOCIAL_INTERNAL_API_KEY" ] ||
       [ "$STUDIO_ANBUD_INTERNAL_API_KEY" = "$STUDIO_LEADS_INTERNAL_API_KEY" ] ||
       [ "$STUDIO_ANBUD_INTERNAL_API_KEY" = "$STUDIO_MARKETING_INTERNAL_API_KEY" ]; then
      echo "Anbud requires distinct runtime, migration and core credentials" >&2
      exit 1
    fi
    export STUDIO_ANBUD_INTERNAL_API_KEY STUDIO_ANBUD_ORG_CORE_TOKEN STUDIO_ANBUD_DB_PASSWORD STUDIO_ANBUD_MIGRATOR_DB_PASSWORD
    set -- --profile anbud "$@"
    ;;
  *) echo "STUDIO_ANBUD_ENABLED must be 0 or 1" >&2; exit 1 ;;
esac

studio_validate_startup() {
  if [ "$STUDIO_SOCIAL_INTERNAL_API_KEY" = "$STUDIO_LEADS_INTERNAL_API_KEY" ] ||
     [ "$STUDIO_SOCIAL_INTERNAL_API_KEY" = "$STUDIO_MARKETING_INTERNAL_API_KEY" ] ||
     [ "$STUDIO_LEADS_INTERNAL_API_KEY" = "$STUDIO_MARKETING_INTERNAL_API_KEY" ]; then
    echo "Social, Leads, and Marketing need distinct INTERNAL_API_KEY values before startup" >&2
    exit 1
  fi
  if [ "$STUDIO_SOCIAL_NATS_PASSWORD" = "$STUDIO_LEADS_NATS_PASSWORD" ] ||
     { [ "${STUDIO_NATS_PROVISIONER_PASSWORD+x}" = x ] &&
       { [ "$STUDIO_SOCIAL_NATS_PASSWORD" = "$STUDIO_NATS_PROVISIONER_PASSWORD" ] ||
         [ "$STUDIO_LEADS_NATS_PASSWORD" = "$STUDIO_NATS_PROVISIONER_PASSWORD" ]; }; }; then
    echo "Social, Leads, and the NATS provisioner need distinct NATS passwords before startup" >&2
    exit 1
  fi
}

# Compose declares the cross-plane bridge as external. Provision it only for
# commands that create containers; read-only config/build commands stay inert.
for studio_compose_arg in "$@"; do
  case "$studio_compose_arg" in
    up|run|create)
      studio_validate_startup
      if ! docker network inspect inter-plane-bus >/dev/null 2>&1; then
        docker network create --driver bridge \
          --label com.docker.compose.network=inter-plane-bus \
          --label com.docker.compose.project=triodelab \
          inter-plane-bus >/dev/null ||
          docker network inspect inter-plane-bus >/dev/null
      fi
      break
      ;;
    start|restart)
      studio_validate_startup
      break
      ;;
  esac
done

# Authorization is mounted independently for each consumer. Provider OAuth
# credentials never enter a Studio container; the shared broker owns them.
studio_integration_authorization() {
  if [ -z "$2" ] || [ -z "$3" ]; then
    echo "$1 Integration bearer file and organization/connection must be configured together" >&2
    exit 1
  fi
  if [ ! -f "$2" ] || [ -L "$2" ]; then
    echo "$1 Integration bearer must reference a regular protected file" >&2
    exit 1
  fi
  studio_bearer_mode=$(stat -f '%Lp' "$2" 2>/dev/null || stat -c '%a' "$2")
  case "$studio_bearer_mode" in
    400|600) ;;
    *) echo "$1 Integration bearer must have mode 0400 or 0600" >&2; exit 1 ;;
  esac
  studio_bearer_bytes=$(wc -c < "$2")
  if [ "$studio_bearer_bytes" -lt 1 ] || [ "$studio_bearer_bytes" -gt 8192 ]; then
    echo "$1 Integration bearer file must contain at most 8 KiB" >&2
    exit 1
  fi
}

studio_marketing_live=0
studio_marketing_live_file=${STUDIO_MARKETING_LIVE_CONFIG_FILE:-./.local/marketing-integration.json}
if [ -e "$studio_marketing_live_file" ] || [ -L "$studio_marketing_live_file" ]; then
  if [ ! -f "$studio_marketing_live_file" ] || [ -L "$studio_marketing_live_file" ]; then
    echo "Saved Marketing integration must be a protected regular JSON file" >&2
    exit 1
  fi
  studio_live_mode=$(stat -f '%Lp' "$studio_marketing_live_file" 2>/dev/null || stat -c '%a' "$studio_marketing_live_file")
  if [ "$studio_live_mode" != 600 ] || ! jq -e '
    .version == 1
    and (.organizationId | type == "string" and test("^[A-Za-z0-9_-]{1,128}$"))
    and (.googleConnectionId | type == "string" and test("^[A-Za-z0-9_-]{1,128}$"))
  ' "$studio_marketing_live_file" >/dev/null 2>&1; then
    echo "Saved Marketing integration has invalid version, organization, or connection" >&2
    exit 1
  fi
  studio_live_connection=$(jq -er '.googleConnectionId' "$studio_marketing_live_file")
  if [ -n "${STUDIO_MARKETING_INTEGRATION_BEARER_FILE:-}" ] ||
     { [ -n "${STUDIO_MARKETING_GOOGLE_CONNECTION_ID:-}" ] && [ "$STUDIO_MARKETING_GOOGLE_CONNECTION_ID" != "$studio_live_connection" ]; }; then
    echo "Saved managed Marketing integration conflicts with an external bearer or connection" >&2
    exit 1
  fi
  STUDIO_MARKETING_INTEGRATION_ORGANIZATION_ID=$(jq -er '.organizationId' "$studio_marketing_live_file")
  STUDIO_MARKETING_GOOGLE_CONNECTION_ID=$studio_live_connection
  STUDIO_MARKETING_DATA_VOLUME=$(jq -er '.dataVolume // "engagement-plane_marketing-websites"' "$studio_marketing_live_file")
  case "$STUDIO_MARKETING_DATA_VOLUME" in
    engagement-plane_marketing-*) ;;
    *) echo "Marketing data volume must be an explicit Engagement Marketing volume" >&2; exit 1 ;;
  esac
  studio_require_safe_component STUDIO_MARKETING_DATA_VOLUME "$STUDIO_MARKETING_DATA_VOLUME"
  export STUDIO_MARKETING_DATA_VOLUME
  STUDIO_MARKETING_SERVICE_KEY_FILE=${STUDIO_MARKETING_SERVICE_KEY_FILE:-"$STUDIO_COMPOSE_DIR/.local/secrets/marketing_plane_service_key"}
  studio_integration_authorization Marketing "$STUDIO_MARKETING_SERVICE_KEY_FILE" "$STUDIO_MARKETING_INTEGRATION_ORGANIZATION_ID"
  export STUDIO_MARKETING_INTEGRATION_ORGANIZATION_ID STUDIO_MARKETING_GOOGLE_CONNECTION_ID STUDIO_MARKETING_SERVICE_KEY_FILE
  studio_marketing_live=1
  set -- -f ./docker-compose.marketing-bearer.studio.yml -f ./docker-compose.marketing-live.studio.yml "$@"
fi

studio_plane_links=${STUDIO_PLANE_LINKS:-0}
if [ "$studio_plane_links" = 1 ]; then
  if [ "$studio_marketing_live" != 1 ]; then
    echo "Studio plane links require the saved fixed-organization Marketing configuration" >&2
    exit 1
  fi
  if [ -n "${STUDIO_ORGANIZATION_ID:-}" ] && [ "$STUDIO_ORGANIZATION_ID" != "$STUDIO_MARKETING_INTEGRATION_ORGANIZATION_ID" ]; then
    echo "Studio plane links must use the fixed Marketing workspace organization" >&2
    exit 1
  fi
  if [ -n "${STUDIO_SOCIAL_INTEGRATION_BEARER_FILE:-}${STUDIO_LEADS_INTEGRATION_BEARER_FILE:-}" ]; then
    echo "Managed Studio plane links cannot be mixed with external provider bearers" >&2
    exit 1
  fi
  STUDIO_ORGANIZATION_ID=$STUDIO_MARKETING_INTEGRATION_ORGANIZATION_ID
  export STUDIO_ORGANIZATION_ID
  for studio_link_core in marketing social leads anbud; do
    studio_link_key=${STUDIO_LOCAL_SECRETS_DIR:-"$STUDIO_COMPOSE_DIR/.local/secrets"}/${studio_link_core}_plane_service_key
    studio_integration_authorization "$studio_link_core plane principal" "$studio_link_key" "$STUDIO_ORGANIZATION_ID"
  done
  set -- -f ./docker-compose.plane-links.studio.yml "$@"
elif [ "$studio_plane_links" != 0 ]; then
  echo "STUDIO_PLANE_LINKS must be 0 or 1" >&2
  exit 1
fi

if [ -n "${STUDIO_SOCIAL_INTEGRATION_BEARER_FILE:-}${STUDIO_SOCIAL_INTEGRATION_ORGANIZATION_ID:-}" ]; then
  studio_integration_authorization Social "${STUDIO_SOCIAL_INTEGRATION_BEARER_FILE:-}" "${STUDIO_SOCIAL_INTEGRATION_ORGANIZATION_ID:-}"
  set -- -f ./docker-compose.social-integrations.studio.yml "$@"
fi
if [ -n "${STUDIO_LEADS_INTEGRATION_BEARER_FILE:-}${STUDIO_LEADS_INTEGRATION_ORGANIZATION_ID:-}" ]; then
  studio_integration_authorization Leads "${STUDIO_LEADS_INTEGRATION_BEARER_FILE:-}" "${STUDIO_LEADS_INTEGRATION_ORGANIZATION_ID:-}"
  set -- -f ./docker-compose.leads-integrations.studio.yml "$@"
fi
if [ "$studio_marketing_live" != 1 ] && [ -n "${STUDIO_MARKETING_INTEGRATION_BEARER_FILE:-}${STUDIO_MARKETING_GOOGLE_CONNECTION_ID:-}${STUDIO_MARKETING_GSC_PROPERTY:-}" ]; then
  studio_integration_authorization Marketing "${STUDIO_MARKETING_INTEGRATION_BEARER_FILE:-}" "${STUDIO_MARKETING_GOOGLE_CONNECTION_ID:-}"
  set -- -f ./docker-compose.marketing-gsc.studio.yml "$@"
fi

case "${STUDIO_WEB_VITALS_ENABLED:-0}" in
  0) ;;
  1)
    STUDIO_WEB_VITALS_DRAGONFLY_CONFIG_FILE=${STUDIO_LOCAL_SECRETS_DIR:-"$STUDIO_COMPOSE_DIR/.local/secrets"}/web_vitals_dragonfly_config
    if [ ! -f "$STUDIO_WEB_VITALS_DRAGONFLY_CONFIG_FILE" ] || [ -L "$STUDIO_WEB_VITALS_DRAGONFLY_CONFIG_FILE" ]; then
      echo "Web Vitals Dragonfly requires a protected regular config file" >&2
      exit 1
    fi
    studio_vitals_mode=$(stat -f '%Lp' "$STUDIO_WEB_VITALS_DRAGONFLY_CONFIG_FILE" 2>/dev/null || stat -c '%a' "$STUDIO_WEB_VITALS_DRAGONFLY_CONFIG_FILE")
    if [ "$studio_vitals_mode" != 600 ] || ! awk '
      NR != 1 || $0 !~ /^--requirepass=[A-Za-z0-9_-]+$/ || length($0) < 46 || length($0) > 270 { bad=1 }
      END { exit (bad || NR != 1) }
    ' "$STUDIO_WEB_VITALS_DRAGONFLY_CONFIG_FILE"; then
      echo "Web Vitals Dragonfly config must be mode 0600 with one dedicated password flag" >&2
      exit 1
    fi
    export STUDIO_WEB_VITALS_DRAGONFLY_CONFIG_FILE
    set -- -f ./docker-compose.web-vitals.studio.yml "$@"
    ;;
  *) echo "STUDIO_WEB_VITALS_ENABLED must be 0 or 1" >&2; exit 1 ;;
esac

# Reuse an explicitly provisioned local website installation on later starts.
STUDIO_WEBSITE_INQUIRIES_DIR=${STUDIO_LOCAL_SECRETS_DIR:-"$STUDIO_COMPOSE_DIR/.local/secrets"}/website-inquiries
studio_inquiries_default=0
if [ -f "$STUDIO_WEBSITE_INQUIRIES_DIR/bindings.json" ]; then studio_inquiries_default=1; fi
case "${STUDIO_WEBSITE_INQUIRIES_ENABLED:-$studio_inquiries_default}" in
  0) ;;
  1)
    for studio_inquiry_file in bindings.json triodelab_write_token triodelab_read_token; do
      studio_inquiry_path=$STUDIO_WEBSITE_INQUIRIES_DIR/$studio_inquiry_file
      if [ ! -f "$studio_inquiry_path" ] || [ -L "$studio_inquiry_path" ]; then
        echo "Website inquiries require protected installation files" >&2; exit 1
      fi
      studio_inquiry_mode=$(stat -f '%Lp' "$studio_inquiry_path" 2>/dev/null || stat -c '%a' "$studio_inquiry_path")
      if [ "$studio_inquiry_mode" != 600 ]; then echo "Website installation files must use mode 0600" >&2; exit 1; fi
    done
    export STUDIO_WEBSITE_INQUIRIES_DIR
    set -- -f ./docker-compose.website-inquiries.studio.yml "$@"
    ;;
  *) echo "STUDIO_WEBSITE_INQUIRIES_ENABLED must be 0 or 1" >&2; exit 1 ;;
esac

case "${SHARED_NATS_ENABLED:-0}" in
  0) ;;
  1)
    SHARED_NATS_SECRETS_DIR=${SHARED_NATS_SECRETS_DIR:-"$STUDIO_COMPOSE_DIR/.local/secrets"}
    sh "$STUDIO_COMPOSE_DIR/scripts/shared-nats-preflight.sh" "$SHARED_NATS_SECRETS_DIR/shared_nats_studio_leaf_password"
    export SHARED_NATS_SECRETS_DIR
    set -- -f ./docker-compose.shared-nats.yml "$@"
    ;;
  *) echo "SHARED_NATS_ENABLED must be 0 or 1" >&2; exit 1 ;;
esac

exec docker compose \
  --env-file ./social-core/.env \
  --env-file ./leads-core/.env \
  --env-file ./visibility-core/.env \
  -f ./docker-compose.yml "$@"
