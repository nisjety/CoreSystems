#!/usr/bin/env bash
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
RUNNER="$ROOT/tests/scripts/run-isolated-real-authority-browser.sh"
OVERLAY="$ROOT/tests/e2e/isolated/real-authority-browser-compose.yml"
MODEL_FIXTURE="$ROOT/tests/e2e/isolated/model-plane-http-fixture.mjs"
SPEC="$ROOT/../Frontend Plane/velionv3/tests/e2e/real-authority-knowledge.spec.ts"
CONFIG="$ROOT/../Frontend Plane/velionv3/playwright.real-authority.config.ts"
SWITCH="$ROOT/../Frontend Plane/velionv3/apps/gateway/src/domains/orgs/switch.rs"
MIDDLEWARE="$ROOT/../Frontend Plane/velionv3/apps/gateway/src/middleware.rs"

for file in "$RUNNER" "$OVERLAY" "$MODEL_FIXTURE" "$SPEC" "$CONFIG"; do
  test -f "$file" || { echo "missing real-authority browser artifact: $file" >&2; exit 1; }
done

grep -q 'umask 077' "$RUNNER"
grep -q 'dpv2-real-authority-browser-e2e-' "$RUNNER"
grep -q '127.0.0.1::3185' "$OVERLAY"
grep -q 'ALLOW_DEV_AUTH_BYPASS: "0"' "$OVERLAY"
grep -q 'INTERNAL_API_KEY: ${REAL_AUTHORITY_INTERNAL_API_KEY:?required}' "$OVERLAY"
if grep -q 'REAL_AUTHORITY_GATEWAY_INTERNAL_KEY' "$OVERLAY" "$RUNNER"; then
  echo "browser gateway must use the Auth Core-recognized isolated internal key" >&2
  exit 1
fi
grep -q 'VITE_ALLOW_DEV_AUTH_BYPASS=false' "$RUNNER"
grep -q 'build_services_sequentially' "$RUNNER"
grep -q 'for service in "$@"' "$RUNNER"
grep -q 'REAL_AUTHORITY_FIXTURE_OUTPUT_DIR' "$RUNNER"
grep -q 'export DOCUMENTS_GDPR_NATS_PASSWORD=' "$RUNNER"
grep -q 'GDPR_DURABLE_CONSUMER_REQUIRED: "0"' "$ROOT/tests/e2e/isolated/docker-compose.yml"
grep -q 'FIXTURE_OUTPUT_FILE' "$ROOT/tests/e2e/isolated/real-authority-compose.yml"
grep -q 'REAL_AUTHORITY_AUTH_GRPC_RETRIEVAL_TOKEN=' "$RUNNER"
grep -q 'REAL_AUTHORITY_AUTH_GRPC_CREDENTIALS_FILE="$runtime_dir/auth-grpc-service-credentials.json"' "$RUNNER"
grep -q 'credentialId: "retrieval-primary"' "$RUNNER"
grep -q 'scopes: \["auth:token:validate"\]' "$RUNNER"
grep -q 'REAL_AUTHORITY_USER_GRPC_TOKEN=$(openssl rand -hex 32)' "$RUNNER"
grep -q 'REAL_AUTHORITY_USER_AUTH_INTERNAL_TOKEN=$(openssl rand -hex 32)' "$RUNNER"
grep -q 'credentialId: "auth-core-isolated"' "$RUNNER"
grep -q 'credentialId: "user-core-isolated"' "$RUNNER"
grep -q 'USER_CORE_GRPC_CLIENT_CREDENTIAL_FILE' "$ROOT/tests/e2e/isolated/real-authority-compose.yml"
grep -q 'USER_CORE_GRPC_SERVICE_CREDENTIALS_FILE' "$ROOT/tests/e2e/isolated/real-authority-compose.yml"
grep -q 'USER_AUTH_INTERNAL_CLIENT_CREDENTIAL_FILE' "$ROOT/tests/e2e/isolated/real-authority-compose.yml"
grep -q 'AUTH_INTERNAL_SERVICE_CREDENTIALS_FILE' "$ROOT/tests/e2e/isolated/real-authority-compose.yml"
grep -q 'persistentDataPlaneToken' "$RUNNER"
if grep -q 'JWT_ZDR=false' "$RUNNER"; then
  echo "browser fixtures must use the audited real Control-issued persistence token" >&2
  exit 1
fi
grep -q '"visibility":"org"' "$RUNNER"
grep -q 'allowPersistentData' "$RUNNER"
grep -q 'dataPlaneToken' "$ROOT/tests/e2e/isolated/real-authority-fixture.mjs"
grep -q 'grpc-auth-matrix.sh' "$RUNNER"
grep -q 'GRPC_VALID_BEARER="$real_bearer"' "$RUNNER"
grep -q 'real_approval_denied_bearer=$(read_authority_field 0 persistentDataPlaneToken)' "$RUNNER"
grep -q 'GRPC_APPROVAL_DENIED_BEARER="$real_approval_denied_bearer"' "$RUNNER"
grep -q '/v1/documents/' "$RUNNER"
grep -q 'down --volumes --remove-orphans' "$RUNNER"
grep -q 'remove_owned_project_images' "$RUNNER"
grep -q 'docker image ls --format' "$RUNNER"
grep -q 'remove_owned_project_images "$project"' "$RUNNER"
grep -q 'cleanup_runtime_dir' "$RUNNER"
grep -q 'test ! -e "$runtime_dir"' "$RUNNER"
grep -q 'logs --no-color --tail 80 authority-db-init' "$RUNNER"
grep -q 'rm -rf "$runtime_dir"' "$RUNNER"
grep -q 'real Auth/User session surfaces isolated Knowledge, GraphRAG, and navbar retrieval' "$SPEC"
grep -q 'cross-tenant headers and search cannot disclose the other isolated fixture' "$SPEC"
grep -q 'Search knowledge base' "$SPEC"
grep -q 'RAGGraph relationship map' "$SPEC"
grep -q 'outputDir: process.env.REAL_AUTHORITY_PLAYWRIGHT_OUTPUT_DIR' "$CONFIG"
test "$(rg -c 'invalidate_session_context_cache' "$SWITCH")" -eq 2
grep -q 'has_authorized_org_role' "$MIDDLEWARE"
if rg -q 'resolve_session_context' \
  "$ROOT/../Frontend Plane/velionv3/apps/gateway/src/domains/mcp.rs" \
  "$ROOT/../Frontend Plane/velionv3/apps/gateway/src/domains/billing.rs" \
  "$ROOT/../Frontend Plane/velionv3/apps/gateway/src/domains/agent_actions.rs"; then
  echo "tenant admin gates must use the live authorized membership" >&2
  exit 1
fi

if grep -Eq '(change-me|example-secret|hardcoded-token)' "$RUNNER" "$OVERLAY"; then
  echo "browser harness contains a forbidden default credential" >&2
  exit 1
fi

echo "PASS: real authority browser harness static contract"
