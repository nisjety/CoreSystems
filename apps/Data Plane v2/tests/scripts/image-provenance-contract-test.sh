#!/usr/bin/env bash
set -euo pipefail

root_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../../.." && pwd)"
data_dir="$root_dir/apps/Data Plane v2"
model_dir="$root_dir/apps/Model Plane"
control_dir="$root_dir/apps/Control Plane/auth-core"

grep -Fxq 'signin_output.txt' "$data_dir/.dockerignore" || {
  echo "FAIL: sign-in output artifacts must be excluded from Docker build contexts" >&2
  exit 1
}

assert_dockerfile_provenance() {
  local dockerfile=$1
  rg -q '^ARG SOURCE_REVISION=' "$dockerfile"
  rg -q '^ARG BUILD_DATE=' "$dockerfile"
  rg -q 'org\.opencontainers\.image\.revision=\$SOURCE_REVISION' "$dockerfile"
  rg -q 'org\.opencontainers\.image\.created=\$BUILD_DATE' "$dockerfile"
}

for dockerfile in \
  "$data_dir/tools/migrator/Dockerfile" \
  "$data_dir"/services/{documents-api-go,graph-index-rs,retrieval-engine-rs,data-orchestrator-go,index-engine-rs,quickwit-adapter-rs,embedding-engine-rs,wiki-store-go,data-quality-go}/Dockerfile \
  "$model_dir"/rust/services/{model-gateway,execution-core,inference-core}/Dockerfile \
  "$control_dir/Dockerfile" \
  "$root_dir/apps/Control Plane/user-core/Dockerfile"; do
  assert_dockerfile_provenance "$dockerfile"
done

for compose_file in \
  "$data_dir/docker-compose.yml" \
  "$model_dir/deploy/docker-compose.yml" \
  "$root_dir/apps/Control Plane/docker-compose.yml"; do
  rg -q 'SOURCE_REVISION: \$\{SOURCE_REVISION:\?' "$compose_file"
  rg -q 'BUILD_DATE: \$\{BUILD_DATE:\?' "$compose_file"
done

echo "PASS: image provenance contract"
