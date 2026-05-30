#!/usr/bin/env bash
set -euo pipefail

# gRPC load test for RetrievalService.Retrieve using ghz
# Install: brew install ghz  OR  go install github.com/bojand/ghz/cmd/ghz@latest
#
# Usage:
#   ./tests/load/grpc-retrieve.sh              # 200 req, 50 concurrency
#   ./tests/load/grpc-retrieve.sh 1000 100     # 1000 req, 100 concurrency
#   GRPC_ADDR=host:port ./tests/load/grpc-retrieve.sh

TOTAL=${1:-200}
CONCURRENCY=${2:-50}
GRPC_ADDR=${GRPC_ADDR:-localhost:50052}
API_KEY=${INTERNAL_API_KEY:-}

PROTO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)/proto"

echo "═══════════════════════════════════════════════"
echo " gRPC Load Test — RetrievalService.Retrieve"
echo " Target: $GRPC_ADDR"
echo " Requests: $TOTAL  Concurrency: $CONCURRENCY"
echo "═══════════════════════════════════════════════"
echo ""

METADATA=""
if [ -n "$API_KEY" ]; then
  METADATA="-metadata '{\"x-api-key\": \"$API_KEY\"}'"
fi

ghz --insecure \
  --proto "$PROTO_ROOT/retrieval_v2.proto" \
  --import-paths "$PROTO_ROOT" \
  --call dataplane.retrieval.v2.RetrievalService/Retrieve \
  --total "$TOTAL" \
  --concurrency "$CONCURRENCY" \
  --timeout 30s \
  --data '{
    "org_id": "org-loadtest",
    "query": "What is the company vacation policy and how many days are available per year?",
    "top_k": 10,
    "filters": {}
  }' \
  ${METADATA:+$METADATA} \
  "$GRPC_ADDR"

echo ""
echo "═══════════════════════════════════════════════"
echo ""

# Also test DocumentService as baseline
echo "── DocumentService.GetIngestStatus (baseline) ──"
ghz --insecure \
  --proto "$PROTO_ROOT/documents_v2.proto" \
  --import-paths "$PROTO_ROOT" \
  --call dataplane.documents.v2.DocumentService/GetIngestStatus \
  --total "$TOTAL" \
  --concurrency "$CONCURRENCY" \
  --timeout 5s \
  --data '{"org_id": "org-loadtest"}' \
  ${METADATA:+$METADATA} \
  "$GRPC_ADDR"
