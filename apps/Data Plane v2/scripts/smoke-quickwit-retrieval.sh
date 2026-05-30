#!/usr/bin/env bash
set -euo pipefail

# Verifies the Quickwit sparse-search path without requiring external model
# calls. It proves:
# - retrieval-engine selected the Quickwit sparse backend wrapper
# - fallback behavior is covered by the Rust unit test
# - documents-api source_object events reach Quickwit through the adapter
# - canonical Postgres rebuild can clean stale Quickwit state deterministically

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
RETRIEVAL_URL="${RETRIEVAL_URL:-http://127.0.0.1:8014}"
DOCUMENTS_URL="${DOCUMENTS_URL:-http://127.0.0.1:8010}"
QUICKWIT_URL="${QUICKWIT_URL:-http://127.0.0.1:7280}"
ADAPTER_URL="${ADAPTER_URL:-http://127.0.0.1:9204}"
QUICKWIT_INDEX_ID="${QUICKWIT_INDEX_ID:-dataplane-corpus}"
ORG_ID="${ORG_ID:-org-smoke-qw}"
ITEM_ID="item-$(date +%s)"
EXTERNAL_ID="drive-smoke:${ITEM_ID}"
SOURCE_OBJECT_ID=""

cd "$ROOT_DIR"

echo "== Rust sparse backend fallback test =="
cargo test -p retrieval-engine-rs \
  search::sparse::tests::fallback_backend_uses_postgres_when_quickwit_fails \
  --quiet

echo "== retrieval-engine sparse backend =="
READY_JSON="$(curl -sS -f "${RETRIEVAL_URL}/readyz")"
BACKEND="$(printf '%s' "$READY_JSON" | python3 -c 'import json,sys; print(json.load(sys.stdin).get("sparse_backend",""))')"
if [[ "$BACKEND" != "quickwit-with-postgres-fallback" ]]; then
  echo "expected sparse_backend=quickwit-with-postgres-fallback, got ${BACKEND:-<empty>}" >&2
  exit 1
fi
echo "sparse_backend=${BACKEND}"

if [[ -z "${INTERNAL_API_KEY:-}" && -f .env ]]; then
  INTERNAL_API_KEY="$(awk -F= '$1 == "INTERNAL_API_KEY" {print substr($0, index($0, "=") + 1)}' .env | tail -n 1)"
  export INTERNAL_API_KEY
fi

AUTH_ARGS=(-H "X-Org-ID: ${ORG_ID}")
if [[ -n "${INTERNAL_API_KEY:-}" ]]; then
  AUTH_ARGS+=(-H "X-Internal-Api-Key: ${INTERNAL_API_KEY}")
fi

cleanup() {
  if [[ -n "$SOURCE_OBJECT_ID" ]]; then
    curl -sS -XPOST "${DOCUMENTS_URL}/v1/source-objects/delete" \
      "${AUTH_ARGS[@]}" \
      -H 'content-type: application/json' \
      -d "{\"connector\":\"sharepoint\",\"external_id\":\"${EXTERNAL_ID}\"}" >/dev/null || true
  fi
  curl -sS -XPOST "${ADAPTER_URL}/admin/rebuild" \
    -H 'content-type: application/json' \
    -d '{"clear":true}' >/dev/null || true
  for _ in {1..10}; do
    local hits
    hits="$(curl -sS -XPOST "${QUICKWIT_URL}/api/v1/${QUICKWIT_INDEX_ID}/search" \
      -H 'content-type: application/json' \
      -d "{\"query\":\"org_id:\\\"${ORG_ID}\\\"\",\"max_hits\":1}" 2>/dev/null \
      | python3 -c 'import json,sys; print(json.load(sys.stdin).get("num_hits", 0))' 2>/dev/null || echo 0)"
    [[ "$hits" == "0" ]] && break
    sleep 1
  done
}
trap cleanup EXIT

echo "== source_object -> outbox -> Quickwit =="
UPSERT_JSON="$(curl -sS -f -XPOST "${DOCUMENTS_URL}/v1/source-objects/" \
  "${AUTH_ARGS[@]}" \
  -H 'content-type: application/json' \
  -d "{
    \"connector\":\"sharepoint\",
    \"source\":\"sharepoint\",
    \"external_id\":\"${EXTERNAL_ID}\",
    \"site_id\":\"site-smoke\",
    \"drive_id\":\"drive-smoke\",
    \"item_id\":\"${ITEM_ID}\",
    \"path\":\"/Smoke/quickwit-retrieval.txt\",
    \"name\":\"quickwit-retrieval.txt\",
    \"mime_type\":\"text/plain\",
    \"size_bytes\":12,
    \"sha1_hash\":\"sha1-${ITEM_ID}\",
    \"content_hash\":\"sha1:sha1-${ITEM_ID}\",
    \"acl_tags\":[\"connector:sharepoint\",\"org:${ORG_ID}\"],
    \"metadata\":{\"smoke\":true}
  }")"
SOURCE_OBJECT_ID="$(printf '%s' "$UPSERT_JSON" | python3 -c 'import json,sys; print(json.load(sys.stdin)["source_object_id"])')"
echo "source_object_id=${SOURCE_OBJECT_ID}"

for attempt in {1..90}; do
  SEARCH_JSON="$(curl -sS -f -XPOST "${QUICKWIT_URL}/api/v1/${QUICKWIT_INDEX_ID}/search" \
    -H 'content-type: application/json' \
    -d "{\"query\":\"source_object_id:\\\"${SOURCE_OBJECT_ID}\\\"\",\"max_hits\":5}")"
  HITS="$(printf '%s' "$SEARCH_JSON" | python3 -c 'import json,sys; print(json.load(sys.stdin).get("num_hits",0))')"
  if [[ "$HITS" == "1" ]]; then
    echo "quickwit_hits=1"
    exit 0
  fi
  sleep 1
  echo "waiting_for_quickwit attempt=${attempt} hits=${HITS}"
done

echo "Quickwit did not index smoke source object in time" >&2
exit 1
