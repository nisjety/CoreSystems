#!/usr/bin/env bash
set -euo pipefail

root=$(cd "$(dirname "$0")/../.." && pwd)
matrix="$root/tests/e2e/grpc-auth-matrix.sh"
overlay="$root/tests/e2e/isolated/docker-compose.yml"
runner="$root/tests/e2e/run-isolated-mvp.sh"

[ -f "$matrix" ] || { echo "FAIL: missing gRPC auth matrix" >&2; exit 1; }
bash -n "$matrix"
rg -Fq 'expected 31 gRPC methods' "$matrix"
rg -Fq 'Unauthenticated' "$matrix"
rg -Fq 'PermissionDenied' "$matrix"
rg -Fq 'x-org-id' "$matrix"
rg -Fq 'authorization: Bearer' "$matrix"
rg -Fq 'valid_same_tenant' "$matrix"
rg -Fq 'GRPC_APPROVAL_DENIED_BEARER' "$matrix"
rg -Fq 'authenticated_bearer=$GRPC_APPROVAL_DENIED_BEARER' "$matrix"
rg -Fq 'document write scope required' "$matrix"
rg -Fq 'DocumentService CreateDocument contained_write' "$matrix"
rg -Fq 'DocumentService DeleteDocument contained_write' "$matrix"
rg -Fq 'DocumentService BulkIngest contained_write' "$matrix"
rg -Fq 'FailedPrecondition' "$matrix"
rg -Fq 'is disabled; use documents-api-go' "$matrix"
rg -Fq 'principal lacks required scope' "$matrix"
rg -Fq 'WikiService ReviewProposal approve_scope' "$matrix"
rg -Fq '127.0.0.1:<port>' "$matrix"
rg -Fq 'tenant does not match authenticated principal' "$matrix"
rg -Fq 'request tenant does not match verified identity' "$matrix"
rg -Fq 'tenant mismatch' "$matrix"
rg -Fq 'request_body()' "$matrix"
rg -Fq 'authenticated NotFound' "$matrix"
rg -Fq '127.0.0.1::50052' "$overlay"
rg -Fq '127.0.0.1::50053' "$overlay"
rg -Fq '127.0.0.1::50054' "$overlay"
rg -Fq 'port retrieval-engine 50052' "$runner"
rg -Fq 'grpc-auth-matrix.sh' "$runner"
rg -Fq 'org:data:write_all' "$runner"

method_count=$(rg -c '^matrix_method ' "$matrix")
[ "$method_count" -eq 31 ] || {
  echo "FAIL: expected 31 gRPC methods, found $method_count" >&2
  exit 1
}

if rg -qi 'rebuild|cleanup|purge|reset|bulkdelete|global' "$matrix"; then
  echo "FAIL: gRPC auth matrix contains a destructive operation" >&2
  exit 1
fi

fake_bin=$(mktemp -d)
trap 'rm -rf "$fake_bin"' EXIT
cat >"$fake_bin/grpcurl" <<'EOF'
#!/usr/bin/env bash
args="$*"
if [[ "$args" != *"authorization: Bearer"* ]]; then
  printf 'Code: Unauthenticated\nMessage: invalid or missing credential\n' >&2
  exit 1
fi
if [[ "$args" == *'other-org'* ]]; then
  if [[ "$args" == *'dataplane.graph.v1.GraphService/'* ]]; then
    message='tenant mismatch'
  elif [[ "$args" == *'dataplane.wiki.v1.WikiService/'* ]]; then
    message='request tenant does not match verified identity'
  else
    message='tenant does not match authenticated principal'
  fi
  printf 'Code: PermissionDenied\nMessage: %s\n' "$message" >&2
  exit 1
fi
if [ "${GRPCURL_CONTAINMENT_PROBE:-0}" = "1" ]; then
  code='FailedPrecondition'
  case "$args" in
    *'dataplane.documents.v2.DocumentService/CreateDocument'*)
      message='gRPC DocumentService.CreateDocument is disabled; use documents-api-go POST /v1/documents instead'
      ;;
    *'dataplane.documents.v2.DocumentService/DeleteDocument'*)
      message='gRPC DocumentService.DeleteDocument is disabled; use documents-api-go DELETE /v1/documents/{id} instead'
      ;;
    *'dataplane.documents.v2.DocumentService/BulkIngest'*)
      message='gRPC DocumentService.BulkIngest is disabled; use documents-api-go POST /v1/documents/bulk instead'
      ;;
    *'dataplane.wiki.v1.WikiService/CreatePage'*|*'dataplane.wiki.v1.WikiService/UpdatePageVersion'*|*'dataplane.wiki.v1.WikiService/SubmitProposal'*)
      code='PermissionDenied'
      message='zdr=true forbids durable wiki persistence'
      ;;
    *'dataplane.wiki.v1.WikiService/ReviewProposal'*)
      code='PermissionDenied'
      message='principal lacks required scope'
      ;;
    *)
      printf '{}\n'
      exit 0
      ;;
  esac
  printf 'Code: %s\nMessage: %s\n' "$code" "$message" >&2
  exit 1
fi
printf 'Code: InvalidArgument\nMessage: synthetic invalid request\n' >&2
exit 1
EOF
chmod 700 "$fake_bin/grpcurl"

common_env=(
  "PATH=$fake_bin:$PATH"
  'GRPC_GRAPH_ADDR=127.0.0.1:50053'
  'GRPC_WIKI_ADDR=127.0.0.1:50054'
  'GRPC_VALID_BEARER=synthetic-test-token'
  'GRPC_APPROVAL_DENIED_BEARER=synthetic-approval-denied-token'
  'GRPC_VALID_ORG_ID=own-org'
  'GRPC_OTHER_ORG_ID=other-org'
)

set +e
containment_result=$(env "${common_env[@]}" \
  GRPCURL_CONTAINMENT_PROBE=1 \
  GRPC_RETRIEVAL_ADDR=127.0.0.1:50052 bash "$matrix" 2>&1)
containment_rc=$?
set -e
[ "$containment_rc" -eq 0 ] || {
  echo "FAIL: canonical legacy-write containment response failed the matrix" >&2
  printf '%s\n' "$containment_result" >&2
  exit 1
}
printf '%s' "$containment_result" | rg -Fq 'PASS: 31-method / 124-shape gRPC auth matrix' || {
  echo "FAIL: containment probe did not complete all matrix assertions" >&2
  exit 1
}

set +e
invalid_result=$(env "${common_env[@]}" \
  GRPC_RETRIEVAL_ADDR=127.0.0.1:50052 bash "$matrix" 2>&1)
invalid_rc=$?
set -e
[ "$invalid_rc" -ne 0 ] || {
  echo "FAIL: InvalidArgument false-passed the authenticated matrix" >&2
  exit 1
}
printf '%s' "$invalid_result" | rg -Fq 'expected OK or an authenticated NotFound, got InvalidArgument' || {
  echo "FAIL: matrix did not reject the unexpected authenticated outcome" >&2
  exit 1
}

set +e
remote_result=$(env "${common_env[@]}" \
  GRPC_RETRIEVAL_ADDR=grpc.example.invalid:50052 bash "$matrix" 2>&1)
remote_rc=$?
set -e
[ "$remote_rc" -ne 0 ] || {
  echo "FAIL: non-loopback gRPC target was accepted" >&2
  exit 1
}
printf '%s' "$remote_result" | rg -Fq 'must be 127.0.0.1:<port>' || {
  echo "FAIL: non-loopback target did not fail at the safety boundary" >&2
  exit 1
}

echo "PASS: gRPC auth matrix contract"
