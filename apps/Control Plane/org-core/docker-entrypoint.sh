#!/bin/sh
# org-core entrypoint — sources shared startup logic
DB_NAME="${DB_NAME:-org_core}"
. /app/scripts/shared-entrypoint.sh
exec "$@"
