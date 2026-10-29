#!/usr/bin/env bash
set -euo pipefail

# Provision the local, least-privilege bridge credential used by documents-api
# to publish content-free Knowledge lifecycle telemetry into Application Plane
# Insights. The Application Plane creates the password in its gitignored secret
# store; this script copies only that one password into this plane's gitignored
# .secrets directory. It never prints the secret.

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
application_root=${APPLICATION_PLANE_ROOT:-"$root/../Application Plane"}
application_secrets=${APPLICATION_PLANE_SECRETS_FILE:-"$application_root/.env.generated-secrets"}
target=${KNOWLEDGE_OBSERVABILITY_ENV_FILE:-"$root/.secrets/knowledge-observability.env"}

[[ -r "$application_secrets" ]] || {
  echo "Application Plane secrets are unavailable at $application_secrets. Run its local stack once first." >&2
  exit 1
}

password=$(awk -F= '$1 == "APPLICATION_DATA_PLANE_KNOWLEDGE_NATS_PASSWORD" { print substr($0, index($0, "=") + 1); exit }' "$application_secrets")
[[ ${#password} -ge 32 ]] || {
  echo "APPLICATION_DATA_PLANE_KNOWLEDGE_NATS_PASSWORD is missing or too short. Start the Application Plane so its local secret generator provisions it." >&2
  exit 1
}

target_dir=$(dirname "$target")
umask 077
mkdir -p "$target_dir"
chmod 700 "$target_dir"
temporary=$(mktemp "$target_dir/.knowledge-observability.XXXXXX")
trap 'rm -f "$temporary"' EXIT

{
  printf '%s\n' 'KNOWLEDGE_OBSERVABILITY_NATS_URL=nats://application-nats:4222'
  printf '%s\n' 'KNOWLEDGE_OBSERVABILITY_NATS_USER=data-plane-knowledge-publisher'
  printf 'KNOWLEDGE_OBSERVABILITY_NATS_PASSWORD=%s\n' "$password"
} >"$temporary"
chmod 600 "$temporary"
mv "$temporary" "$target"
trap - EXIT

echo "Provisioned the local Knowledge observability bridge credential at $target."
