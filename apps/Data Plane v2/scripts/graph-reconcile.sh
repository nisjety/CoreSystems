#!/bin/sh
# Re-drive graph extraction for org-visible documents that have no graph rows.
#
# ## Why this exists
#
# graph-index consumes `dataplane.documents.indexed` and runs ONE inference call
# per chunk — tens of seconds per document. Signed event envelopes carry a 120s
# TTL (300s max). So the queue depth times the per-document extraction time must
# stay under the TTL, which is impossible for a consumer this slow: announce a
# dozen documents and roughly the first four extract while the rest expire and
# are dropped. Measured 2026-08-26: 12 documents announced, 2 extracted, 10
# discarded.
#
# graph-index now logs those drops explicitly (naming the document to retry)
# rather than reporting them as "unauthorized", but something has to do the
# retrying. This is that something — the graph equivalent of index-engine's
# `reconcile.rs`, which re-drives `embedding_status='failed'` knowledge units.
#
# ## How it avoids the same trap
#
# One document at a time, waiting for its rows to land before announcing the
# next, so every envelope is fresh when its turn comes. Slower than a bulk
# re-announce and that is the entire point.
#
# ## Truth source
#
# A document is "extracted" iff it has at least one `graph_text_units` row.
# Extraction legitimately yields nothing for some content, so a document that
# genuinely produces no entities is retried on each run — bounded by --max.
# Only `visibility='org'` documents are eligible: graph entities are org-shared,
# so private/shared documents are excluded by design (see
# `graph-index-rs/src/store.rs::load_org_visible_chunks`).
#
# Usage:
#   scripts/graph-reconcile.sh <org-id> [--max N] [--wait-secs S] [--dry-run]
set -eu

ORG="${1:?usage: graph-reconcile.sh <org-id> [--max N] [--wait-secs S] [--dry-run]}"
shift
MAX=25
WAIT=75
DRY=0
while [ $# -gt 0 ]; do
  case "$1" in
    --max) MAX="$2"; shift 2 ;;
    --wait-secs) WAIT="$2"; shift 2 ;;
    --dry-run) DRY=1; shift ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done

PG=data-plane-v2-postgres-1
EMB=data-plane-v2-embedding-engine-1

psql_q() {
  docker exec "$PG" sh -c \
    "psql -U \"\$POSTGRES_USER\" -d \"\$POSTGRES_DB\" -t -A -c \"$1\"" 2>/dev/null \
    | tr -d ' \r' | grep -v '^$' || true
}

extracted_count() {
  psql_q "SELECT count(DISTINCT ku.document_id)
          FROM graph_text_units g
          JOIN knowledge_units ku ON ku.knowledge_id = g.knowledge_id
         WHERE ku.org_id = '$ORG';"
}

# Org-visible, live documents with chunks but no graph rows.
PENDING=$(psql_q "SELECT d.document_id
   FROM documents d
   WHERE d.org_id = '$ORG'
     AND d.visibility = 'org'
     AND d.deleted_at IS NULL
     AND EXISTS (SELECT 1 FROM knowledge_units ku
                  WHERE ku.document_id = d.document_id AND ku.org_id = d.org_id)
     AND NOT EXISTS (
           SELECT 1 FROM graph_text_units g
           JOIN knowledge_units ku2 ON ku2.knowledge_id = g.knowledge_id
          WHERE ku2.document_id = d.document_id AND ku2.org_id = d.org_id)
   ORDER BY d.document_id
   LIMIT $MAX;")

TOTAL=$(printf '%s\n' "$PENDING" | grep -c . || true)
echo "org=$ORG  already-extracted=$(extracted_count)  pending=$TOTAL (cap $MAX)"
[ "$TOTAL" -gt 0 ] || { echo "nothing to reconcile."; exit 0; }

if [ "$DRY" = "1" ]; then
  echo "dry run — would re-announce:"
  printf '  %s\n' $PENDING
  exit 0
fi

i=0
for doc in $PENDING; do
  i=$((i + 1))
  printf '[%d/%d] %s ... ' "$i" "$TOTAL" "$(echo "$doc" | cut -c1-8)"
  # MSYS_NO_PATHCONV: Git Bash rewrites a leading / in an exec'd path.
  MSYS_NO_PATHCONV=1 docker exec -e RUST_LOG=warn "$EMB" \
    backfill-reannounce --org-id "$ORG" --document-id "$doc" >/dev/null 2>&1 \
    || { echo "re-announce FAILED"; continue; }
  # Wait for this document's rows before announcing the next, so the next
  # envelope is minted fresh rather than queued behind slow work.
  waited=0
  while [ "$waited" -lt "$WAIT" ]; do
    sleep 5
    waited=$((waited + 5))
    got=$(psql_q "SELECT count(*) FROM graph_text_units g
                  JOIN knowledge_units ku ON ku.knowledge_id = g.knowledge_id
                 WHERE ku.document_id = '$doc' AND ku.org_id = '$ORG';")
    [ "${got:-0}" -gt 0 ] && break
  done
  if [ "${got:-0}" -gt 0 ]; then
    echo "ok (${got} text units, ${waited}s)"
  else
    echo "no rows after ${WAIT}s — extraction may be slow, rate-limited, or this document yields no entities"
  fi
done

echo "done. extracted documents now: $(extracted_count)"
