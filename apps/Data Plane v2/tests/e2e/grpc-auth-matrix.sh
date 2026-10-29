#!/usr/bin/env bash
set -euo pipefail
set +x

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
: "${GRPC_RETRIEVAL_ADDR:?required}"
: "${GRPC_GRAPH_ADDR:?required}"
: "${GRPC_WIKI_ADDR:?required}"
: "${GRPC_VALID_BEARER:?required}"
: "${GRPC_APPROVAL_DENIED_BEARER:?required}"
: "${GRPC_VALID_ORG_ID:?required}"
: "${GRPC_OTHER_ORG_ID:?required}"

require_loopback_addr() {
  local name=$1 value=$2 port
  if [[ ! "$value" =~ ^127\.0\.0\.1:([0-9]{1,5})$ ]]; then
    echo "FAIL: $name must be 127.0.0.1:<port>" >&2
    exit 2
  fi
  port=${BASH_REMATCH[1]}
  if [ "$port" -lt 1 ] || [ "$port" -gt 65535 ]; then
    echo "FAIL: $name must be 127.0.0.1:<port>" >&2
    exit 2
  fi
}

require_loopback_addr GRPC_RETRIEVAL_ADDR "$GRPC_RETRIEVAL_ADDR"
require_loopback_addr GRPC_GRAPH_ADDR "$GRPC_GRAPH_ADDR"
require_loopback_addr GRPC_WIKI_ADDR "$GRPC_WIKI_ADDR"

command -v grpcurl >/dev/null 2>&1 || { echo "missing required command: grpcurl" >&2; exit 2; }

passed=0
methods=0

grpc_call() {
  local addr=$1 proto=$2 service=$3 method=$4 body=$5
  shift 5
  local output rc code
  set +e
  output=$(grpcurl -plaintext -max-time 20 \
    -import-path "$ROOT/proto" -proto "$proto" \
    "$@" -d "$body" "$addr" "$service/$method" 2>&1)
  rc=$?
  set -e
  if [ "$rc" -eq 0 ]; then
    GRPC_CODE=OK
    GRPC_LAST_OUTPUT=$output
    return
  fi
  code=$(printf '%s\n' "$output" | sed -nE 's/^[[:space:]]*Code:[[:space:]]*//p' | head -n1)
  GRPC_CODE=${code:-Unknown}
  GRPC_LAST_OUTPUT=$output
}

expect_code() {
  local expected=$1 actual=$2 label=$3
  if [ "$actual" != "$expected" ]; then
    echo "FAIL: $label expected $expected, got $actual" >&2
    exit 1
  fi
  passed=$((passed + 1))
}

expect_authenticated_boundary() {
  local actual=$1 access=$2 label=$3 output=$4
  local expected_message='' expected_code='PermissionDenied' rpc_label operation
  case "$access" in
    read)
      case "$actual" in
        OK) ;;
        NotFound)
          local not_found_message
          not_found_message=$(printf '%s\n' "$output" | sed -nE 's/^[[:space:]]*Message:[[:space:]]*//p' | head -n1)
          case "$not_found_message" in
            "trace not found"|"document not found"|"page not found") ;;
            *)
              echo "FAIL: $label returned an unexpected NotFound reason" >&2
              exit 1
              ;;
          esac
          ;;
        *)
          echo "FAIL: $label expected OK or an authenticated NotFound, got $actual" >&2
          exit 1
          ;;
      esac
      ;;
    zdr_write)
      expected_message='zdr=true forbids durable wiki persistence'
      ;;
    write_scope)
      expected_message='document write scope required'
      ;;
    contained_write)
      expected_code='FailedPrecondition'
      rpc_label=${label%% *}
      operation=${rpc_label##*/}
      expected_message="gRPC DocumentService.${operation} is disabled; use documents-api-go"
      ;;
    approve_scope)
      expected_message='principal lacks required scope'
      ;;
    *)
      echo "FAIL: $label has unknown access classification" >&2
      exit 1
      ;;
  esac
  if [ -n "$expected_message" ]; then
    if [ "$actual" != "$expected_code" ]; then
      echo "FAIL: $label expected $expected_code, got $actual" >&2
      exit 1
    fi
    case "$access" in
      zdr_write)
        printf '%s' "$output" | rg -Fq "$expected_message" || {
        echo "FAIL: $label did not reach the signed ZDR mutation guard" >&2
        exit 1
        }
        ;;
      write_scope)
        printf '%s' "$output" | rg -Fq "$expected_message" || {
        echo "FAIL: $label did not reach the authenticated document write-scope guard" >&2
        exit 1
        }
        ;;
      approve_scope)
        printf '%s' "$output" | rg -Fq "$expected_message" || {
        echo "FAIL: $label did not reach the authenticated wiki approval-scope guard" >&2
        exit 1
        }
        ;;
      contained_write)
        printf '%s' "$output" | rg -Fq "$expected_message" || {
        echo "FAIL: $label did not reach permanent legacy-write containment" >&2
        exit 1
        }
        ;;
    esac
  fi
  passed=$((passed + 1))
}

request_body() {
  local service=$1 method=$2 org_id=$3
  case "$service/$method" in
    dataplane.retrieval.v2.RetrievalService/Retrieve|dataplane.retrieval.v2.RetrievalService/RetrieveStream)
      printf '{"org_id":"%s","query":"matrix authorization probe","top_k":1,"zdr_mode":"ephemeral"}' "$org_id"
      ;;
    dataplane.retrieval.v2.RetrievalService/GetTrace)
      printf '{"org_id":"%s","trace_id":"matrix-missing-trace"}' "$org_id"
      ;;
    dataplane.retrieval.v2.RetrievalService/GetSources)
      printf '{"org_id":"%s","document_ids":["matrix-missing-document"]}' "$org_id"
      ;;
    dataplane.retrieval.v2.RetrievalService/GetChunks|dataplane.retrieval.v2.RetrievalService/PackContext)
      printf '{"org_id":"%s","knowledge_ids":["matrix-missing-unit"]}' "$org_id"
      ;;
    dataplane.documents.v2.DocumentService/GetDocument|dataplane.documents.v2.DocumentService/DeleteDocument|dataplane.documents.v2.DocumentService/GetDocumentIndexStatus)
      printf '{"org_id":"%s","document_id":"matrix-missing-document"}' "$org_id"
      ;;
    dataplane.documents.v2.DocumentService/CreateDocument)
      printf '{"org_id":"%s","source":"matrix","type":"probe","title":"matrix-probe","content":"matrix-probe","zdr_classification":"ephemeral_only"}' "$org_id"
      ;;
    dataplane.documents.v2.DocumentService/BulkIngest)
      printf '{"org_id":"%s","documents":[{"org_id":"%s","source":"matrix","type":"probe","title":"matrix-probe","content":"matrix-probe","zdr_classification":"ephemeral_only"}]}' "$org_id" "$org_id"
      ;;
    dataplane.knowledge.v2.KnowledgeService/*)
      printf '{"org_id":"%s","document_id":"matrix-missing-document"}' "$org_id"
      ;;
    dataplane.graph.v1.GraphService/GetEntity|dataplane.graph.v1.GraphService/GetRelationships)
      printf '{"org_id":"%s","entity_id":"matrix-missing-entity"}' "$org_id"
      ;;
    dataplane.graph.v1.GraphService/ExpandGraph)
      printf '{"org_id":"%s","entity_ids":["matrix-missing-entity"],"max_hops":1,"max_entities":1}' "$org_id"
      ;;
    dataplane.wiki.v1.WikiService/GetPage|dataplane.wiki.v1.WikiService/ListPageVersions|dataplane.wiki.v1.WikiService/GetPageSources|dataplane.wiki.v1.WikiService/GetBacklinks)
      printf '{"org_id":"%s","page_id":"matrix-missing-page"}' "$org_id"
      ;;
    dataplane.wiki.v1.WikiService/GetPageByPath)
      printf '{"org_id":"%s","path":"/matrix-missing-page"}' "$org_id"
      ;;
    dataplane.wiki.v1.WikiService/CreatePage)
      printf '{"org_id":"%s","workspace_id":"matrix-workspace","title":"matrix-probe","path":"/matrix-probe","initial_content":"matrix-probe"}' "$org_id"
      ;;
    dataplane.wiki.v1.WikiService/UpdatePageVersion)
      printf '{"org_id":"%s","page_id":"matrix-missing-page","new_content":"matrix-probe","edit_reason":"authorization matrix"}' "$org_id"
      ;;
    dataplane.wiki.v1.WikiService/SubmitProposal)
      printf '{"org_id":"%s","page_id":"matrix-missing-page","proposed_content":"matrix-probe","edit_reason":"authorization matrix"}' "$org_id"
      ;;
    dataplane.wiki.v1.WikiService/ReviewProposal)
      printf '{"org_id":"%s","proposal_id":"matrix-missing-proposal","decision":"reject"}' "$org_id"
      ;;
    *)
      printf '{"org_id":"%s"}' "$org_id"
      ;;
  esac
}

expect_tenant_mismatch() {
  local actual=$1 service=$2 label=$3 output=$4 expected_message
  expect_code PermissionDenied "$actual" "$label"
  case "$service" in
    dataplane.graph.v1.GraphService)
      expected_message='tenant mismatch'
      ;;
    dataplane.wiki.v1.WikiService)
      expected_message='request tenant does not match verified identity'
      ;;
    *)
      expected_message='tenant does not match authenticated principal'
      ;;
  esac
  printf '%s' "$output" | rg -Fq "$expected_message" || {
    echo "FAIL: $label did not reach the tenant-pinning guard" >&2
    exit 1
  }
}

matrix_method() {
  local addr=$1 proto=$2 service=$3 method=$4 access=${5:-read}
  local own_body other_body code output label authenticated_bearer
  methods=$((methods + 1))
  own_body=$(request_body "$service" "$method" "$GRPC_VALID_ORG_ID")
  other_body=$(request_body "$service" "$method" "$GRPC_OTHER_ORG_ID")
  label="$service/$method"
  authenticated_bearer=$GRPC_VALID_BEARER
  if [ "$access" = "approve_scope" ]; then
    authenticated_bearer=$GRPC_APPROVAL_DENIED_BEARER
  fi

  GRPC_LAST_OUTPUT=''
  grpc_call "$addr" "$proto" "$service" "$method" "$own_body"
  code=$GRPC_CODE
  expect_code Unauthenticated "$code" "$label no_auth"

  GRPC_LAST_OUTPUT=''
  grpc_call "$addr" "$proto" "$service" "$method" "$own_body" \
    -H "x-org-id: $GRPC_VALID_ORG_ID"
  code=$GRPC_CODE
  expect_code Unauthenticated "$code" "$label forged_header"

  GRPC_LAST_OUTPUT=''
  grpc_call "$addr" "$proto" "$service" "$method" "$own_body" \
    -H "authorization: Bearer $authenticated_bearer"
  code=$GRPC_CODE
  output=$GRPC_LAST_OUTPUT
  expect_authenticated_boundary "$code" "$access" "$label valid_same_tenant" "$output"

  GRPC_LAST_OUTPUT=''
  grpc_call "$addr" "$proto" "$service" "$method" "$other_body" \
    -H "authorization: Bearer $GRPC_VALID_BEARER" \
    -H "x-org-id: $GRPC_OTHER_ORG_ID"
  code=$GRPC_CODE
  output=$GRPC_LAST_OUTPUT
  expect_tenant_mismatch "$code" "$service" "$label authenticated_cross_tenant" "$output"
}

matrix_method "$GRPC_RETRIEVAL_ADDR" retrieval_v2.proto dataplane.retrieval.v2.RetrievalService Retrieve read
matrix_method "$GRPC_RETRIEVAL_ADDR" retrieval_v2.proto dataplane.retrieval.v2.RetrievalService RetrieveStream read
matrix_method "$GRPC_RETRIEVAL_ADDR" retrieval_v2.proto dataplane.retrieval.v2.RetrievalService GetTrace read
matrix_method "$GRPC_RETRIEVAL_ADDR" retrieval_v2.proto dataplane.retrieval.v2.RetrievalService GetSources read
matrix_method "$GRPC_RETRIEVAL_ADDR" retrieval_v2.proto dataplane.retrieval.v2.RetrievalService GetChunks read
matrix_method "$GRPC_RETRIEVAL_ADDR" retrieval_v2.proto dataplane.retrieval.v2.RetrievalService PackContext read

matrix_method "$GRPC_RETRIEVAL_ADDR" documents_v2.proto dataplane.documents.v2.DocumentService GetDocument read
matrix_method "$GRPC_RETRIEVAL_ADDR" documents_v2.proto dataplane.documents.v2.DocumentService ListDocuments read
matrix_method "$GRPC_RETRIEVAL_ADDR" documents_v2.proto dataplane.documents.v2.DocumentService CreateDocument contained_write
matrix_method "$GRPC_RETRIEVAL_ADDR" documents_v2.proto dataplane.documents.v2.DocumentService DeleteDocument contained_write
matrix_method "$GRPC_RETRIEVAL_ADDR" documents_v2.proto dataplane.documents.v2.DocumentService BulkIngest contained_write
matrix_method "$GRPC_RETRIEVAL_ADDR" documents_v2.proto dataplane.documents.v2.DocumentService GetDocumentIndexStatus read
matrix_method "$GRPC_RETRIEVAL_ADDR" documents_v2.proto dataplane.documents.v2.DocumentService GetIngestStatus read

matrix_method "$GRPC_RETRIEVAL_ADDR" knowledge_v2.proto dataplane.knowledge.v2.KnowledgeService CheckPermissions read
matrix_method "$GRPC_RETRIEVAL_ADDR" knowledge_v2.proto dataplane.knowledge.v2.KnowledgeService GetKnowledgeUnits read

matrix_method "$GRPC_GRAPH_ADDR" graph/v1/graph.proto dataplane.graph.v1.GraphService GetEntity read
matrix_method "$GRPC_GRAPH_ADDR" graph/v1/graph.proto dataplane.graph.v1.GraphService ListEntitiesByType read
matrix_method "$GRPC_GRAPH_ADDR" graph/v1/graph.proto dataplane.graph.v1.GraphService GetRelationships read
matrix_method "$GRPC_GRAPH_ADDR" graph/v1/graph.proto dataplane.graph.v1.GraphService GetClaims read
matrix_method "$GRPC_GRAPH_ADDR" graph/v1/graph.proto dataplane.graph.v1.GraphService ExpandGraph read
matrix_method "$GRPC_GRAPH_ADDR" graph/v1/graph.proto dataplane.graph.v1.GraphService GetContradictions read

matrix_method "$GRPC_WIKI_ADDR" wiki/v1/wiki.proto dataplane.wiki.v1.WikiService GetPage read
matrix_method "$GRPC_WIKI_ADDR" wiki/v1/wiki.proto dataplane.wiki.v1.WikiService GetPageByPath read
matrix_method "$GRPC_WIKI_ADDR" wiki/v1/wiki.proto dataplane.wiki.v1.WikiService ListPageVersions read
matrix_method "$GRPC_WIKI_ADDR" wiki/v1/wiki.proto dataplane.wiki.v1.WikiService GetPageSources read
matrix_method "$GRPC_WIKI_ADDR" wiki/v1/wiki.proto dataplane.wiki.v1.WikiService ListMaintenanceIssues read
matrix_method "$GRPC_WIKI_ADDR" wiki/v1/wiki.proto dataplane.wiki.v1.WikiService GetBacklinks read
matrix_method "$GRPC_WIKI_ADDR" wiki/v1/wiki.proto dataplane.wiki.v1.WikiService CreatePage zdr_write
matrix_method "$GRPC_WIKI_ADDR" wiki/v1/wiki.proto dataplane.wiki.v1.WikiService UpdatePageVersion zdr_write
matrix_method "$GRPC_WIKI_ADDR" wiki/v1/wiki.proto dataplane.wiki.v1.WikiService SubmitProposal zdr_write
matrix_method "$GRPC_WIKI_ADDR" wiki/v1/wiki.proto dataplane.wiki.v1.WikiService ReviewProposal approve_scope

[ "$methods" -eq 31 ] || { echo "FAIL: expected 31 gRPC methods, ran $methods" >&2; exit 1; }
[ "$passed" -eq 124 ] || { echo "FAIL: expected 124 gRPC assertions, passed $passed" >&2; exit 1; }
echo "PASS: 31-method / 124-shape gRPC auth matrix"
