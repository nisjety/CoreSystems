#!/usr/bin/env bash
set -euo pipefail

# Data Plane v2 — Docker Compose Smoke Test
# Prerequisites: docker compose up -d  (all services running)
# Usage: ./scripts/smoke-test.sh [--grpc-only] [--http-only]

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

PASS=0
FAIL=0
SKIP=0

# Use `PASS=$((PASS+1))` form — `((PASS++))` returns the *old* value of PASS,
# which is 0 on the first call; combined with `set -e`, that aborts the script.
pass() { echo -e "  ${GREEN}✓${NC} $1"; PASS=$((PASS+1)); }
fail() { echo -e "  ${RED}✗${NC} $1"; FAIL=$((FAIL+1)); }
skip() { echo -e "  ${YELLOW}⊘${NC} $1 (skipped)"; SKIP=$((SKIP+1)); }

# Configurable base URLs (default: docker-compose local ports)
HTTP_RETRIEVAL=${HTTP_RETRIEVAL:-http://localhost:8014}
HTTP_DOCUMENTS=${HTTP_DOCUMENTS:-http://localhost:8010}
HTTP_WIKI=${HTTP_WIKI:-http://localhost:8011}
HTTP_ORCHESTRATOR=${HTTP_ORCHESTRATOR:-http://localhost:8012}
HTTP_QUALITY=${HTTP_QUALITY:-http://localhost:8013}
GRPC_ADDR=${GRPC_ADDR:-localhost:50062}
API_KEY=${INTERNAL_API_KEY:-}

RUN_HTTP=true
RUN_GRPC=true
case "${1:-}" in
  --grpc-only) RUN_HTTP=false ;;
  --http-only) RUN_GRPC=false ;;
esac

echo "═══════════════════════════════════════════════"
echo " Data Plane v2 — Smoke Test"
echo "═══════════════════════════════════════════════"
echo ""

# ─── HTTP Health Checks ──────────────────────────────────────────

if $RUN_HTTP; then
  echo "─── HTTP Health Checks ───"

  for svc_url in "$HTTP_RETRIEVAL" "$HTTP_DOCUMENTS" "$HTTP_WIKI" "$HTTP_ORCHESTRATOR" "$HTTP_QUALITY"; do
    svc_name=$(echo "$svc_url" | sed 's|http://localhost:||')
    status=$(curl -s -o /dev/null -w "%{http_code}" "$svc_url/health" 2>/dev/null || echo "000")
    if [ "$status" = "200" ]; then
      pass "health :$svc_name → 200"
    else
      fail "health :$svc_name → $status"
    fi
  done

  echo ""
  echo "─── HTTP Readyz Checks ───"

  for svc_url in "$HTTP_RETRIEVAL" "$HTTP_DOCUMENTS" "$HTTP_WIKI" "$HTTP_ORCHESTRATOR" "$HTTP_QUALITY"; do
    svc_name=$(echo "$svc_url" | sed 's|http://localhost:||')
    body=$(curl -s "$svc_url/readyz" 2>/dev/null || echo '{"status":"unreachable"}')
    ready=$(echo "$body" | python3 -c "import sys,json; print(json.load(sys.stdin).get('status','?'))" 2>/dev/null || echo "?")
    if [ "$ready" = "ready" ]; then
      pass "readyz :$svc_name → ready"
    else
      fail "readyz :$svc_name → $ready"
    fi
  done

  echo ""
  echo "─── HTTP Document CRUD ───"

  AUTH_HEADER=""
  if [ -n "$API_KEY" ]; then
    AUTH_HEADER="-H x-api-key:$API_KEY"
  fi

  # Create document
  CREATE_RESP=$(curl -s -X POST "$HTTP_DOCUMENTS/v1/documents" \
    -H "Content-Type: application/json" \
    $AUTH_HEADER -H "X-Org-ID: smoke-test" \
    -d '{"org_id":"smoke-test","source":"smoke","type":"test","title":"Smoke Test Doc","content":"Hello from smoke test"}' \
    2>/dev/null || echo '{}')

  DOC_ID=$(echo "$CREATE_RESP" | python3 -c "import sys,json; print(json.load(sys.stdin).get('document_id',''))" 2>/dev/null || echo "")

  if [ -n "$DOC_ID" ] && [ "$DOC_ID" != "" ]; then
    pass "POST /v1/documents → doc_id=$DOC_ID"
  else
    fail "POST /v1/documents → no document_id returned"
  fi

  # Get document
  if [ -n "$DOC_ID" ]; then
    GET_STATUS=$(curl -s -o /dev/null -w "%{http_code}" "$HTTP_DOCUMENTS/v1/documents/$DOC_ID?org_id=smoke-test" $AUTH_HEADER -H "X-Org-ID: smoke-test" 2>/dev/null || echo "000")
    if [ "$GET_STATUS" = "200" ]; then
      pass "GET /v1/documents/$DOC_ID → 200"
    else
      fail "GET /v1/documents/$DOC_ID → $GET_STATUS"
    fi
  fi

  # Delete document (cleanup)
  if [ -n "$DOC_ID" ]; then
    DEL_STATUS=$(curl -s -o /dev/null -w "%{http_code}" -X DELETE "$HTTP_DOCUMENTS/v1/documents/$DOC_ID?org_id=smoke-test" $AUTH_HEADER -H "X-Org-ID: smoke-test" 2>/dev/null || echo "000")
    if [ "$DEL_STATUS" = "200" ]; then
      pass "DELETE /v1/documents/$DOC_ID → 200"
    else
      fail "DELETE /v1/documents/$DOC_ID → $DEL_STATUS"
    fi
  fi
fi

# ─── gRPC Checks ─────────────────────────────────────────────────

if $RUN_GRPC; then
  echo ""
  echo "─── gRPC Service Discovery ───"

  if ! command -v grpcurl &>/dev/null; then
    skip "grpcurl not installed — skipping gRPC tests"
  else
    GRPC_META=""
    if [ -n "$API_KEY" ]; then
      GRPC_META="-H x-api-key:$API_KEY"
    fi

    # gRPC server doesn't ship reflection in prod; use proto files directly.
    # If a method call succeeds, the service is registered and reachable.
    # Use a Bash array for flags so paths-with-spaces work correctly.
    PROTO_ROOT="$(cd "$(dirname "$0")/.." && pwd)/proto"
    PROTO_FLAGS=(-proto "$PROTO_ROOT/documents_v2.proto" -import-path "$PROTO_ROOT")
    GRPC_META_FLAGS=()
    if [ -n "$API_KEY" ]; then
      GRPC_META_FLAGS=(-H "x-api-key:$API_KEY")
    fi

    if grpcurl -plaintext ${GRPC_META_FLAGS[@]+"${GRPC_META_FLAGS[@]}"} "${PROTO_FLAGS[@]}" \
        -d '{"org_id":"smoke-grpc"}' \
        "$GRPC_ADDR" dataplane.documents.v2.DocumentService/GetIngestStatus 2>&1 | grep -q "isActive"; then
      pass "gRPC DocumentService reachable"
    else
      fail "gRPC DocumentService not reachable"
    fi

    echo ""
    echo "─── gRPC Document CRUD ───"

    # Create via gRPC
    GRPC_CREATE=$(grpcurl -plaintext ${GRPC_META_FLAGS[@]+"${GRPC_META_FLAGS[@]}"} "${PROTO_FLAGS[@]}" \
      -d '{"org_id":"smoke-grpc","source":"grpc-smoke","type":"test","title":"gRPC Smoke","content":"Hello gRPC"}' \
      "$GRPC_ADDR" dataplane.documents.v2.DocumentService/CreateDocument 2>&1 || echo '{}')

    GRPC_DOC_ID=$(echo "$GRPC_CREATE" | python3 -c "import sys,json; d=json.load(sys.stdin).get('document',{}); print(d.get('documentId',''))" 2>/dev/null || echo "")

    if [ -n "$GRPC_DOC_ID" ]; then
      pass "gRPC CreateDocument → doc_id=$GRPC_DOC_ID"
    else
      fail "gRPC CreateDocument → no document_id ($(echo "$GRPC_CREATE" | head -1))"
    fi

    # Get via gRPC
    if [ -n "$GRPC_DOC_ID" ]; then
      GRPC_GET=$(grpcurl -plaintext ${GRPC_META_FLAGS[@]+"${GRPC_META_FLAGS[@]}"} "${PROTO_FLAGS[@]}" \
        -d "{\"document_id\":\"$GRPC_DOC_ID\",\"org_id\":\"smoke-grpc\"}" \
        "$GRPC_ADDR" dataplane.documents.v2.DocumentService/GetDocument 2>&1 || echo "ERROR")

      if echo "$GRPC_GET" | grep -q "gRPC Smoke"; then
        pass "gRPC GetDocument → found"
      else
        fail "gRPC GetDocument failed"
      fi

      # Cleanup
      grpcurl -plaintext ${GRPC_META_FLAGS[@]+"${GRPC_META_FLAGS[@]}"} "${PROTO_FLAGS[@]}" \
        -d "{\"document_id\":\"$GRPC_DOC_ID\",\"org_id\":\"smoke-grpc\"}" \
        "$GRPC_ADDR" dataplane.documents.v2.DocumentService/DeleteDocument >/dev/null 2>&1
      pass "gRPC DeleteDocument → cleanup"
    fi

    echo ""
    echo "─── gRPC Auth ───"

    # No key should fail
    GRPC_NOAUTH=$(grpcurl -plaintext $PROTO_FLAGS \
      -d '{"org_id":"smoke-grpc"}' \
      "$GRPC_ADDR" dataplane.documents.v2.DocumentService/GetIngestStatus 2>&1 || echo "")

    if echo "$GRPC_NOAUTH" | grep -qi "unauthenticated\|api.key"; then
      pass "gRPC no-auth → rejected"
    elif [ -z "$API_KEY" ]; then
      skip "gRPC auth test (INTERNAL_API_KEY not set)"
    else
      fail "gRPC no-auth → not rejected: $GRPC_NOAUTH"
    fi
  fi
fi

# ─── Summary ──────────────────────────────────────────────────────

echo ""
echo "═══════════════════════════════════════════════"
echo -e " Results: ${GREEN}$PASS passed${NC}, ${RED}$FAIL failed${NC}, ${YELLOW}$SKIP skipped${NC}"
echo "═══════════════════════════════════════════════"

if [ "$FAIL" -gt 0 ]; then
  exit 1
fi
