#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
SPEC="${SCRIPT_DIR}/openapi.yaml"
OUT="${SCRIPT_DIR}/sdk"

if ! command -v npx &>/dev/null; then
  echo "npx not found — install Node.js 18+ to generate SDKs" >&2
  exit 1
fi

echo "==> Generating TypeScript SDK"
npx @openapitools/openapi-generator-cli generate \
  -i "$SPEC" \
  -g typescript-fetch \
  -o "${OUT}/typescript" \
  --additional-properties=npmName=@model-plane/sdk,supportsES6=true,typescriptThreePlus=true

echo "==> Generating Python SDK"
npx @openapitools/openapi-generator-cli generate \
  -i "$SPEC" \
  -g python \
  -o "${OUT}/python" \
  --additional-properties=packageName=model_plane_sdk,projectName=model-plane-sdk

echo "==> Done. SDKs written to ${OUT}/{typescript,python}"
