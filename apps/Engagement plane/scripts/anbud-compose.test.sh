#!/bin/sh
# Synthetic launcher contract; never contacts Docker or reads deployment secrets.
set -eu
root=$(CDPATH= cd "$(dirname "$0")/.." && pwd)
fixture=$(mktemp -d)
trap 'rm -r "$fixture"' EXIT HUP INT TERM
mkdir -p "$fixture/scripts" "$fixture/social-core" "$fixture/leads-core" "$fixture/visibility-core" "$fixture/anbud-core" "$fixture/bin" "$fixture/.local/secrets"
cp "$root/scripts/engagement-compose.sh" "$fixture/scripts/"
chmod 700 "$fixture/.local/secrets"
printf '%s\n' INTERNAL_API_KEY=synthetic-social STUDIO_SOCIAL_NATS_PASSWORD=synthetic-social-nats > "$fixture/social-core/.env"
printf '%s\n' INTERNAL_API_KEY=synthetic-leads STUDIO_LEADS_NATS_PASSWORD=synthetic-leads-nats > "$fixture/leads-core/.env"
printf '%s\n' INTERNAL_API_KEY=synthetic-marketing > "$fixture/visibility-core/.env"
printf '%s\n' INTERNAL_API_KEY=must-not-be-reused > "$fixture/anbud-core/.env"
printf '%s\n' '#!/bin/sh' 'printf "%s\n" "$*" >> "$TEST_LOG"' 'test "$STUDIO_ANBUD_INTERNAL_API_KEY" = synthetic-anbud' 'test "$STUDIO_ANBUD_DB_PASSWORD" = synthetic-anbud-db' 'test "$STUDIO_ANBUD_MIGRATOR_DB_PASSWORD" = synthetic-anbud-migrator' > "$fixture/bin/docker"
chmod 700 "$fixture/bin/docker"
export TEST_LOG="$fixture/docker.log"
export PATH="$fixture/bin:$PATH"
export STUDIO_ANBUD_ENABLED=1
unset STUDIO_LOCAL_SECRETS_DIR
if sh "$fixture/scripts/engagement-compose.sh" config --quiet 2>/dev/null; then
  echo 'Anbud must fail closed without dedicated credentials' >&2; exit 1
fi
for entry in anbud_internal_api_key:synthetic-anbud anbud_org_core_token:synthetic-anbud-org anbud_db_password:synthetic-anbud-db anbud_migrator_db_password:synthetic-anbud-migrator; do
  name=${entry%%:*}; value=${entry#*:}
  printf '%s' "$value" > "$fixture/.local/secrets/$name"
  chmod 600 "$fixture/.local/secrets/$name"
done
sh "$fixture/scripts/engagement-compose.sh" config --quiet
grep -q -- '--profile anbud' "$TEST_LOG"
if grep -q -- 'synthetic-anbud\|must-not-be-reused\|--env-file ./anbud-core/.env' "$TEST_LOG"; then
  echo 'Anbud credentials or unused template env entered command arguments' >&2; exit 1
fi
echo 'Anbud launcher contract passed'
