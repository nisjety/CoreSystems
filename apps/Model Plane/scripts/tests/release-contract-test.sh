#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

APPLICATION_IMAGES=(
  model-gateway session-core inference-core execution-core orchestrator-core
  capability-core sandbox-manager browser-broker letta-bridge cost-core bridge-core
)
INFRASTRUCTURE_IMAGES=(
  postgres nats minio dragonfly temporal temporal-ui otel-collector
  agent-memory-redis agent-memory-server
)
ARTIFACT_IMAGES=("${APPLICATION_IMAGES[@]}" "${INFRASTRUCTURE_IMAGES[@]}")

sha256_file() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  else
    shasum -a 256 "$1" | awk '{print $1}'
  fi
}

sha256_stdin() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum | awk '{print $1}'
  else
    shasum -a 256 | awk '{print $1}'
  fi
}

write_fixture_root_manifest() {
  local artifact="$1" file relative
  {
    printf 'path\tkind\tsha256\tbytes\n'
    while IFS= read -r file; do
      relative="${file#"$artifact"/}"
      printf '%s\tfile\t%s\t%s\n' "$relative" "$(sha256_file "$file")" "$(wc -c <"$file" | tr -d '[:space:]')"
    done < <(find "$artifact" -type f ! -name artifact-manifest.tsv ! -name artifact-manifest.sig ! -name .complete | LC_ALL=C sort)
  } >"$artifact/artifact-manifest.tsv"
}

# This is deliberately a hand-built, non-secret fixture. It exercises the
# verifier and artifact-contained Compose runner without building or loading a
# Docker image. The root manifest must cover every release input, not merely
# the Docker archives.
make_artifact_fixture() {
  local artifact="$1" revision="$2" payload_id="${3:-0}"
  local service env_name archive_sha migration_service relative file
  [[ "$payload_id" =~ ^[0-9]+$ ]] || {
    echo "fixture payload ID must be numeric" >&2
    exit 1
  }

  mkdir -p "$artifact/deploy" "$artifact/scripts" \
    "$artifact/migrations/session-core" "$artifact/migrations/capability-core" \
    "$artifact/migrations/cost-core" "$artifact/migrations/letta-bridge"
  cp "$ROOT_DIR/scripts/release-artifact.sh" "$artifact/scripts/release-artifact.sh"
  cp "$ROOT_DIR/scripts/compose.sh" "$artifact/scripts/compose.sh"
  chmod +x "$artifact/scripts/release-artifact.sh" "$artifact/scripts/compose.sh"

  for file in docker-compose.yml docker-compose.production.yml docker-compose.release.yml \
    nats.conf otel-collector-config.yaml seccomp-bwrap.json; do
    printf 'fixture %s\n' "$file" >"$artifact/deploy/$file"
  done
  for migration_service in session-core capability-core cost-core letta-bridge; do
    printf 'fixture migration %s\n' "$migration_service" \
      >"$artifact/migrations/$migration_service/0001_fixture.up.sql"
  done
  printf 'key\tpartition\nAUTH_CORE_ISSUER\tpublic\nAUTH_CORE_JWKS_URL\tpublic\n' >"$artifact/config-policy.tsv"
  printf 'AZURE_OPENAI_ZDR_CONFIRMED\tpublic\nMODEL_POSTGRES_PASSWORD\tsecret\n' >>"$artifact/config-policy.tsv"
  printf 'SOURCE_REVISION\tartifact\nBUILD_DATE\tartifact\n' >>"$artifact/config-policy.tsv"
  printf 'AUTH_CORE_ISSUER=https://auth.release.example/api/convex-auth\nAUTH_CORE_JWKS_URL=https://jwks.release.example/keys/model-plane\nAZURE_OPENAI_ZDR_CONFIRMED=false\n' \
    >"$artifact/runtime-public-policy.env"
  printf 'ROLLBACK_RUNTIME_EVIDENCE_VERSION=1\nSTATUS=not-attested\n' \
    >"$artifact/rollback-runtime-evidence.env"
  printf 'path\tsha256\n' >"$artifact/compose-inputs.tsv"
  for file in docker-compose.yml docker-compose.production.yml docker-compose.release.yml \
    nats.conf otel-collector-config.yaml seccomp-bwrap.json; do
    printf 'deploy/%s\t%s\n' "$file" "$(sha256_file "$artifact/deploy/$file")" >>"$artifact/compose-inputs.tsv"
  done
  printf 'path\tsha256\n' >"$artifact/migration-manifest.tsv"
  for migration_service in session-core capability-core cost-core letta-bridge; do
    printf 'migrations/%s/0001_fixture.up.sql\t%s\n' "$migration_service" \
      "$(sha256_file "$artifact/migrations/$migration_service/0001_fixture.up.sql")" >>"$artifact/migration-manifest.tsv"
  done
  printf 'dependency\trepo_revision\tgit_tree\tenv_keyset_sha256\n' >"$artifact/cross-plane-dependencies.tsv"
  for dependency in control-plane data-plane-v2 frontend-v3 ingestion-plane application-plane; do
    printf '%s\t%s\t%s\t%s\n' "$dependency" "$revision" \
      "0123456789abcdef0123456789abcdef01234567" \
      "$(printf '%064d' 0)" >>"$artifact/cross-plane-dependencies.tsv"
  done
  printf 'COMPATIBILITY_GATES_VERSION=1\nSOURCE_REVISION=%s\nSTATUS=not-attested\n' "$revision" \
    >"$artifact/compatibility-gates.env"
  printf 'SOURCE_REVISION=%s\nBUILD_DATE=2026-07-16T00:00:00Z\n' "$revision" >"$artifact/images.lock.env"
  printf 'service\tsource_revision\timage_id\tarchive\tarchive_sha256\n' >"$artifact/manifest.tsv"
  for service in "${ARTIFACT_IMAGES[@]}"; do
    env_name="$(printf '%s_RELEASE_IMAGE' "$service" | tr '[:lower:]-' '[:upper:]_')"
    printf '%s=sha256:%064d\n' "$env_name" "$payload_id" >>"$artifact/images.lock.env"
    printf 'fixture archive payload %s for %s\n' "$payload_id" "$service" \
      >"$artifact/${service}.docker.tar"
    archive_sha="$(sha256_file "$artifact/${service}.docker.tar")"
    printf '%s\t%s\tsha256:%064d\t%s.docker.tar\t%s\n' \
      "$service" "$revision" "$payload_id" "$service" "$archive_sha" >>"$artifact/manifest.tsv"
  done
  printf 'ARTIFACT_FORMAT_VERSION=3\nSOURCE_REVISION=%s\nBUILD_DATE=2026-07-16T00:00:00Z\nSOURCE_DATE_EPOCH=0\n' "$revision" \
    >"$artifact/artifact-metadata.env"
  printf 'MODEL_PLANE_GIT_TREE=0123456789abcdef0123456789abcdef01234567\n' \
    >>"$artifact/artifact-metadata.env"
  printf 'CONFIG_POLICY_SHA256=%s\n' "$(sha256_file "$artifact/config-policy.tsv")" \
    >>"$artifact/artifact-metadata.env"
  printf 'RUNTIME_PUBLIC_POLICY_SHA256=%s\n' "$(sha256_file "$artifact/runtime-public-policy.env")" \
    >>"$artifact/artifact-metadata.env"
  printf 'COMPOSE_INPUTS_SHA256=%s\n' "$(sha256_file "$artifact/compose-inputs.tsv")" \
    >>"$artifact/artifact-metadata.env"
  printf 'MIGRATION_MANIFEST_SHA256=%s\n' "$(sha256_file "$artifact/migration-manifest.tsv")" \
    >>"$artifact/artifact-metadata.env"
  printf 'CROSS_PLANE_DEPENDENCIES_SHA256=%s\n' "$(sha256_file "$artifact/cross-plane-dependencies.tsv")" \
    >>"$artifact/artifact-metadata.env"
  printf 'COMPATIBILITY_GATES_SHA256=%s\n' "$(sha256_file "$artifact/compatibility-gates.env")" \
    >>"$artifact/artifact-metadata.env"
  write_fixture_root_manifest "$artifact"
  touch "$artifact/.complete"
}

write_runtime_evidence_fixture() {
  local candidate="$1" rollback="$2" runtime_file="$3" manifest_sha="$4"
  local secret_schema runtime_keyset secret_reference
  secret_schema="$(awk -F '\t' 'NR > 1 && $2 == "secret" { print $1 }' \
    "$rollback/config-policy.tsv" | LC_ALL=C sort | sha256_stdin)"
  runtime_keyset="$(sed -nE 's/^([A-Z][A-Z0-9_]*)=.*/\1/p' "$runtime_file" |
    LC_ALL=C sort | sha256_stdin)"
  secret_reference="$(printf 'fixture-secret-version-v1' | sha256_stdin)"
  printf 'ROLLBACK_RUNTIME_EVIDENCE_VERSION=1\nROLLBACK_ARTIFACT_MANIFEST_SHA256=%s\n' \
    "$manifest_sha" >"$candidate/rollback-runtime-evidence.env"
  printf 'SECRET_SCHEMA_SHA256=%s\nRUNTIME_KEYSET_SHA256=%s\n' \
    "$secret_schema" "$runtime_keyset" >>"$candidate/rollback-runtime-evidence.env"
  printf 'SECRET_VERSION_REFERENCE_SHA256=%s\nSTATUS=rollback-config-renderable\n' \
    "$secret_reference" >>"$candidate/rollback-runtime-evidence.env"
}

attest_and_sign_fixture() {
  local artifact="$1" revision="$2" private_key="$3" public_key="$4"
  local retention_path="${5:-zdr-provider-route-attested}"
  local rollback_manifest_sha="${6:-$(printf '%064d' 3)}" metadata_tmp
  command -v openssl >/dev/null 2>&1 || {
    echo "openssl is required for signed release artifact contract coverage" >&2
    exit 1
  }
  printf 'COMPATIBILITY_GATES_VERSION=1\nSOURCE_REVISION=%s\nSTATUS=passed\n' "$revision" \
    >"$artifact/compatibility-gates.env"
  printf 'PROTOCOL_COMPATIBILITY=passed\nMIGRATION_COMPATIBILITY=passed\nLIVE_AUTHORIZATION=passed\n' \
    >>"$artifact/compatibility-gates.env"
  printf 'APPROVAL_CONTINUATION=passed\nZDR_RETENTION_PATH=%s\n' "$retention_path" \
    >>"$artifact/compatibility-gates.env"
  printf 'ZDR_EVIDENCE_SHA256=%064d\nAUTH_IDENTITY_EVIDENCE_SHA256=%064d\n' 1 2 \
    >>"$artifact/compatibility-gates.env"
  printf 'ROLLBACK_ARTIFACT_MANIFEST_SHA256=%s\n' "$rollback_manifest_sha" \
    >>"$artifact/compatibility-gates.env"
  metadata_tmp="$TMP_DIR/artifact-metadata.updated"
  awk -F= -v replacement="$(sha256_file "$artifact/compatibility-gates.env")" \
    '$1 == "COMPATIBILITY_GATES_SHA256" { print "COMPATIBILITY_GATES_SHA256=" replacement; next } { print }' \
    "$artifact/artifact-metadata.env" >"$metadata_tmp"
  mv "$metadata_tmp" "$artifact/artifact-metadata.env"
  if [[ ! -e "$private_key" && ! -e "$public_key" ]]; then
    openssl genpkey -algorithm RSA -pkeyopt rsa_keygen_bits:2048 -out "$private_key" >/dev/null 2>&1
    openssl pkey -in "$private_key" -pubout -out "$public_key" >/dev/null 2>&1
  elif [[ ! -r "$private_key" || ! -r "$public_key" ]]; then
    echo "fixture signing key pair is incomplete" >&2
    exit 1
  fi
  cp "$public_key" "$artifact/signing-public-key.pem"
  write_fixture_root_manifest "$artifact"
  openssl dgst -sha256 -sign "$private_key" -out "$artifact/artifact-manifest.sig" \
    "$artifact/artifact-manifest.tsv" >/dev/null 2>&1
}

mkdir -p "$TMP_DIR/bin"
CAPTURE="$TMP_DIR/docker-args"
cat >"$TMP_DIR/bin/docker" <<'EOF'
#!/usr/bin/env bash
printf '%s\n' "$@" >"${DOCKER_ARG_CAPTURE:?}"
EOF
chmod +x "$TMP_DIR/bin/docker"

DOCKER_ARG_CAPTURE="$CAPTURE" \
PATH="$TMP_DIR/bin:$PATH" \
MODEL_PLANE_DEV=0 \
"$ROOT_DIR/scripts/compose.sh" ps

grep -Fx -- "$ROOT_DIR/deploy/.env" "$CAPTURE" >/dev/null
if grep -Fx -- "$ROOT_DIR/.env" "$CAPTURE" >/dev/null; then
  echo "compose.sh used the non-canonical root .env" >&2
  exit 1
fi

# A mutable workspace lock must never become a production release path. The
# only production entry point is the signed artifact-contained runner below.
if DOCKER_ARG_CAPTURE="$TMP_DIR/workspace-production-args" \
  PATH="$TMP_DIR/bin:$PATH" \
  MODEL_PLANE_PRODUCTION=1 MODEL_PLANE_DEV=0 \
  "$ROOT_DIR/scripts/compose.sh" config >/dev/null 2>&1; then
  echo "workspace Compose accepted a direct production invocation" >&2
  exit 1
fi

# Development/integration deployments publish the three protocol-critical
# gRPC listeners on loopback so compatibility probes can exercise the exact
# contracts that existing clients use. The production overlay must continue to
# remove all host mappings; east-west callers use the private Docker networks.
for mapping in \
  '127.0.0.1:9090:9090' \
  '127.0.0.1:9092:9092' \
  '127.0.0.1:9093:9093'; do
  grep -F -- "$mapping" "$ROOT_DIR/deploy/docker-compose.yml" >/dev/null
done
for service in model-gateway inference-core execution-core; do
  awk -v service="$service" '
    $0 ~ "^  " service ":$" { in_service = 1; next }
    in_service && $0 ~ "^  [[:alnum:]_-]+:$" { exit }
    in_service && index($0, "ports: !reset []") { found = 1 }
    END { exit(found ? 0 : 1) }
  ' "$ROOT_DIR/deploy/docker-compose.production.yml"
done

services=("${APPLICATION_IMAGES[@]}")

for service in "${services[@]}"; do
  env_name="$(printf '%s_RELEASE_IMAGE' "$service" | tr '[:lower:]-' '[:upper:]_')"
  grep -F "image: \${${env_name}:?" "$ROOT_DIR/deploy/docker-compose.release.yml" >/dev/null
done

reset_count="$(grep -Fc 'build: !reset null' "$ROOT_DIR/deploy/docker-compose.release.yml")"
test "$reset_count" -eq "${#services[@]}"

for image in "${INFRASTRUCTURE_IMAGES[@]}"; do
  env_name="$(printf '%s_RELEASE_IMAGE' "$image" | tr '[:lower:]-' '[:upper:]_')"
  grep -F "image: \${${env_name}:?" "$ROOT_DIR/deploy/docker-compose.release.yml" >/dev/null
done
grep -F 'image: ${POSTGRES_RELEASE_IMAGE:?' "$ROOT_DIR/deploy/docker-compose.release.yml" | \
  test "$(wc -l | tr -d '[:space:]')" -eq 2
pull_never_count="$(grep -Ec '^[[:space:]]+pull_policy: never$' "$ROOT_DIR/deploy/docker-compose.release.yml")"
test "$pull_never_count" -eq 21

revision="0123456789abcdef0123456789abcdef01234567"
dry_run="$TMP_DIR/dry-run"
SOURCE_REVISION="$revision" \
BUILD_DATE="2026-07-16T00:00:00Z" \
"$ROOT_DIR/scripts/release-artifact.sh" dry-run "$TMP_DIR/artifact" >"$dry_run"

build_count="$(grep -c '^docker build ' "$dry_run")"
pull_count="$(grep -c '^docker image pull ' "$dry_run")"
save_count="$(grep -c '^docker image save ' "$dry_run")"
test "$build_count" -eq "${#services[@]}"
test "$pull_count" -eq "${#INFRASTRUCTURE_IMAGES[@]}"
test "$save_count" -eq "${#ARTIFACT_IMAGES[@]}"
! grep -F "${SECRET_SENTINEL:-never-present}" "$dry_run" >/dev/null

if SOURCE_REVISION=dirty "$ROOT_DIR/scripts/release-artifact.sh" dry-run "$TMP_DIR/bad" >/dev/null 2>&1; then
  echo "release workflow accepted a non-immutable source revision" >&2
  exit 1
fi

# A release-mode build must reject missing evidence before it can ask Docker to
# build, pull, or inspect an image. The current dirty worktree is intentional:
# the required gate error must win over any later source-state failure.
preflight_output="$TMP_DIR/release-preflight-output"
if DOCKER_ARG_CAPTURE="$TMP_DIR/unexpected-docker-args" \
  PATH="$TMP_DIR/bin:$PATH" \
  MODEL_PLANE_RELEASE_MODE=1 \
  "$ROOT_DIR/scripts/release-artifact.sh" build "$TMP_DIR/release-preflight" >"$preflight_output" 2>&1; then
  echo "release build unexpectedly passed without evidence gates" >&2
  exit 1
fi
grep -F 'MODEL_PLANE_COMPATIBILITY_GATES_FILE' "$preflight_output" >/dev/null
if [[ -e "$TMP_DIR/unexpected-docker-args" ]]; then
  echo "release preflight invoked Docker before evidence validation" >&2
  exit 1
fi

lock="$TMP_DIR/images.lock.env"
printf 'SOURCE_REVISION=%s\nBUILD_DATE=2026-07-16T00:00:00Z\n' "$revision" >"$lock"
for service in "${ARTIFACT_IMAGES[@]}"; do
  env_name="$(printf '%s_RELEASE_IMAGE' "$service" | tr '[:lower:]-' '[:upper:]_')"
  printf '%s=sha256:%064d\n' "$env_name" 0 >>"$lock"
done
"$ROOT_DIR/scripts/release-artifact.sh" validate-lock "$lock"
printf 'UNSUPPORTED_SECRET=%s\n' "${SECRET_SENTINEL:-contract-secret}" >>"$lock"
if "$ROOT_DIR/scripts/release-artifact.sh" validate-lock "$lock" >/dev/null 2>&1; then
  echo "release lock accepted an unsupported environment override" >&2
  exit 1
fi

# Red phase: a valid artifact must carry a complete root manifest, runtime
# policy, migration snapshot, cross-plane revision record, and embedded
# Compose inputs. Mutating a non-image release input must make verification
# fail. The legacy verifier only checked image archives, so it incorrectly
# accepted this tamper.
artifact="$TMP_DIR/self-contained-artifact"
make_artifact_fixture "$artifact" "$revision"
"$ROOT_DIR/scripts/release-artifact.sh" verify "$artifact"

# A verified artifact must be independently restorable without registry access.
# The fake Docker client accepts only archive loads and image-ID inspection;
# any pull/build attempt would fail the contract.
restore_bin="$TMP_DIR/restore-bin"
restore_capture="$TMP_DIR/restore-archives"
mkdir -p "$restore_bin"
cat >"$restore_bin/docker" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
if [[ "${1:-}" == "image" && "${2:-}" == "load" && "${3:-}" == "--input" ]]; then
  basename "$4" >>"${RESTORE_ARCHIVE_CAPTURE:?}"
  exit 0
fi
if [[ "${1:-}" == "image" && "${2:-}" == "inspect" ]]; then
  printf '%s\n' "${!#}"
  exit 0
fi
echo "unexpected offline restore Docker operation: $*" >&2
exit 1
EOF
chmod +x "$restore_bin/docker"
RESTORE_ARCHIVE_CAPTURE="$restore_capture" PATH="$restore_bin:$PATH" \
  "$ROOT_DIR/scripts/release-artifact.sh" restore "$artifact" >/dev/null
test "$(wc -l <"$restore_capture" | tr -d '[:space:]')" -eq "${#ARTIFACT_IMAGES[@]}"
for image in "${ARTIFACT_IMAGES[@]}"; do
  grep -Fx "${image}.docker.tar" "$restore_capture" >/dev/null
done

printf 'tampered release policy\n' >>"$artifact/deploy/docker-compose.production.yml"
if "$ROOT_DIR/scripts/release-artifact.sh" verify "$artifact" >/dev/null 2>&1; then
  echo "release verifier accepted a mutated artifact-contained compose policy" >&2
  exit 1
fi

# An unsigned/unattested artifact can be used only for local verification. A
# release-mode verification must fail closed before any Docker operation.
make_artifact_fixture "$artifact" "$revision"
if MODEL_PLANE_RELEASE_MODE=1 "$ROOT_DIR/scripts/release-artifact.sh" verify "$artifact" >/dev/null 2>&1; then
  echo "release verifier accepted an unsigned or unattested artifact" >&2
  exit 1
fi

# A syntactically signed gate is still rejected when it tries to hand-wave the
# all-interactive-ZDR/no-verified-provider contradiction. Only an independently
# attested provider route or an authoritative non-ZDR policy path is accepted.
invalid_zdr_artifact="$TMP_DIR/invalid-zdr-artifact"
invalid_zdr_private_key="$TMP_DIR/invalid-zdr-private.pem"
invalid_zdr_public_key="$TMP_DIR/invalid-zdr-public.pem"
make_artifact_fixture "$invalid_zdr_artifact" "$revision"
attest_and_sign_fixture "$invalid_zdr_artifact" "$revision" \
  "$invalid_zdr_private_key" "$invalid_zdr_public_key" "unverified-provider"
if MODEL_PLANE_RELEASE_MODE=1 \
  MODEL_PLANE_ARTIFACT_VERIFY_KEY="$invalid_zdr_public_key" \
  "$ROOT_DIR/scripts/release-artifact.sh" verify "$invalid_zdr_artifact" >/dev/null 2>&1; then
  echo "release verifier accepted an unverified ZDR retention path" >&2
  exit 1
fi

# Runtime configuration is external by design so secrets never enter the
# artifact. It is nevertheless constrained to the artifact's public keyset.
runtime_env="$TMP_DIR/runtime.env"
printf 'MODEL_POSTGRES_PASSWORD=not-a-real-secret\n' >"$runtime_env"
chmod 600 "$runtime_env"
"$ROOT_DIR/scripts/release-artifact.sh" validate-runtime-config "$runtime_env" \
  "$artifact/config-policy.tsv" "$artifact/runtime-public-policy.env" \
  "$artifact/compatibility-gates.env"
cp "$runtime_env" "$TMP_DIR/runtime-wrong-public.env"
printf 'AUTH_CORE_ISSUER=https://unsigned-attacker.example/api/convex-auth\n' \
  >>"$TMP_DIR/runtime-wrong-public.env"
if "$ROOT_DIR/scripts/release-artifact.sh" validate-runtime-config \
  "$TMP_DIR/runtime-wrong-public.env" "$artifact/config-policy.tsv" \
  "$artifact/runtime-public-policy.env" "$artifact/compatibility-gates.env" >/dev/null 2>&1; then
  echo "release runtime policy accepted an unsigned Auth issuer override" >&2
  exit 1
fi
sed 's/AZURE_OPENAI_ZDR_CONFIRMED=false/AZURE_OPENAI_ZDR_CONFIRMED=true/' \
  "$artifact/runtime-public-policy.env" >"$TMP_DIR/runtime-unattested-zdr-policy.env"
if "$ROOT_DIR/scripts/release-artifact.sh" validate-runtime-config \
  "$runtime_env" "$artifact/config-policy.tsv" \
  "$TMP_DIR/runtime-unattested-zdr-policy.env" "$artifact/compatibility-gates.env" >/dev/null 2>&1; then
  echo "release runtime policy accepted ZDR confirmation without provider attestation" >&2
  exit 1
fi
printf 'UNAPPROVED_OVERRIDE=value\n' >>"$runtime_env"
if "$ROOT_DIR/scripts/release-artifact.sh" validate-runtime-config "$runtime_env" \
  "$artifact/config-policy.tsv" "$artifact/runtime-public-policy.env" \
  "$artifact/compatibility-gates.env" >/dev/null 2>&1; then
  echo "release runtime policy accepted an unreviewed configuration key" >&2
  exit 1
fi

# Red phase: the compatibility gate must bind to an externally supplied,
# independently signed rollback artifact. A digest-shaped value alone cannot
# prove that a deployable rollback archive exists. These fixtures intentionally
# exercise the gate without building or loading Docker images.
rollback_artifact="$TMP_DIR/rollback-artifact"
rollback_private_key="$TMP_DIR/rollback-private.pem"
rollback_public_key="$TMP_DIR/rollback-public.pem"
rollback_revision="89abcdef0123456789abcdef0123456789abcdef"
mismatched_rollback_revision="fedcba9876543210fedcba9876543210fedcba98"
make_artifact_fixture "$rollback_artifact" "$rollback_revision" 1
# Simulate an accepted v3 predecessor built before the fixed renderability
# command existed. The current candidate adapter must not execute or recurse
# into this predecessor's command dispatcher.
sed 's/validate-deployability/legacy-deployability-unsupported/g' \
  "$rollback_artifact/scripts/release-artifact.sh" >"$TMP_DIR/legacy-release-artifact.sh"
mv "$TMP_DIR/legacy-release-artifact.sh" "$rollback_artifact/scripts/release-artifact.sh"
chmod +x "$rollback_artifact/scripts/release-artifact.sh"
attest_and_sign_fixture "$rollback_artifact" "$rollback_revision" \
  "$rollback_private_key" "$rollback_public_key" \
  "authoritative-non-zdr-policy-attested"
rollback_manifest_sha="$(sha256_file "$rollback_artifact/artifact-manifest.tsv")"
rollback_runtime_env="$TMP_DIR/rollback-runtime.env"
printf 'MODEL_POSTGRES_PASSWORD=not-a-real-rollback-secret\n' >"$rollback_runtime_env"
chmod 600 "$rollback_runtime_env"

mismatched_rollback_artifact="$TMP_DIR/mismatched-rollback-artifact"
mismatched_rollback_public_key="$invalid_zdr_public_key"
make_artifact_fixture "$mismatched_rollback_artifact" "$mismatched_rollback_revision" 2
attest_and_sign_fixture "$mismatched_rollback_artifact" "$mismatched_rollback_revision" \
  "$invalid_zdr_private_key" "$mismatched_rollback_public_key" \
  "authoritative-non-zdr-policy-attested"
test "$(sha256_file "$mismatched_rollback_artifact/artifact-manifest.tsv")" != "$rollback_manifest_sha"

# Artifact-contained Compose must not fall back to the mutable workspace. A
# fake Docker client lets this assert the selected files without deployment.
release_private_key="$invalid_zdr_private_key"
release_public_key="$invalid_zdr_public_key"
write_runtime_evidence_fixture "$artifact" "$rollback_artifact" \
  "$rollback_runtime_env" "$rollback_manifest_sha"
attest_and_sign_fixture "$artifact" "$revision" "$release_private_key" "$release_public_key" \
  "authoritative-non-zdr-policy-attested" "$rollback_manifest_sha"
rollback_runtime_evidence="$TMP_DIR/rollback-runtime-evidence.env"
cp "$artifact/rollback-runtime-evidence.env" "$rollback_runtime_evidence"
export MODEL_PLANE_ROLLBACK_RUNTIME_ENV_FILE="$rollback_runtime_env"
export MODEL_PLANE_ROLLBACK_RUNTIME_EVIDENCE_FILE="$rollback_runtime_evidence"

# A rollback is not a rollback when it was produced from the candidate's exact
# source revision. A separately signed root and a different manifest digest do
# not make the same revision a safe predecessor.
same_revision_rollback_artifact="$TMP_DIR/same-revision-rollback-artifact"
same_revision_candidate="$TMP_DIR/same-revision-candidate"
make_artifact_fixture "$same_revision_rollback_artifact" "$revision" 1
attest_and_sign_fixture "$same_revision_rollback_artifact" "$revision" \
  "$rollback_private_key" "$rollback_public_key" \
  "authoritative-non-zdr-policy-attested"
same_revision_manifest_sha="$(sha256_file "$same_revision_rollback_artifact/artifact-manifest.tsv")"
make_artifact_fixture "$same_revision_candidate" "$revision" 0
attest_and_sign_fixture "$same_revision_candidate" "$revision" \
  "$release_private_key" "$release_public_key" \
  "authoritative-non-zdr-policy-attested" "$same_revision_manifest_sha"
same_revision_rollback_output="$TMP_DIR/same-revision-rollback-output"
if MODEL_PLANE_RELEASE_MODE=1 \
  MODEL_PLANE_ARTIFACT_VERIFY_KEY="$release_public_key" \
  MODEL_PLANE_ROLLBACK_ARTIFACT_VERIFY_KEY="$rollback_public_key" \
  MODEL_PLANE_ROLLBACK_ARTIFACT_DIR="$same_revision_rollback_artifact" \
  "$ROOT_DIR/scripts/release-artifact.sh" verify "$same_revision_candidate" \
  >"$same_revision_rollback_output" 2>&1; then
  echo "release verifier accepted candidate and rollback from the same source revision" >&2
  exit 1
fi
grep -F 'rollback artifact source revision must differ from the candidate source revision' \
  "$same_revision_rollback_output" >/dev/null

# A distinct commit alone is not a usable rollback payload. At least one
# content-addressed image in the normalized lock must differ from the candidate.
same_image_rollback_artifact="$TMP_DIR/same-image-rollback-artifact"
same_image_candidate="$TMP_DIR/same-image-candidate"
make_artifact_fixture "$same_image_rollback_artifact" "$rollback_revision" 0
attest_and_sign_fixture "$same_image_rollback_artifact" "$rollback_revision" \
  "$rollback_private_key" "$rollback_public_key" \
  "authoritative-non-zdr-policy-attested"
same_image_manifest_sha="$(sha256_file "$same_image_rollback_artifact/artifact-manifest.tsv")"
make_artifact_fixture "$same_image_candidate" "$revision" 0
attest_and_sign_fixture "$same_image_candidate" "$revision" \
  "$release_private_key" "$release_public_key" \
  "authoritative-non-zdr-policy-attested" "$same_image_manifest_sha"
same_image_rollback_output="$TMP_DIR/same-image-rollback-output"
if MODEL_PLANE_RELEASE_MODE=1 \
  MODEL_PLANE_ARTIFACT_VERIFY_KEY="$release_public_key" \
  MODEL_PLANE_ROLLBACK_ARTIFACT_VERIFY_KEY="$rollback_public_key" \
  MODEL_PLANE_ROLLBACK_ARTIFACT_DIR="$same_image_rollback_artifact" \
  "$ROOT_DIR/scripts/release-artifact.sh" verify "$same_image_candidate" \
  >"$same_image_rollback_output" 2>&1; then
  echo "release verifier accepted candidate and rollback with the same image-lock payload" >&2
  exit 1
fi
grep -F 'rollback artifact image-lock payload must differ from the candidate image-lock payload' \
  "$same_image_rollback_output" >/dev/null

# The rollback's signed public policy is not a substitute for its external
# secret runtime. Binding must prove the exact rollback config can be rendered.
missing_rollback_runtime_output="$TMP_DIR/missing-rollback-runtime-output"
if env -u MODEL_PLANE_ROLLBACK_RUNTIME_ENV_FILE \
  MODEL_PLANE_RELEASE_MODE=1 \
  MODEL_PLANE_ARTIFACT_VERIFY_KEY="$release_public_key" \
  MODEL_PLANE_ROLLBACK_ARTIFACT_VERIFY_KEY="$rollback_public_key" \
  MODEL_PLANE_ROLLBACK_ARTIFACT_DIR="$rollback_artifact" \
  "$ROOT_DIR/scripts/release-artifact.sh" verify "$artifact" \
  >"$missing_rollback_runtime_output" 2>&1; then
  echo "release verifier accepted a rollback without an external runtime file" >&2
  exit 1
fi
grep -F 'MODEL_PLANE_ROLLBACK_RUNTIME_ENV_FILE' "$missing_rollback_runtime_output" >/dev/null

# The candidate-signed rollback evidence binds the supplied runtime keyset and
# an opaque secret-manager version reference without copying secret values.
wrong_runtime_evidence_candidate="$TMP_DIR/wrong-runtime-evidence-candidate"
make_artifact_fixture "$wrong_runtime_evidence_candidate" "$revision" 0
write_runtime_evidence_fixture "$wrong_runtime_evidence_candidate" "$rollback_artifact" \
  "$rollback_runtime_env" "$rollback_manifest_sha"
sed 's/^RUNTIME_KEYSET_SHA256=.*/RUNTIME_KEYSET_SHA256=0000000000000000000000000000000000000000000000000000000000000000/' \
  "$wrong_runtime_evidence_candidate/rollback-runtime-evidence.env" \
  >"$TMP_DIR/wrong-runtime-evidence.env"
mv "$TMP_DIR/wrong-runtime-evidence.env" \
  "$wrong_runtime_evidence_candidate/rollback-runtime-evidence.env"
attest_and_sign_fixture "$wrong_runtime_evidence_candidate" "$revision" \
  "$release_private_key" "$release_public_key" \
  "authoritative-non-zdr-policy-attested" "$rollback_manifest_sha"
wrong_runtime_evidence_output="$TMP_DIR/wrong-runtime-evidence-output"
if MODEL_PLANE_RELEASE_MODE=1 \
  MODEL_PLANE_ARTIFACT_VERIFY_KEY="$release_public_key" \
  MODEL_PLANE_ROLLBACK_ARTIFACT_VERIFY_KEY="$rollback_public_key" \
  MODEL_PLANE_ROLLBACK_ARTIFACT_DIR="$rollback_artifact" \
  "$ROOT_DIR/scripts/release-artifact.sh" verify "$wrong_runtime_evidence_candidate" \
  >"$wrong_runtime_evidence_output" 2>&1; then
  echo "release verifier accepted rollback evidence for a different runtime keyset" >&2
  exit 1
fi
grep -F 'rollback runtime evidence keyset does not match' \
  "$wrong_runtime_evidence_output" >/dev/null

# Release verification must fail before any deployment when no external
# rollback locator is supplied, even though the candidate root itself is
# correctly signed and the gate has a well-formed digest.
missing_rollback_output="$TMP_DIR/missing-rollback-output"
if MODEL_PLANE_RELEASE_MODE=1 \
  MODEL_PLANE_ARTIFACT_VERIFY_KEY="$release_public_key" \
  MODEL_PLANE_ROLLBACK_ARTIFACT_VERIFY_KEY="$rollback_public_key" \
  "$ROOT_DIR/scripts/release-artifact.sh" verify "$artifact" >"$missing_rollback_output" 2>&1; then
  echo "release verifier accepted a candidate without a rollback artifact locator" >&2
  exit 1
fi
grep -F 'MODEL_PLANE_ROLLBACK_ARTIFACT_DIR' "$missing_rollback_output" >/dev/null

# A nonexistent external locator must fail at the same pre-Docker boundary.
nonexistent_rollback_output="$TMP_DIR/nonexistent-rollback-output"
if MODEL_PLANE_RELEASE_MODE=1 \
  MODEL_PLANE_ARTIFACT_VERIFY_KEY="$release_public_key" \
  MODEL_PLANE_ROLLBACK_ARTIFACT_VERIFY_KEY="$rollback_public_key" \
  MODEL_PLANE_ROLLBACK_ARTIFACT_DIR="$TMP_DIR/does-not-exist" \
  "$ROOT_DIR/scripts/release-artifact.sh" verify "$artifact" >"$nonexistent_rollback_output" 2>&1; then
  echo "release verifier accepted a nonexistent rollback artifact" >&2
  exit 1
fi
grep -F 'rollback artifact directory does not exist' "$nonexistent_rollback_output" >/dev/null

# The rollback root manifest covers the archive contents. Mutating a copied
# rollback payload without re-signing it must invalidate the candidate gate.
tampered_rollback_artifact="$TMP_DIR/tampered-rollback-artifact"
cp -R "$rollback_artifact" "$tampered_rollback_artifact"
chmod -R u+w "$tampered_rollback_artifact"
printf 'tampered rollback payload\n' >>"$tampered_rollback_artifact/deploy/docker-compose.production.yml"
tampered_rollback_output="$TMP_DIR/tampered-rollback-output"
if MODEL_PLANE_RELEASE_MODE=1 \
  MODEL_PLANE_ARTIFACT_VERIFY_KEY="$release_public_key" \
  MODEL_PLANE_ROLLBACK_ARTIFACT_VERIFY_KEY="$rollback_public_key" \
  MODEL_PLANE_ROLLBACK_ARTIFACT_DIR="$tampered_rollback_artifact" \
  "$ROOT_DIR/scripts/release-artifact.sh" verify "$artifact" >"$tampered_rollback_output" 2>&1; then
  echo "release verifier accepted a tampered rollback artifact" >&2
  exit 1
fi
grep -F 'artifact file size mismatch: deploy/docker-compose.production.yml' "$tampered_rollback_output" >/dev/null

# File content signatures do not cover Unix execute bits. A rollback whose
# signed runners cannot actually execute must fail deployability validation.
nonexecutable_rollback_artifact="$TMP_DIR/nonexecutable-rollback-artifact"
cp -R "$rollback_artifact" "$nonexecutable_rollback_artifact"
chmod u-x "$nonexecutable_rollback_artifact/scripts/compose.sh"
nonexecutable_rollback_output="$TMP_DIR/nonexecutable-rollback-output"
if MODEL_PLANE_RELEASE_MODE=1 \
  MODEL_PLANE_ARTIFACT_VERIFY_KEY="$release_public_key" \
  MODEL_PLANE_ROLLBACK_ARTIFACT_VERIFY_KEY="$rollback_public_key" \
  MODEL_PLANE_ROLLBACK_ARTIFACT_DIR="$nonexecutable_rollback_artifact" \
  "$ROOT_DIR/scripts/release-artifact.sh" verify "$artifact" \
  >"$nonexecutable_rollback_output" 2>&1; then
  echo "release verifier accepted a rollback with a nonexecutable deployment runner" >&2
  exit 1
fi
grep -F 'deployability validation requires executable artifact-contained runners' \
  "$nonexecutable_rollback_output" >/dev/null

# A local writer racing the render operation must not make verification and
# use observe different rollback inputs. Compose reads private snapshots, and
# the final stored-artifact pass detects the concurrent mutation.
racing_rollback_artifact="$TMP_DIR/racing-rollback-artifact"
cp -R "$rollback_artifact" "$racing_rollback_artifact"
chmod -R u+w "$racing_rollback_artifact"
racing_docker_bin="$TMP_DIR/racing-docker-bin"
mkdir -p "$racing_docker_bin"
printf '#!/usr/bin/env bash\nprintf "raced\\n" >> %q\nexit 0\n' \
  "$racing_rollback_artifact/deploy/docker-compose.production.yml" \
  >"$racing_docker_bin/docker"
chmod +x "$racing_docker_bin/docker"
racing_rollback_output="$TMP_DIR/racing-rollback-output"
if PATH="$racing_docker_bin:$PATH" \
  MODEL_PLANE_RELEASE_MODE=1 \
  MODEL_PLANE_ARTIFACT_VERIFY_KEY="$release_public_key" \
  MODEL_PLANE_ROLLBACK_ARTIFACT_VERIFY_KEY="$rollback_public_key" \
  MODEL_PLANE_ROLLBACK_ARTIFACT_DIR="$racing_rollback_artifact" \
  "$ROOT_DIR/scripts/release-artifact.sh" verify "$artifact" \
  >"$racing_rollback_output" 2>&1; then
  echo "release verifier accepted a rollback mutated during renderability validation" >&2
  exit 1
fi
grep -E 'artifact file size mismatch|rollback artifact changed during deployability validation' \
  "$racing_rollback_output" >/dev/null

# The candidate's signed gate, rollback evidence, and image lock must be read
# from one trusted private view. Previously the verifier fully checked the
# candidate, then reopened its mutable gate before creating that view. A local
# writer could temporarily replace only the gate after signature verification,
# bind it to another trusted rollback that matched the signed runtime evidence,
# and restore the signed gate before the final verification pass.
gate_race_rollback_artifact="$TMP_DIR/gate-race-rollback-artifact"
gate_race_rollback_revision="76543210fedcba9876543210fedcba9876543210"
make_artifact_fixture "$gate_race_rollback_artifact" "$gate_race_rollback_revision" 2
attest_and_sign_fixture "$gate_race_rollback_artifact" "$gate_race_rollback_revision" \
  "$rollback_private_key" "$rollback_public_key" \
  "authoritative-non-zdr-policy-attested"
gate_race_rollback_manifest_sha="$(sha256_file "$gate_race_rollback_artifact/artifact-manifest.tsv")"

gate_race_candidate="$TMP_DIR/gate-race-candidate"
make_artifact_fixture "$gate_race_candidate" "$revision" 0
write_runtime_evidence_fixture "$gate_race_candidate" "$gate_race_rollback_artifact" \
  "$rollback_runtime_env" "$gate_race_rollback_manifest_sha"
# Keep the signed gate bound to the original rollback while deliberately
# signing runtime evidence for the alternate rollback. Without the race this
# inconsistent candidate must fail closed.
attest_and_sign_fixture "$gate_race_candidate" "$revision" \
  "$release_private_key" "$release_public_key" \
  "authoritative-non-zdr-policy-attested" "$rollback_manifest_sha"
gate_race_signed_gate="$TMP_DIR/gate-race-signed-gate.env"
gate_race_attack_gate="$TMP_DIR/gate-race-attack-gate.env"
cp "$gate_race_candidate/compatibility-gates.env" "$gate_race_signed_gate"
sed "s/^ROLLBACK_ARTIFACT_MANIFEST_SHA256=.*/ROLLBACK_ARTIFACT_MANIFEST_SHA256=$gate_race_rollback_manifest_sha/" \
  "$gate_race_signed_gate" >"$gate_race_attack_gate"

gate_race_bin="$TMP_DIR/gate-race-bin"
gate_race_state="$TMP_DIR/gate-race-swapped"
gate_race_restored="$TMP_DIR/gate-race-restored"
gate_race_real_openssl="$(command -v openssl)"
gate_race_real_sed="$(command -v sed)"
mkdir -p "$gate_race_bin"
printf '%s\n' \
  '#!/usr/bin/env bash' \
  'set -u' \
  'status=0' \
  '"$GATE_RACE_REAL_OPENSSL" "$@" || status=$?' \
  'matched=0' \
  'for argument in "$@"; do' \
  '  [[ "$argument" == "$GATE_RACE_CANDIDATE/artifact-manifest.sig" ]] && matched=1' \
  'done' \
  'if [[ "$status" == 0 && "$matched" == 1 && ! -e "$GATE_RACE_STATE" ]]; then' \
  '  cp "$GATE_RACE_ATTACK_GATE" "$GATE_RACE_CANDIDATE/compatibility-gates.env"' \
  '  printf "0\n" >"$GATE_RACE_STATE"' \
  'fi' \
  'exit "$status"' >"$gate_race_bin/openssl"
printf '%s\n' \
  '#!/usr/bin/env bash' \
  'set -u' \
  'status=0' \
  '"$GATE_RACE_REAL_SED" "$@" || status=$?' \
  'matched=0' \
  'for argument in "$@"; do' \
  '  [[ "$argument" == "s/^ROLLBACK_ARTIFACT_MANIFEST_SHA256=//p" ]] && matched=1' \
  'done' \
  'last="${!#:-}"' \
  'if [[ "$status" == 0 && "$matched" == 1 && "$last" == "$GATE_RACE_CANDIDATE/compatibility-gates.env" && -e "$GATE_RACE_STATE" ]]; then' \
  '  count=0' \
  '  read -r count <"$GATE_RACE_STATE" || true' \
  '  count=$((count + 1))' \
  '  printf "%s\n" "$count" >"$GATE_RACE_STATE"' \
  '  if [[ "$count" == 2 ]]; then' \
  '    cp "$GATE_RACE_SIGNED_GATE" "$GATE_RACE_CANDIDATE/compatibility-gates.env"' \
  '    : >"$GATE_RACE_RESTORED"' \
  '  fi' \
  'fi' \
  'exit "$status"' >"$gate_race_bin/sed"
printf '#!/usr/bin/env bash\nexit 0\n' >"$gate_race_bin/docker"
chmod +x "$gate_race_bin/openssl" "$gate_race_bin/sed" "$gate_race_bin/docker"

gate_race_output="$TMP_DIR/gate-race-output"
if PATH="$gate_race_bin:$PATH" \
  GATE_RACE_REAL_OPENSSL="$gate_race_real_openssl" \
  GATE_RACE_REAL_SED="$gate_race_real_sed" \
  GATE_RACE_CANDIDATE="$gate_race_candidate" \
  GATE_RACE_SIGNED_GATE="$gate_race_signed_gate" \
  GATE_RACE_ATTACK_GATE="$gate_race_attack_gate" \
  GATE_RACE_STATE="$gate_race_state" \
  GATE_RACE_RESTORED="$gate_race_restored" \
  MODEL_PLANE_RELEASE_MODE=1 \
  MODEL_PLANE_ARTIFACT_VERIFY_KEY="$release_public_key" \
  MODEL_PLANE_ROLLBACK_ARTIFACT_VERIFY_KEY="$rollback_public_key" \
  MODEL_PLANE_ROLLBACK_ARTIFACT_DIR="$gate_race_rollback_artifact" \
  "$ROOT_DIR/scripts/release-artifact.sh" verify "$gate_race_candidate" \
  >"$gate_race_output" 2>&1; then
  echo "release verifier accepted a gate swapped after candidate signature verification" >&2
  exit 1
fi
test -e "$gate_race_state"
if [[ -e "$gate_race_restored" ]]; then
  echo "rollback binding reopened the mutable candidate gate after signature verification" >&2
  exit 1
fi
grep -E 'artifact member changed after signed-root verification: compatibility-gates.env|artifact file checksum mismatch: compatibility-gates.env' \
  "$gate_race_output" >/dev/null

# A trusted but different rollback artifact cannot satisfy the candidate's
# content-addressed gate digest.
mismatched_rollback_output="$TMP_DIR/mismatched-rollback-output"
if MODEL_PLANE_RELEASE_MODE=1 \
  MODEL_PLANE_ARTIFACT_VERIFY_KEY="$release_public_key" \
  MODEL_PLANE_ROLLBACK_ARTIFACT_VERIFY_KEY="$mismatched_rollback_public_key" \
  MODEL_PLANE_ROLLBACK_ARTIFACT_DIR="$mismatched_rollback_artifact" \
  "$ROOT_DIR/scripts/release-artifact.sh" verify "$artifact" >"$mismatched_rollback_output" 2>&1; then
  echo "release verifier accepted a rollback artifact with the wrong root manifest" >&2
  exit 1
fi
grep -F 'rollback artifact root manifest SHA-256 does not match ROLLBACK_ARTIFACT_MANIFEST_SHA256' \
  "$mismatched_rollback_output" >/dev/null

# The rollback signature must be checked against the external trusted anchor,
# never against the rollback artifact's embedded public key.
untrusted_rollback_output="$TMP_DIR/untrusted-rollback-output"
if MODEL_PLANE_RELEASE_MODE=1 \
  MODEL_PLANE_ARTIFACT_VERIFY_KEY="$release_public_key" \
  MODEL_PLANE_ROLLBACK_ARTIFACT_VERIFY_KEY="$mismatched_rollback_public_key" \
  MODEL_PLANE_ROLLBACK_ARTIFACT_DIR="$rollback_artifact" \
  "$ROOT_DIR/scripts/release-artifact.sh" verify "$artifact" >"$untrusted_rollback_output" 2>&1; then
  echo "release verifier accepted a rollback artifact under an untrusted signer" >&2
  exit 1
fi
grep -F 'artifact signature does not match the configured verification key' "$untrusted_rollback_output" >/dev/null

# A candidate cannot call itself its own rollback, even if an operator points
# the external locator at the candidate directory.
self_rollback_artifact="$TMP_DIR/self-rollback-artifact"
self_rollback_private_key="$release_private_key"
self_rollback_public_key="$release_public_key"
make_artifact_fixture "$self_rollback_artifact" "$revision"
attest_and_sign_fixture "$self_rollback_artifact" "$revision" \
  "$self_rollback_private_key" "$self_rollback_public_key" \
  "authoritative-non-zdr-policy-attested"
self_rollback_output="$TMP_DIR/self-rollback-output"
if MODEL_PLANE_RELEASE_MODE=1 \
  MODEL_PLANE_ARTIFACT_VERIFY_KEY="$self_rollback_public_key" \
  MODEL_PLANE_ROLLBACK_ARTIFACT_VERIFY_KEY="$self_rollback_public_key" \
  MODEL_PLANE_ROLLBACK_ARTIFACT_DIR="$self_rollback_artifact" \
  "$ROOT_DIR/scripts/release-artifact.sh" verify "$self_rollback_artifact" >"$self_rollback_output" 2>&1; then
  echo "release verifier accepted a candidate as its own rollback artifact" >&2
  exit 1
fi
grep -F 'candidate artifact cannot be its own rollback artifact' "$self_rollback_output" >/dev/null

# Candidate builds must prove the external rollback artifact before Docker can
# build, pull, inspect, or save an image. The current tree remains deliberately
# dirty; the missing rollback error must win over the later source-state gate.
rollback_preflight_output="$TMP_DIR/rollback-preflight-output"
rollback_preflight_docker="$TMP_DIR/rollback-preflight-docker-args"
rollback_preflight_revision="$(git -C "$ROOT_DIR" rev-parse HEAD)"
rollback_preflight_gates="$TMP_DIR/rollback-preflight-gates.env"
sed "s/^SOURCE_REVISION=.*/SOURCE_REVISION=$rollback_preflight_revision/" \
  "$artifact/compatibility-gates.env" >"$rollback_preflight_gates"
if DOCKER_ARG_CAPTURE="$rollback_preflight_docker" \
  PATH="$TMP_DIR/bin:$PATH" \
  SOURCE_REVISION="$rollback_preflight_revision" \
  BUILD_DATE="2026-07-16T00:00:00Z" \
  SOURCE_DATE_EPOCH=0 \
  MODEL_PLANE_RELEASE_MODE=1 \
  MODEL_PLANE_COMPATIBILITY_GATES_FILE="$rollback_preflight_gates" \
  MODEL_PLANE_RUNTIME_PUBLIC_POLICY_FILE="$artifact/runtime-public-policy.env" \
  MODEL_PLANE_ARTIFACT_SIGNING_KEY="$release_private_key" \
  MODEL_PLANE_ARTIFACT_VERIFY_KEY="$release_public_key" \
  MODEL_PLANE_ROLLBACK_ARTIFACT_VERIFY_KEY="$rollback_public_key" \
  MODEL_PLANE_ROLLBACK_ARTIFACT_DIR="$TMP_DIR/does-not-exist" \
  "$ROOT_DIR/scripts/release-artifact.sh" build "$TMP_DIR/rollback-preflight-artifact" \
  >"$rollback_preflight_output" 2>&1; then
  echo "release build accepted a nonexistent rollback artifact" >&2
  exit 1
fi
if ! grep -F 'rollback artifact directory does not exist' "$rollback_preflight_output" >/dev/null; then
  echo "release preflight failed at an unexpected gate:" >&2
  sed -n '1,8p' "$rollback_preflight_output" >&2
  exit 1
fi
if [[ -e "$rollback_preflight_docker" ]]; then
  echo "release build invoked Docker before verifying the rollback artifact" >&2
  exit 1
fi

# The public compose/deploy entry points force release mode internally, so the
# same absent-locator case must fail before their artifact-contained runner can
# reach Docker. `deploy` is intentionally the same dispatch path as `compose`.
compose_missing_rollback_output="$TMP_DIR/compose-missing-rollback-output"
compose_missing_rollback_docker="$TMP_DIR/compose-missing-rollback-docker-args"
if DOCKER_ARG_CAPTURE="$compose_missing_rollback_docker" \
  PATH="$TMP_DIR/bin:$PATH" \
  MODEL_PLANE_RUNTIME_ENV_FILE="$runtime_env" \
  MODEL_PLANE_ARTIFACT_VERIFY_KEY="$release_public_key" \
  "$ROOT_DIR/scripts/release-artifact.sh" compose "$artifact" config \
  >"$compose_missing_rollback_output" 2>&1; then
  echo "release compose accepted a candidate without a rollback artifact locator" >&2
  exit 1
fi
grep -F 'MODEL_PLANE_ROLLBACK_ARTIFACT_DIR' "$compose_missing_rollback_output" >/dev/null
if [[ -e "$compose_missing_rollback_docker" ]]; then
  echo "release compose invoked Docker before verifying the rollback artifact" >&2
  exit 1
fi

deploy_missing_rollback_output="$TMP_DIR/deploy-missing-rollback-output"
if MODEL_PLANE_RUNTIME_ENV_FILE="$runtime_env" \
  MODEL_PLANE_ARTIFACT_VERIFY_KEY="$release_public_key" \
  "$ROOT_DIR/scripts/release-artifact.sh" deploy "$artifact" \
  >"$deploy_missing_rollback_output" 2>&1; then
  echo "release deploy accepted a candidate without a rollback artifact locator" >&2
  exit 1
fi
grep -F 'MODEL_PLANE_ROLLBACK_ARTIFACT_DIR' "$deploy_missing_rollback_output" >/dev/null

# A valid binding executes the current signed verifier's fixed, non-recursive
# adapter over private snapshots of rollback-contained inputs. This supports a
# predecessor that predates the command without reopening its mutable paths.
rollback_deployability_bin="$TMP_DIR/rollback-deployability-bin"
rollback_deployability_capture="$TMP_DIR/rollback-deployability-args"
mkdir -p "$rollback_deployability_bin"
printf '#!/usr/bin/env bash\nprintf "%%s\\n" "$@" > %q\n' \
  "$rollback_deployability_capture" >"$rollback_deployability_bin/docker"
chmod +x "$rollback_deployability_bin/docker"
PATH="$rollback_deployability_bin:$PATH" \
  MODEL_PLANE_RELEASE_MODE=1 \
  MODEL_PLANE_ARTIFACT_VERIFY_KEY="$release_public_key" \
  MODEL_PLANE_ROLLBACK_ARTIFACT_VERIFY_KEY="$rollback_public_key" \
  MODEL_PLANE_ROLLBACK_ARTIFACT_DIR="$rollback_artifact" \
  "$ROOT_DIR/scripts/release-artifact.sh" verify "$artifact"
grep -Fx -- 'config' "$rollback_deployability_capture" >/dev/null
grep -Fx -- '--quiet' "$rollback_deployability_capture" >/dev/null
for relative in \
  images.lock.env runtime-public-policy.env deploy/docker-compose.yml \
  deploy/docker-compose.production.yml deploy/docker-compose.release.yml; do
  grep -E "/rollback-render-view-[0-9]+/$relative$" \
    "$rollback_deployability_capture" >/dev/null
done
if grep -F "$rollback_artifact/" "$rollback_deployability_capture" >/dev/null; then
  echo "rollback renderability validation reopened mutable artifact paths" >&2
  exit 1
fi

# Artifact mode clears its parent environment before executing Compose. Replace
# the generic fake with one whose capture location is baked into the test stub.
artifact_docker_bin="$TMP_DIR/artifact-bin"
mkdir -p "$artifact_docker_bin"
capture_artifact="$TMP_DIR/artifact-compose-args"
capture_artifact_env="$TMP_DIR/artifact-compose-env"
printf '#!/usr/bin/env bash\nprintf "%%s\\n" "$@" > %q\nenv | LC_ALL=C sort > %q\n' \
  "$capture_artifact" "$capture_artifact_env" >"$artifact_docker_bin/docker"
chmod +x "$artifact_docker_bin/docker"
printf 'MODEL_POSTGRES_PASSWORD=not-a-real-secret\n' >"$runtime_env"
chmod 600 "$runtime_env"
PATH="$artifact_docker_bin:$PATH" \
MODEL_PLANE_RUNTIME_ENV_FILE="$runtime_env" \
MODEL_PLANE_PRODUCTION=1 \
MODEL_PLANE_ARTIFACT_VERIFY_KEY="$release_public_key" \
MODEL_PLANE_ROLLBACK_ARTIFACT_VERIFY_KEY="$rollback_public_key" \
MODEL_PLANE_ROLLBACK_ARTIFACT_DIR="$rollback_artifact" \
MODEL_GATEWAY_RELEASE_IMAGE='sha256:ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff' \
"$artifact/scripts/compose.sh" config --quiet
grep -Fx -- "$artifact/runtime-public-policy.env" "$capture_artifact" >/dev/null
grep -Fx -- "$artifact/images.lock.env" "$capture_artifact" >/dev/null
grep -Fx -- "$artifact/deploy/docker-compose.yml" "$capture_artifact" >/dev/null
grep -Fx -- "$artifact/deploy/docker-compose.production.yml" "$capture_artifact" >/dev/null
grep -Fx -- "$artifact/deploy/docker-compose.release.yml" "$capture_artifact" >/dev/null
if grep -Fx -- "$runtime_env" "$capture_artifact" >/dev/null; then
  echo "artifact Compose passed the mutable caller runtime file instead of its validated snapshot" >&2
  exit 1
fi
public_line="$(grep -nFx -- "$artifact/runtime-public-policy.env" "$capture_artifact" | cut -d: -f1)"
secret_line="$(grep -nE '/model-plane-runtime-env\.[^/]+/runtime\.env$' "$capture_artifact" | cut -d: -f1)"
image_line="$(grep -nFx -- "$artifact/images.lock.env" "$capture_artifact" | cut -d: -f1)"
[[ -n "$secret_line" && "$public_line" -lt "$secret_line" && "$secret_line" -lt "$image_line" ]] || {
  echo "artifact Compose did not layer signed public policy, secret snapshot, and image lock in order" >&2
  exit 1
}
if grep -F 'MODEL_GATEWAY_RELEASE_IMAGE=' "$capture_artifact_env" >/dev/null; then
  echo "artifact Compose inherited a caller-supplied image override" >&2
  exit 1
fi
if grep -E 'MODEL_PLANE_ROLLBACK_(ARTIFACT_(DIR|VERIFY_KEY)|RUNTIME_(ENV|EVIDENCE)_FILE)=' "$capture_artifact_env" >/dev/null; then
  echo "artifact Compose passed rollback locator, trust anchor, or runtime evidence path to Docker" >&2
  exit 1
fi
if grep -F "$ROOT_DIR/deploy/docker-compose.yml" "$capture_artifact" >/dev/null; then
  echo "artifact Compose fell back to the mutable workspace" >&2
  exit 1
fi

# The runtime env path must identify one absolute file for the shell validator
# and Docker Compose. A caller-relative path previously let the validator read
# the artifact's file while Docker read a same-named file from the caller CWD.
relative_runtime_cwd="$TMP_DIR/relative-runtime-cwd"
mkdir -p "$relative_runtime_cwd"
printf 'AUTH_CORE_ISSUER=https://unsigned-attacker.example/api/convex-auth\nAUTH_CORE_JWKS_URL=https://unsigned-attacker.example/api/convex-auth/jwks\nAZURE_OPENAI_ZDR_CONFIRMED=false\n' \
  >"$relative_runtime_cwd/runtime-public-policy.env"
rm -f "$capture_artifact"
if (
  cd "$relative_runtime_cwd"
  PATH="$artifact_docker_bin:$PATH" \
    MODEL_PLANE_RUNTIME_ENV_FILE=runtime-public-policy.env \
    MODEL_PLANE_PRODUCTION=1 \
    MODEL_PLANE_ARTIFACT_VERIFY_KEY="$release_public_key" \
    MODEL_PLANE_ROLLBACK_ARTIFACT_VERIFY_KEY="$rollback_public_key" \
    MODEL_PLANE_ROLLBACK_ARTIFACT_DIR="$rollback_artifact" \
    "$artifact/scripts/compose.sh" config --quiet >/dev/null 2>&1
); then
  echo "artifact Compose accepted a caller-relative runtime environment path" >&2
  exit 1
fi
if [[ -e "$capture_artifact" ]]; then
  echo "artifact Compose invoked Docker for a caller-relative runtime environment path" >&2
  exit 1
fi

# Neither the public wrapper nor the copied runner may accept caller-selected
# Compose files, env files, commands, profiles, or service overrides. Otherwise
# an unsigned trailing overlay can replace signed images, pull policy, ports,
# entrypoints, and commands after artifact verification.
unsigned_overlay="$TMP_DIR/unsigned-overlay.yml"
printf 'services:\n  model-gateway:\n    image: unsigned.invalid/gateway:attacker\n' >"$unsigned_overlay"
rm -f "$capture_artifact"
if PATH="$artifact_docker_bin:$PATH" \
  MODEL_PLANE_RUNTIME_ENV_FILE="$runtime_env" \
  MODEL_PLANE_PRODUCTION=1 \
  MODEL_PLANE_ARTIFACT_VERIFY_KEY="$release_public_key" \
  MODEL_PLANE_ROLLBACK_ARTIFACT_VERIFY_KEY="$rollback_public_key" \
  MODEL_PLANE_ROLLBACK_ARTIFACT_DIR="$rollback_artifact" \
  "$artifact/scripts/compose.sh" config --quiet -f "$unsigned_overlay" >/dev/null 2>&1; then
  echo "artifact-contained runner accepted an unsigned Compose overlay" >&2
  exit 1
fi
if [[ -e "$capture_artifact" ]]; then
  echo "artifact-contained runner invoked Docker for rejected arguments" >&2
  exit 1
fi

if PATH="$artifact_docker_bin:$PATH" \
  MODEL_PLANE_RUNTIME_ENV_FILE="$runtime_env" \
  MODEL_PLANE_ARTIFACT_VERIFY_KEY="$release_public_key" \
  MODEL_PLANE_ROLLBACK_ARTIFACT_VERIFY_KEY="$rollback_public_key" \
  MODEL_PLANE_ROLLBACK_ARTIFACT_DIR="$rollback_artifact" \
  "$ROOT_DIR/scripts/release-artifact.sh" compose "$artifact" config -f "$unsigned_overlay" >/dev/null 2>&1; then
  echo "release compose wrapper accepted an unsigned Compose overlay" >&2
  exit 1
fi

rm -f "$capture_artifact"
PATH="$artifact_docker_bin:$PATH" \
MODEL_PLANE_RUNTIME_ENV_FILE="$runtime_env" \
MODEL_PLANE_ARTIFACT_VERIFY_KEY="$release_public_key" \
MODEL_PLANE_ROLLBACK_ARTIFACT_VERIFY_KEY="$rollback_public_key" \
MODEL_PLANE_ROLLBACK_ARTIFACT_DIR="$rollback_artifact" \
"$ROOT_DIR/scripts/release-artifact.sh" deploy "$artifact"
grep -Fx -- 'up' "$capture_artifact" >/dev/null
grep -Fx -- '-d' "$capture_artifact" >/dev/null
grep -Fx -- '--wait' "$capture_artifact" >/dev/null
grep -Fx -- '--no-build' "$capture_artifact" >/dev/null
if PATH="$artifact_docker_bin:$PATH" \
  MODEL_PLANE_RUNTIME_ENV_FILE="$runtime_env" \
  MODEL_PLANE_ARTIFACT_VERIFY_KEY="$release_public_key" \
  MODEL_PLANE_ROLLBACK_ARTIFACT_VERIFY_KEY="$rollback_public_key" \
  MODEL_PLANE_ROLLBACK_ARTIFACT_DIR="$rollback_artifact" \
  "$ROOT_DIR/scripts/release-artifact.sh" deploy "$artifact" config >/dev/null 2>&1; then
  echo "release deploy accepted caller-selected Compose arguments" >&2
  exit 1
fi

grep -F 'model-gateway:' "$ROOT_DIR/deploy/docker-compose.yml" >/dev/null
gateway_health="$(sed -n '/^  model-gateway:/,/^  session-core:/p' "$ROOT_DIR/deploy/docker-compose.yml")"
inference_health="$(sed -n '/^  inference-core:/,/^  execution-core:/p' "$ROOT_DIR/deploy/docker-compose.yml")"
grep -F 'http://localhost:8080/readyz' <<<"$gateway_health" >/dev/null
grep -F 'http://localhost:8082/readyz' <<<"$inference_health" >/dev/null

echo "release contracts: ok"
