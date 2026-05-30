#!/usr/bin/env bash
set -euo pipefail

# Data Plane v2 — restore script
# Restores Postgres dump and Qdrant snapshots from a backup directory.
#
# Usage:
#   ./scripts/restore.sh ./backups/<timestamp>
#
# WARNING: This script overwrites the target Postgres database and Qdrant
# collections. Confirm before running against production.

if [ $# -lt 1 ]; then
  echo "usage: $0 <backup-dir>" >&2
  exit 2
fi

BACKUP_DIR=$1
if [ ! -d "$BACKUP_DIR" ]; then
  echo "backup dir not found: $BACKUP_DIR" >&2
  exit 1
fi

POSTGRES_HOST=${POSTGRES_HOST:-localhost}
POSTGRES_PORT=${POSTGRES_PORT:-5442}
POSTGRES_USER=${POSTGRES_USER:-dataplane}
POSTGRES_DB=${POSTGRES_DB:-dataplane}
QDRANT_URL=${QDRANT_URL:-http://localhost:6345}

echo "═══════════════════════════════════════════════"
echo " Data Plane v2 — RESTORE"
echo "═══════════════════════════════════════════════"
echo " Source:    $BACKUP_DIR"
echo " Postgres:  $POSTGRES_HOST:$POSTGRES_PORT/$POSTGRES_DB"
echo " Qdrant:    $QDRANT_URL"
echo "═══════════════════════════════════════════════"
echo ""
echo "This will OVERWRITE the target database and Qdrant collections."
read -p "Type 'restore' to continue: " confirm
if [ "$confirm" != "restore" ]; then
  echo "Aborted."
  exit 1
fi

# ─── Postgres ────────────────────────────────────────────────────
PG_DUMP="$BACKUP_DIR/postgres.sql.gz"
if [ ! -f "$PG_DUMP" ]; then
  echo "missing Postgres dump: $PG_DUMP" >&2
  exit 1
fi

echo ""
echo "── Postgres restore ──"
gunzip -c "$PG_DUMP" | psql \
  -h "$POSTGRES_HOST" \
  -p "$POSTGRES_PORT" \
  -U "$POSTGRES_USER" \
  -d "$POSTGRES_DB" \
  --quiet \
  -v ON_ERROR_STOP=1
echo "  done"

# ─── Qdrant ──────────────────────────────────────────────────────
QDRANT_DIR="$BACKUP_DIR/qdrant"
if [ -d "$QDRANT_DIR" ]; then
  echo ""
  echo "── Qdrant restore ──"
  for snap in "$QDRANT_DIR"/*; do
    [ -f "$snap" ] || continue
    fname=$(basename "$snap")
    # filename format: <collection>_<snapshot_name>
    collection=${fname%%_*}

    echo "  uploading $fname → $collection"
    curl -sS -X POST "$QDRANT_URL/collections/$collection/snapshots/upload" \
      -H "Content-Type: multipart/form-data" \
      -F "snapshot=@$snap" \
      | python3 -m json.tool 2>/dev/null || echo "  (upload failed for $fname)"
  done
fi

echo ""
echo "Restore complete."
echo ""
echo "Recommended next steps:"
echo "  1. make migrate-status   # verify schema versions"
echo "  2. make smoke            # health-check all services"
echo "  3. Sample retrieval to validate vectors loaded"
