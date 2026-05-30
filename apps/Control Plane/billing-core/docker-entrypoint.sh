#!/bin/sh
# billing-core entrypoint — sources shared startup logic
DB_NAME="${DB_NAME:-billing_service}"
. /app/scripts/shared-entrypoint.sh
exec "$@"
