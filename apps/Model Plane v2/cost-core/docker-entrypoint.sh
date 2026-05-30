#!/bin/sh
set -e

echo "=============================================="
echo " cost-core v2 starting"
echo " port=${PORT:-8006}"
echo " db=${POSTGRES_DB:-cost_core_v2_db}"
echo "=============================================="

exec "$@"
