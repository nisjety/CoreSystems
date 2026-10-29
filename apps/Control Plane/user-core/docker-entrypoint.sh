#!/bin/sh
# user-core entrypoint — sources shared startup logic

# Docker Compose file-backed secrets retain the source file's ownership and
# mode. The production override starts this entrypoint as root only long enough
# to copy each mounted secret into an app-owned private directory, then drops
# privileges before migrations or the service binary run.
if [ "$(id -u)" -eq 0 ]; then
  set -eu
  mkdir -p /run/control-secrets
  chown appuser:appgroup /run/control-secrets
  chmod 0700 /run/control-secrets
  for variable in \
    USER_CORE_GRPC_SERVICE_CREDENTIALS_FILE \
    USER_CORE_GRPC_TLS_CERT_FILE \
    USER_CORE_GRPC_TLS_KEY_FILE \
    USER_AUTH_INTERNAL_CLIENT_CREDENTIAL_FILE \
    AUTH_CORE_JWT_PUBLIC_KEY_FILE; do
    eval "source=\${$variable:-}"
    case "$source" in
      /run/secrets/*)
        [ -f "$source" ] || { echo "$variable secret is not a regular file" >&2; exit 1; }
        target="/run/control-secrets/$(basename "$source")"
        cp -- "$source" "$target"
        chown appuser:appgroup "$target"
        chmod 0600 "$target"
        export "$variable=$target"
        ;;
      "")
        ;;
    esac
  done
  export CONTROL_SECRET_HANDOFF_DONE=1
  exec /sbin/su-exec appuser "$0" "$@"
fi

DB_NAME="${DB_NAME:-user_service}"
. /app/scripts/shared-entrypoint.sh
exec "$@"
