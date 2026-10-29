#!/bin/sh
set -eu

root=$(CDPATH= cd "$(dirname "$0")/.." && pwd)
fixture_dir=$(mktemp -d)
source_env="$fixture_dir/source.env"
secret_dir="$fixture_dir/secrets"
meta_file="$secret_dir/integration_meta_app_client_secret"
facebook_file="$secret_dir/integration_facebook_client_secret"
cleanup() {
  rm -f "$source_env" "$meta_file" "$facebook_file" "$fixture_dir/source-link.env"
  rmdir "$secret_dir" "$fixture_dir"
}
trap cleanup EXIT INT TERM
mkdir "$secret_dir"
chmod 700 "$secret_dir"

write_source() {
  printf '%s\n' 'META_APP_CLIENT_ID=123456789' 'META_APP_CLIENT_SECRET=synthetic-meta-secret' "$@" > "$source_env"
  chmod 600 "$source_env"
}

run_import() {
  INTEGRATION_META_SOURCE_ENV="$source_env" STUDIO_LOCAL_SECRETS_DIR="$secret_dir" \
    sh "$root/scripts/import-integration-meta-secrets.sh" 2>&1
}

assert_no_secrets() {
  case "$1" in
    *synthetic-meta-secret*|*synthetic-facebook-secret*)
      echo "importer printed a provider credential" >&2
      exit 1
      ;;
  esac
}

expect_failure() {
  if output=$(run_import); then
    echo "unsafe or incomplete source was accepted: $1" >&2
    exit 1
  fi
  assert_no_secrets "$output"
}

write_source
output=$(run_import)
assert_no_secrets "$output"
test "$(cat "$meta_file")" = synthetic-meta-secret
test ! -e "$facebook_file"
test "$(stat -f '%Lp' "$meta_file" 2>/dev/null || stat -c '%a' "$meta_file")" = 600
output=$(run_import)
assert_no_secrets "$output"

write_source 'FACEBOOK_CLIENT_ID=987654321' 'FACEBOOK_CLIENT_SECRET=synthetic-facebook-secret'
output=$(run_import)
assert_no_secrets "$output"
test "$(cat "$facebook_file")" = synthetic-facebook-secret
test "$(stat -f '%Lp' "$facebook_file" 2>/dev/null || stat -c '%a' "$facebook_file")" = 600
output=$(run_import)
assert_no_secrets "$output"

# Render only synthetic deployment inputs. Both provider pairs must retain
# independent file mounts and neither app secret may enter container env.
(
  export INTEGRATION_LOCAL_DB_PASSWORD=synthetic-db-password
  export INTEGRATION_LOCAL_INTERNAL_API_KEY=synthetic-internal-key
  export INTEGRATION_LOCAL_AUTH_CORE_INTERNAL_API_KEY=synthetic-auth-key
  export INTEGRATION_LOCAL_ORG_SERVICE_TOKEN=synthetic-org-token
  export INTEGRATION_LOCAL_BILLING_SERVICE_TOKEN=synthetic-billing-token
  export INTEGRATION_LOCAL_AUDIT_SERVICE_TOKEN=synthetic-audit-token
  export INTEGRATION_LOCAL_VAULT_KEY=synthetic-vault-key
  export INTEGRATION_LOCAL_WRITE_ATTESTATION_KEYS_JSON='[]'
  export INTEGRATION_LOCAL_GOOGLE_CLIENT_ID=synthetic-google-id
  export INTEGRATION_LOCAL_GOOGLE_SECRET_FILE="$meta_file"
  export INTEGRATION_LOCAL_META_APP_CLIENT_ID=123456789
  export INTEGRATION_LOCAL_META_APP_SECRET_FILE="$meta_file"
  export INTEGRATION_LOCAL_META_BUSINESS_LOGIN_STUDIO_ADS_CONFIG_ID=444444444
  export INTEGRATION_LOCAL_FACEBOOK_CLIENT_ID=987654321
  export INTEGRATION_LOCAL_FACEBOOK_SECRET_FILE="$facebook_file"
  docker compose --env-file /dev/null -f "$root/docker-compose.integration-persistent.local.yml" \
    -f "$root/docker-compose.integration-meta.local.yml" \
    -f "$root/docker-compose.integration-facebook.local.yml" config --format json |
    jq -e '
      .services."integration-api" as $service |
      $service.environment.META_APP_CLIENT_ID == "123456789"
      and $service.environment.FACEBOOK_CLIENT_ID == "987654321"
      and $service.environment.META_APP_CLIENT_SECRET == ""
      and $service.environment.FACEBOOK_CLIENT_SECRET == ""
      and $service.environment.META_APP_CLIENT_SECRET_FILE == "/run/secrets/meta_app_client_secret"
      and $service.environment.FACEBOOK_CLIENT_SECRET_FILE == "/run/secrets/facebook_client_secret"
      and $service.environment.META_BUSINESS_LOGIN_STUDIO_ADS_CONFIG_ID == "444444444"
      and $service.environment.INSTAGRAM_CLIENT_ID == null
      and ($service.secrets | map(.source) | sort) == ["facebook_client_secret", "google_client_secret", "meta_app_client_secret"]
      and .secrets.meta_app_client_secret.file != .secrets.facebook_client_secret.file
    ' >/dev/null
)

# Preflight both files before installing either: a legacy mismatch must not
# leave a new unified Meta secret behind.
rm "$meta_file"
printf '%s' differing-protected-secret > "$facebook_file"
write_source 'FACEBOOK_CLIENT_ID=987654321' 'FACEBOOK_CLIENT_SECRET=synthetic-facebook-secret'
expect_failure 'existing legacy mismatch'
test ! -e "$meta_file"
test "$(cat "$facebook_file")" = differing-protected-secret
rm "$facebook_file"

write_source 'FACEBOOK_CLIENT_ID=987654321'
expect_failure 'partial legacy pair'
test ! -e "$meta_file"
write_source 'FACEBOOK_CLIENT_SECRET=synthetic-facebook-secret'
expect_failure 'missing legacy ID'
write_source 'META_APP_CLIENT_SECRET=duplicate-secret'
expect_failure 'duplicate required key'
write_source 'FACEBOOK_CLIENT_ID=' 'FACEBOOK_CLIENT_ID=' 'FACEBOOK_CLIENT_SECRET='
expect_failure 'duplicate optional key'
write_source 'META_APP_CLIENT_ID=222222222'
expect_failure 'duplicate ID'
printf '%s\n' 'META_APP_CLIENT_ID=123456789' > "$source_env"
expect_failure 'missing unified secret'
printf '%s\n' 'META_APP_CLIENT_SECRET=synthetic-meta-secret' > "$source_env"
expect_failure 'missing unified ID'
printf '%s\n' 'META_APP_CLIENT_ID=123456789' 'META_APP_CLIENT_SECRET="synthetic-meta-secret"' > "$source_env"
expect_failure 'quoted secret'
printf '%s\n' 'META_APP_CLIENT_ID=CHANGE_ME' 'META_APP_CLIENT_SECRET=synthetic-meta-secret' > "$source_env"
expect_failure 'placeholder ID'

write_source
printf '%s' synthetic-meta-secret > "$meta_file"
chmod 644 "$meta_file"
expect_failure 'unprotected secret target'
rm "$meta_file"
chmod 644 "$source_env"
expect_failure 'unprotected source'
chmod 600 "$source_env"
chmod 755 "$secret_dir"
expect_failure 'unprotected destination directory'
chmod 700 "$secret_dir"
ln -s "$source_env" "$meta_file"
expect_failure 'symlink secret target'
rm "$meta_file"
ln -s "$source_env" "$fixture_dir/source-link.env"
if INTEGRATION_META_SOURCE_ENV="$fixture_dir/source-link.env" STUDIO_LOCAL_SECRETS_DIR="$secret_dir" \
     sh "$root/scripts/import-integration-meta-secrets.sh" >/dev/null 2>&1; then
  echo "symlink provider source was accepted" >&2
  exit 1
fi

# Deliberately duplicated dedicated Instagram fields are unrelated and ignored.
write_source 'FACEBOOK_CLIENT_ID=' 'FACEBOOK_CLIENT_SECRET=' 'INSTAGRAM_CLIENT_ID=first' 'INSTAGRAM_CLIENT_ID=second'
output=$(run_import)
assert_no_secrets "$output"
test -f "$meta_file"
test ! -e "$facebook_file"

echo "Meta provider secret import contract passed"
