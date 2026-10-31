#!/bin/sh
# Opt-in transport federation only. Hub per-plane permissions are the positive
# subject allowlist; neither JetStream administration nor GDPR crosses the link.
set -eu
umask 077

case "${NATS_LEAF_DOMAIN:-}" in
  STUDIO) leaf_user=studio-leaf ;;
  CONTROL_SHARED) leaf_user=control-leaf ;;
  INGESTION) leaf_user=ingestion-leaf ;;
  *) echo "Unsupported NATS leaf domain" >&2; exit 78 ;;
esac
leaf_secret_file=${NATS_LEAF_SECRET_FILE:-/run/secrets/shared_nats_leaf_password}
if [ ! -f "$leaf_secret_file" ] || [ -L "$leaf_secret_file" ]; then
  echo "NATS leaf credential must be a protected regular file" >&2; exit 78
fi
leaf_mode=$(stat -c '%a' "$leaf_secret_file" 2>/dev/null || stat -f '%Lp' "$leaf_secret_file")
case "$leaf_mode" in 400|600) ;; *) echo "NATS leaf credential must have mode 0400 or 0600" >&2; exit 78 ;; esac
leaf_bytes=$(wc -c < "$leaf_secret_file" | tr -d '[:space:]')
case "$leaf_bytes" in 64|65) ;; *) echo "NATS leaf credential must contain 64 hexadecimal bytes and at most one newline" >&2; exit 78 ;; esac
leaf_password=$(cat "$leaf_secret_file")
if [ "${#leaf_password}" -ne 64 ]; then echo "NATS leaf credential must contain exactly 64 hexadecimal bytes" >&2; exit 78; fi
case "$leaf_password" in *[!0-9a-f]*|"") echo "NATS leaf credential must be hexadecimal" >&2; exit 78 ;; esac
# NATS config expands environment variables as tokens. Include the quoting in
# the environment, not in a secret-bearing CLI argument or generated config.
NATS_SHARED_LEAF_URL="\"nats-leaf://$leaf_user:$leaf_password@verevon-nats:7422\""
export NATS_SHARED_LEAF_URL
unset leaf_password

# Ingestion previously supplied its token through --auth. Keep its existing
# token authentication but remove it from process arguments in this overlay.
if [ "$NATS_LEAF_DOMAIN" = INGESTION ]; then
  : "${INGESTION_NATS_TOKEN:?INGESTION_NATS_TOKEN is required}"
  case "$INGESTION_NATS_TOKEN" in *[!A-Za-z0-9._~-]*|"") echo "Invalid Ingestion NATS token encoding" >&2; exit 78 ;; esac
  INGESTION_NATS_CONFIG_TOKEN="\"$INGESTION_NATS_TOKEN\""
  export INGESTION_NATS_CONFIG_TOKEN
fi

leaf_base=${NATS_LEAF_BASE_CONFIG:-/etc/nats/nats.conf}
leaf_extra=${NATS_LEAF_CONFIG:-/etc/nats/shared-leaf.conf}
leaf_runtime=${NATS_LEAF_RUNTIME_CONFIG:-/tmp/shared-nats-runtime.conf}
# Preserve all scoped user ACLs verbatim; fail closed if the expected legacy
# singleton JetStream/store settings drift instead of silently mis-merging.
awk '
  /^[[:space:]]*jetstream:[[:space:]]*true[[:space:]]*$/ { js++; next }
  /^[[:space:]]*store_dir:[[:space:]]*\/data(\/jetstream)?[[:space:]]*$/ { store++; next }
  { print }
  END { if (js != 1 || store != 1) exit 78 }
' "$leaf_base" > "$leaf_runtime" || { echo "NATS base config changed; refusing unsafe leaf merge" >&2; exit 78; }
cat "$leaf_extra" >> "$leaf_runtime"
exec nats-server -c "$leaf_runtime"
