#!/bin/sh
# user-core entrypoint — sources shared startup logic
DB_NAME="${DB_NAME:-user_service}"
. /app/scripts/shared-entrypoint.sh
exec "$@"
