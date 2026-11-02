#!/bin/sh
set -eu

root=$(CDPATH= cd "$(dirname "$0")/.." && pwd)
fixture_dir=$(mktemp -d)
trap 'rm -f "$fixture_dir/source.env" "$fixture_dir/secrets/integration_google_client_secret"; rmdir "$fixture_dir/secrets" "$fixture_dir"' EXIT INT TERM
mkdir "$fixture_dir/secrets"
chmod 700 "$fixture_dir/secrets"
printf 'GOOGLE_CLIENT_SECRET=synthetic-oauth-secret\n' > "$fixture_dir/source.env"
chmod 600 "$fixture_dir/source.env"

INTEGRATION_GOOGLE_SOURCE_ENV="$fixture_dir/source.env" \
  STUDIO_LOCAL_SECRETS_DIR="$fixture_dir/secrets" \
  sh "$root/scripts/import-integration-google-secret.sh" >/dev/null
test "$(cat "$fixture_dir/secrets/integration_google_client_secret")" = synthetic-oauth-secret
test "$(stat -f '%Lp' "$fixture_dir/secrets/integration_google_client_secret" 2>/dev/null || stat -c '%a' "$fixture_dir/secrets/integration_google_client_secret")" = 600

INTEGRATION_GOOGLE_SOURCE_ENV="$fixture_dir/source.env" \
  STUDIO_LOCAL_SECRETS_DIR="$fixture_dir/secrets" \
  sh "$root/scripts/import-integration-google-secret.sh" >/dev/null

printf 'GOOGLE_CLIENT_SECRET=different-secret\n' > "$fixture_dir/source.env"
if INTEGRATION_GOOGLE_SOURCE_ENV="$fixture_dir/source.env" \
     STUDIO_LOCAL_SECRETS_DIR="$fixture_dir/secrets" \
     sh "$root/scripts/import-integration-google-secret.sh" >/dev/null 2>&1; then
  echo "mismatched existing OAuth secret was accepted" >&2
  exit 1
fi

echo "Google client secret import contract passed"
