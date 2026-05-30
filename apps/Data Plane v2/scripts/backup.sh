#!/usr/bin/env bash
set -euo pipefail

# Data Plane v2 — backup script
# Captures Postgres dump and Qdrant snapshots to a timestamped directory.
#
# Usage:
#   ./scripts/backup.sh                       # writes to ./backups/<timestamp>/
#   BACKUP_DIR=/tmp/dpv2-backup ./scripts/backup.sh
#
# Env:
#   POSTGRES_HOST   default: localhost
#   POSTGRES_PORT   default: 5442
#   POSTGRES_USER   default: dataplane
#   POSTGRES_DB     default: dataplane
#   PGPASSWORD      required if Postgres requires auth
#   QDRANT_URL      default: http://localhost:6345

POSTGRES_HOST=${POSTGRES_HOST:-localhost}
POSTGRES_PORT=${POSTGRES_PORT:-5442}
POSTGRES_USER=${POSTGRES_USER:-dataplane}
POSTGRES_DB=${POSTGRES_DB:-dataplane}
QDRANT_URL=${QDRANT_URL:-http://localhost:6345}

TIMESTAMP=$(date -u +%Y%m%dT%H%M%SZ)
ROOT=${BACKUP_DIR:-./backups/$TIMESTAMP}
mkdir -p "$ROOT"

echo "Backup destination: $ROOT"
echo ""

# ─── Postgres ────────────────────────────────────────────────────
echo "── Postgres ──"
PG_DUMP="$ROOT/postgres.sql.gz"
pg_dump \
  -h "$POSTGRES_HOST" \
  -p "$POSTGRES_PORT" \
  -U "$POSTGRES_USER" \
  --no-owner --no-privileges --clean --if-exists \
  "$POSTGRES_DB" \
  | gzip > "$PG_DUMP"

PG_SIZE=$(du -h "$PG_DUMP" | cut -f1)
echo "  pg_dump → $PG_DUMP ($PG_SIZE)"

# ─── Qdrant collections ──────────────────────────────────────────
echo ""
echo "── Qdrant ──"
QDRANT_DIR="$ROOT/qdrant"
mkdir -p "$QDRANT_DIR"

COLLECTIONS=$(curl -s "$QDRANT_URL/collections" | python3 -c "
import sys, json
data = json.load(sys.stdin)
for c in data.get('result', {}).get('collections', []):
    print(c['name'])
" 2>/dev/null || true)

if [ -z "$COLLECTIONS" ]; then
  echo "  (no Qdrant collections found, or Qdrant not reachable at $QDRANT_URL)"
else
  for col in $COLLECTIONS; do
    echo "  collection: $col"
    SNAPSHOT_RESP=$(curl -s -X POST "$QDRANT_URL/collections/$col/snapshots")
    SNAPSHOT_NAME=$(echo "$SNAPSHOT_RESP" | python3 -c "
import sys, json
print(json.load(sys.stdin).get('result', {}).get('name', ''))
" 2>/dev/null || true)

    if [ -z "$SNAPSHOT_NAME" ]; then
      echo "    failed to create snapshot for $col"
      continue
    fi

    OUT="$QDRANT_DIR/${col}_${SNAPSHOT_NAME}"
    curl -sS -o "$OUT" "$QDRANT_URL/collections/$col/snapshots/$SNAPSHOT_NAME"
    SIZE=$(du -h "$OUT" | cut -f1)
    echo "    snapshot → $OUT ($SIZE)"
  done
fi

# ─── Manifest ────────────────────────────────────────────────────
echo ""
echo "── Manifest ──"
cat > "$ROOT/MANIFEST.txt" << EOF
Data Plane v2 backup
Created: $(date -u)
Postgres: $POSTGRES_HOST:$POSTGRES_PORT/$POSTGRES_DB
Qdrant:   $QDRANT_URL

Files:
$(cd "$ROOT" && find . -type f -not -name MANIFEST.txt | sort | sed 's|^\./|  |')

Restore: see ./scripts/restore.sh "$ROOT"
EOF

cat "$ROOT/MANIFEST.txt"
echo ""
echo "Backup complete: $ROOT"
