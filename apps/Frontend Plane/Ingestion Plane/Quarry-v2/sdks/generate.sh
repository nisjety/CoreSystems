#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
GENERATOR_VERSION="${OPENAPI_GENERATOR_VERSION:-7.22.0}"
GENERATOR_IMAGE="openapitools/openapi-generator-cli:v${GENERATOR_VERSION}"
SPEC="/local/docs/openapi.yaml"
PYTHON_OUT="/local/sdks/python"
TYPESCRIPT_OUT="/local/sdks/typescript"

if ! command -v docker >/dev/null 2>&1 || ! docker info >/dev/null 2>&1; then
  echo "SDK generation requires a running Docker daemon (image: $GENERATOR_IMAGE)." >&2
  echo "Start Docker Desktop, or run this script on a build worker with Docker available." >&2
  exit 1
fi

generate() {
  docker run --rm --pull missing \
    --user "$(id -u):$(id -g)" \
    --volume "$REPO_ROOT:/local" \
    "$GENERATOR_IMAGE" generate "$@"
}

echo "Generating SDKs from $REPO_ROOT/docs/openapi.yaml with $GENERATOR_IMAGE"

echo "==> Python SDK"
generate \
  -i "$SPEC" \
  -g python \
  -o "$PYTHON_OUT" \
  --package-name quarry_client \
  --additional-properties=projectName=quarry-client,packageVersion=0.1.0

echo "==> TypeScript SDK"
generate \
  -i "$SPEC" \
  -g typescript-fetch \
  -o "$TYPESCRIPT_OUT" \
  --additional-properties=npmName=@quarry/client,npmVersion=0.1.0,supportsES6=true,typescriptThreePlus=true

echo "Done. SDKs written to $SCRIPT_DIR/{python,typescript}"
