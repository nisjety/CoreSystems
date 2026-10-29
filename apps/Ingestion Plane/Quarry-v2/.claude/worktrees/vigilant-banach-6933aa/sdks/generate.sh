#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
SPEC="$REPO_ROOT/docs/openapi.yaml"

echo "Generating SDKs from $SPEC"

echo "==> Python SDK"
npx --yes @openapitools/openapi-generator-cli generate \
  -i "$SPEC" \
  -g python \
  -o "$SCRIPT_DIR/python" \
  --package-name quarry_client \
  --additional-properties=projectName=quarry-client,packageVersion=0.1.0

echo "==> TypeScript SDK"
npx --yes @openapitools/openapi-generator-cli generate \
  -i "$SPEC" \
  -g typescript-fetch \
  -o "$SCRIPT_DIR/typescript" \
  --additional-properties=npmName=@quarry/client,npmVersion=0.1.0,supportsES6=true,typescriptThreePlus=true

echo "Done. SDKs written to $SCRIPT_DIR/{python,typescript}"
