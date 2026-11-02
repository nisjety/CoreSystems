#!/bin/sh
set -eu

studio_test_root=$(CDPATH= cd "$(dirname "$0")/.." && pwd)
studio_test_dir=$(mktemp -d)
trap 'rm -r "$studio_test_dir"' EXIT HUP INT TERM
studio_test_secrets="$studio_test_dir/secrets"

STUDIO_LOCAL_SECRETS_DIR="$studio_test_secrets" "$studio_test_root/scripts/provision-local-secrets.sh"
test "$(stat -f '%Lp' "$studio_test_secrets")" = 700
for studio_secret_name in \
  social_internal_api_key leads_internal_api_key marketing_internal_api_key \
  anbud_internal_api_key anbud_db_password anbud_migrator_db_password \
  shared_nats_studio_leaf_password shared_nats_control_leaf_password shared_nats_ingestion_leaf_password \
  studio_db_password studio_dragonfly_password studio_nats_provisioner_password \
  studio_grafana_password; do
  studio_secret_file="$studio_test_secrets/$studio_secret_name"
  test -s "$studio_secret_file"
  test "$(stat -f '%Lp' "$studio_secret_file")" = 600
  cp "$studio_secret_file" "$studio_test_dir/$studio_secret_name.before"
done

STUDIO_LOCAL_SECRETS_DIR="$studio_test_secrets" "$studio_test_root/scripts/provision-local-secrets.sh"
for studio_secret_name in \
  social_internal_api_key leads_internal_api_key marketing_internal_api_key \
  anbud_internal_api_key anbud_db_password anbud_migrator_db_password \
  shared_nats_studio_leaf_password shared_nats_control_leaf_password shared_nats_ingestion_leaf_password \
  studio_db_password studio_dragonfly_password studio_nats_provisioner_password \
  studio_grafana_password; do
  cmp -s "$studio_test_secrets/$studio_secret_name" "$studio_test_dir/$studio_secret_name.before"
done

echo "local secret provisioning contract: ok"
