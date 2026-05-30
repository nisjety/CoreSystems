#!/bin/bash
set -e

DB_NAME="${DB_NAME:-session_core}"
DB_USER="${DB_USER:-controlplane_user}"

# Source shared entrypoint for DB provisioning if available
SHARED_ENTRYPOINT="/shared/shared-entrypoint.sh"
if [ -f "$SHARED_ENTRYPOINT" ]; then
    export DB_NAME DB_USER
    source "$SHARED_ENTRYPOINT"
fi

exec "$@"
