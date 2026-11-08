#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"

# Which spec to generate from. `openapi` (the default, and all a bare call has ever
# done) is the gateway's internal surface. `agent-api` is the public Agent API v1
# contract (EP-12): it gets its own folder and package names, so the two SDKs never
# share a directory or a package.
SPEC_NAME="${1:-openapi}"
case "$SPEC_NAME" in
  openapi)
    SPEC="${SCRIPT_DIR}/openapi.yaml"
    OUT="${SCRIPT_DIR}/sdk"
    TS_PACKAGE="@model-plane/sdk"
    PY_PACKAGE="model_plane_sdk"
    PY_PROJECT="model-plane-sdk"
    ;;
  agent-api)
    SPEC="${SCRIPT_DIR}/agent-api.v1.yaml"
    OUT="${SCRIPT_DIR}/sdk/agent-api"
    TS_PACKAGE="@verevon/agent-api"
    PY_PACKAGE="verevon_agent_api"
    PY_PROJECT="verevon-agent-api"
    ;;
  *)
    echo "unknown spec '${SPEC_NAME}': use 'openapi' (default) or 'agent-api'" >&2
    exit 2
    ;;
esac

if ! command -v npx &>/dev/null; then
  echo "npx not found — install Node.js 18+ to generate SDKs" >&2
  exit 1
fi

echo "==> Generating TypeScript SDK"
npx @openapitools/openapi-generator-cli generate \
  -i "$SPEC" \
  -g typescript-fetch \
  -o "${OUT}/typescript" \
  --additional-properties=npmName=${TS_PACKAGE},supportsES6=true,typescriptThreePlus=true

echo "==> Generating Python SDK"
npx @openapitools/openapi-generator-cli generate \
  -i "$SPEC" \
  -g python \
  -o "${OUT}/python" \
  --additional-properties=packageName=${PY_PACKAGE},projectName=${PY_PROJECT}

echo "==> Done. SDKs written to ${OUT}/{typescript,python}"
