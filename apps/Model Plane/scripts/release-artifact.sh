#!/usr/bin/env bash
set -euo pipefail
umask 077

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MODEL_PLANE_REPO_PATH="apps/Model Plane"
ARTIFACT_FORMAT_VERSION="3"

SERVICES=(
  model-gateway session-core inference-core execution-core orchestrator-core
  capability-core sandbox-manager browser-broker letta-bridge cost-core bridge-core
)
CONTEXTS=(
  "$ROOT_DIR" "$ROOT_DIR" "$ROOT_DIR" "$ROOT_DIR" "$ROOT_DIR/go"
  "$ROOT_DIR/go" "$ROOT_DIR/go" "$ROOT_DIR/go" "$ROOT_DIR/go" "$ROOT_DIR/go" "$ROOT_DIR/go"
)
DOCKERFILES=(
  "$ROOT_DIR/rust/services/model-gateway/Dockerfile"
  "$ROOT_DIR/rust/services/session-core/Dockerfile"
  "$ROOT_DIR/rust/services/inference-core/Dockerfile"
  "$ROOT_DIR/rust/services/execution-core/Dockerfile"
  "$ROOT_DIR/go/services/orchestrator-core/Dockerfile"
  "$ROOT_DIR/go/services/capability-core/Dockerfile"
  "$ROOT_DIR/go/services/sandbox-manager/Dockerfile"
  "$ROOT_DIR/go/services/browser-broker/Dockerfile"
  "$ROOT_DIR/go/services/letta-bridge/Dockerfile"
  "$ROOT_DIR/go/services/cost-core/Dockerfile"
  "$ROOT_DIR/go/services/bridge-core/Dockerfile"
)
INFRASTRUCTURE_IMAGES=(
  postgres nats minio dragonfly temporal temporal-ui otel-collector
  agent-memory-redis agent-memory-server
)
INFRASTRUCTURE_REFS=(
  postgres:16.14-alpine3.24
  nats:2.12.12-alpine3.22@sha256:2ca98656a279b2d88cfdf2b8c3f0d5d7f3941ae9dc2ab12ebaa92d83e0f4ccdb
  minio/minio:RELEASE.2025-09-07T16-13-09Z
  docker.dragonflydb.io/dragonflydb/dragonfly:v1.37.0
  temporalio/auto-setup:1.27.2
  temporalio/ui:2.30.0
  otel/opentelemetry-collector-contrib:0.96.0
  redis/redis-stack-server:7.4.0-v3
  redislabs/agent-memory-server:0.15.1
)
ARTIFACT_IMAGES=("${SERVICES[@]}" "${INFRASTRUCTURE_IMAGES[@]}")
# This is the complete audited set of values that may remain external to a
# signed artifact. Any new credential-shaped Compose variable fails artifact
# construction until it is reviewed and added here deliberately. Everything
# else is either signed public configuration or artifact-owned provenance.
RUNTIME_SECRET_KEYS=(
  AGENT_MEMORY_REDIS_PASSWORD AGENT_MEMORY_TOKEN ANTHROPIC_API_KEY
  APPLICATION_CORE_INTERNAL_KEY
  APPLICATION_CONVEX_MODEL_NATS_PASSWORD APPLICATION_INSIGHT_MODEL_NATS_PASSWORD
  AUDIT_MODEL_NATS_PASSWORD AZURE_AI_LANGUAGE_KEY AZURE_ANTHROPIC_API_KEY
  AZURE_DOCUMENT_INTELLIGENCE_KEY AZURE_OPENAI_API_KEY AZURE_OPENAI_STT_API_KEY
  AZURE_OPENAI_TTS_API_KEY AZURE_OPENAI_VIDEO_API_KEY AZURE_SPEECH_KEY
  AZURE_TRANSLATOR_API_KEY AZURE_TRANSLATOR_KEY DATAPLANE_INTERNAL_KEY
  DATA_PLANE_INTERNAL_KEY EXECUTION_CORE_SERVICE_API_KEY
  EXECUTION_CORE_USER_CORE_GRPC_TOKEN EXECUTION_ORG_CORE_SERVICE_TOKEN
  EXECUTION_CORE_PROVIDER_WRITE_ATTESTATION_KEY_ID
  EXECUTION_CORE_PROVIDER_WRITE_ATTESTATION_PRIVATE_KEY
  EXECUTION_CORE_PROVIDER_WRITE_ATTESTATION_PRIVATE_KEY_NEXT
  FINETUNE_POLLER_SERVICE_API_KEY MCP_OAUTH_SERVICE_TOKEN
  MCP_TOKEN_ENCRYPTION_KEY
  INFERENCE_ROUTING_SERVICE_API_KEY INFORMATION_CORE_INTERNAL_KEY
  INTEGRATION_COREV2_INTERNAL_KEY INTERNAL_API_KEY LETTA_API_KEY
  MODEL_CAPABILITY_CORE_NATS_PASSWORD MODEL_COST_CORE_NATS_PASSWORD
  MODEL_DRAGONFLY_PASSWORD MODEL_EXECUTION_SERVICE_API_KEY
  MODEL_GATEWAY_MANAGED_START_KEY_SECRET MODEL_GATEWAY_NATS_PASSWORD
  MODEL_GATEWAY_SERVICE_API_KEY MODEL_MINIO_ROOT_PASSWORD MODEL_MINIO_ROOT_USER
  MODEL_NATS_PROVISIONER_PASSWORD MODEL_NATS_RUNTIME_PASSWORD MODEL_NATS_TOKEN
  MODEL_ORCHESTRATOR_CORE_NATS_PASSWORD ORCHESTRATOR_CORE_SERVICE_API_KEY
  ORCHESTRATOR_CORE_SERVICE_TOKEN ORCHESTRATOR_INTERNAL_SERVICE_TOKEN
  MODEL_POSTGRES_PASSWORD MODEL_SESSION_CORE_NATS_PASSWORD
  SESSION_CORE_GDPR_NATS_PASSWORD USER_CORE_GRPC_CREDENTIAL_ID
  MODEL_TEMPORAL_POSTGRES_PASSWORD MODEL_TOOL_COMPLETION_NATS_PASSWORD
  OPENAI_API_KEY SESSION_CORE_SERVICE_API_KEY SESSION_CORE_CONTINUATION_DESCRIPTOR_KEY
  CAPABILITY_CORE_INFERENCE_CORE_SERVICE_TOKEN CAPABILITY_CORE_SERVICE_API_KEY
  CAPABILITY_CORE_SESSION_CORE_SERVICE_TOKEN COST_CORE_GDPR_NATS_PASSWORD
  CAPABILITY_CORE_DECISION_SIGNING_KEY
  EXECUTION_CORE_CAPABILITY_DECISION_PUBLIC_KEY
  DEEP_RESEARCH_REPORT_TOKENS
)
DEPLOY_FILES=(
  docker-compose.yml
  docker-compose.production.yml
  docker-compose.release.yml
  nats.conf
  otel-collector-config.yaml
  seccomp-bwrap.json
)
MIGRATION_SOURCES=(
  "session-core|rust/services/session-core/migrations"
  "capability-core|go/services/capability-core/migrations"
  "cost-core|go/services/cost-core/migrations"
  "letta-bridge|go/services/letta-bridge/migrations"
)
CROSS_PLANE_DEPENDENCIES=(
  "control-plane|apps/Control Plane|apps/Control Plane/auth-core/.env.example"
  "data-plane-v2|apps/Data Plane v2|apps/Data Plane v2/.env.example"
  "frontend-v3|apps/Frontend Plane/verevonv3|apps/Frontend Plane/verevonv3/.env.example"
  "ingestion-plane|apps/Ingestion Plane|apps/Ingestion Plane/.env.example"
  "application-plane|apps/Application Plane|apps/Application Plane/.env.example"
)

usage() {
  cat <<'EOF'
Usage:
  scripts/release-artifact.sh dry-run [artifact-directory]
  scripts/release-artifact.sh build [artifact-directory]
  scripts/release-artifact.sh verify <artifact-directory>
  scripts/release-artifact.sh restore <artifact-directory>
  scripts/release-artifact.sh compose <artifact-directory> config
  scripts/release-artifact.sh deploy <artifact-directory>
  scripts/release-artifact.sh validate-deployability <artifact-directory>
  scripts/release-artifact.sh validate-lock <images.lock.env>
  scripts/release-artifact.sh validate-runtime-config <runtime.env> <config-policy.tsv> <runtime-public-policy.env> <compatibility-gates.env>
  scripts/release-artifact.sh snapshot-runtime-config <runtime.env> <config-policy.tsv> <runtime-public-policy.env> <compatibility-gates.env> <destination>

`build` creates one Docker archive per application and infrastructure image
plus an immutable release-input snapshot. It copies signed non-secret runtime
policy but never copies runtime credentials. `verify` checks the signed root
manifest, image lock, exact Compose/config/migration inputs and
cross-plane revision record. `restore` imports images only; `compose`/`deploy`
run the artifact-contained Compose files and require release-mode gates.

Environment gates for a production artifact:
  MODEL_PLANE_RELEASE_MODE=1
  MODEL_PLANE_ARTIFACT_SIGNING_KEY=/managed/private-key.pem       (build)
  MODEL_PLANE_ARTIFACT_VERIFY_KEY=/managed/public-key.pem         (verify/deploy)
  MODEL_PLANE_ROLLBACK_ARTIFACT_DIR=/accepted/previous-artifact-v3 (build/verify/deploy)
  MODEL_PLANE_ROLLBACK_ARTIFACT_VERIFY_KEY=/managed/rollback-public-key.pem (optional override)
  MODEL_PLANE_ROLLBACK_RUNTIME_ENV_FILE=/managed/previous-runtime.env (build/verify/deploy)
  MODEL_PLANE_ROLLBACK_RUNTIME_EVIDENCE_FILE=/approved/previous-runtime-evidence.env (build)
  MODEL_PLANE_COMPATIBILITY_GATES_FILE=/approved/gates.env        (build)
  MODEL_PLANE_RUNTIME_PUBLIC_POLICY_FILE=/approved/public-runtime.env (build)
  MODEL_PLANE_RUNTIME_ENV_FILE=/managed/secret-runtime.env        (compose/deploy)

The compatibility-gates file is non-secret and must attest to protocol,
migration, live authorization, approval-continuation, and a separately accepted
rollback artifact. The rollback locator is external-only: it is never copied
into the candidate artifact and must resolve to a distinct, trusted-signed root
whose actual manifest digest equals ROLLBACK_ARTIFACT_MANIFEST_SHA256. Its
ZDR_RETENTION_PATH must be either
`zdr-provider-route-attested` or `authoritative-non-zdr-policy-attested`, with
a non-secret SHA-256 evidence digest. It is an operator evidence gate, not a
substitute for live tests.

The runtime-public-policy file contains every non-secret override and is part
of the signed artifact. The external runtime file may contain only the audited
secret partition recorded in config-policy.tsv. Image IDs, SOURCE_REVISION and
BUILD_DATE remain artifact-owned and are forbidden in both runtime inputs.
EOF
}

die() {
  echo "release artifact error: $*" >&2
  exit 1
}

require_command() {
  command -v "$1" >/dev/null 2>&1 || die "required command is unavailable: $1"
}

release_mode_enabled() {
  [[ "${MODEL_PLANE_RELEASE_MODE:-0}" == "1" || "${MODEL_PLANE_REQUIRE_SIGNED_ARTIFACT:-0}" == "1" ]]
}

validate_revision() {
  [[ "$1" =~ ^[0-9a-f]{40}([0-9a-f]{24})?$ ]] ||
    die "SOURCE_REVISION must be a full 40- or 64-character lowercase hex revision"
}

validate_sha256() {
  [[ "$1" =~ ^[0-9a-f]{64}$ ]] || die "expected a lowercase SHA-256 digest"
}

validate_evidence_sha256() {
  local name="$1" value="$2"
  validate_sha256 "$value"
  [[ "$value" != "$(printf '%064d' 0)" ]] ||
    die "$name must not be a placeholder digest"
}

absolute_path() {
  if [[ "$1" = /* ]]; then
    printf '%s\n' "$1"
  else
    printf '%s/%s\n' "$ROOT_DIR" "$1"
  fi
}

canonical_directory() {
  local path="$1"
  [[ -d "$path" && ! -L "$path" ]] || die "artifact directory does not exist or is a symbolic link: $path"
  (
    cd "$path"
    pwd -P
  )
}

lock_key() {
  printf '%s_RELEASE_IMAGE' "$1" | tr '[:lower:]-' '[:upper:]_'
}

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

file_size() {
  wc -c <"$1" | tr -d '[:space:]'
}

require_regular_file() {
  [[ -f "$1" && ! -L "$1" ]] || die "required regular file is missing: $1"
}

# External release inputs cross a trust boundary before they reach OpenSSL,
# parsers, or Docker Compose. Copy each one through an O_NOFOLLOW descriptor
# into a process-private snapshot, then use only that snapshot. The fstat/lstat
# checks make path replacement and in-place mutation fail closed without ever
# printing key or runtime-environment contents.
SECURE_INPUT_TEMP_DIR=""
SECURE_INPUT_SEQUENCE=0
ARTIFACT_RENDER_SEQUENCE=0

cleanup_secure_input_snapshots() {
  if [[ -n "$SECURE_INPUT_TEMP_DIR" && -d "$SECURE_INPUT_TEMP_DIR" ]]; then
    rm -rf "$SECURE_INPUT_TEMP_DIR"
  fi
}
trap cleanup_secure_input_snapshots EXIT

secure_input_snapshot() {
  local source="$1" profile="$2" max_bytes="$3" output_name="$4" destination previous_umask
  [[ "$source" == /* ]] || die "release input paths must be absolute"
  [[ "$max_bytes" =~ ^[1-9][0-9]*$ ]] || die "release input size policy is invalid"
  case "$profile" in
    private | public | runtime) ;;
    *) die "release input security profile is invalid" ;;
  esac
  require_command python3

  if [[ -z "$SECURE_INPUT_TEMP_DIR" ]]; then
    SECURE_INPUT_TEMP_DIR="$(mktemp -d "${TMPDIR:-/tmp}/model-plane-release-inputs.XXXXXX")"
    chmod 700 "$SECURE_INPUT_TEMP_DIR"
  fi
  SECURE_INPUT_SEQUENCE=$((SECURE_INPUT_SEQUENCE + 1))
  destination="$SECURE_INPUT_TEMP_DIR/input-${SECURE_INPUT_SEQUENCE}"
  previous_umask="$(umask)"
  umask 077

  if ! python3 - "$source" "$destination" "$profile" "$max_bytes" <<'PY'
import errno
import os
import stat
import sys

source, destination, profile, raw_max_bytes = sys.argv[1:]
max_bytes = int(raw_max_bytes)


def reject(message: str) -> None:
    print(f"secure release input rejected: {message}", file=sys.stderr)
    raise SystemExit(65)


if not os.path.isabs(source):
    reject("release input path must be absolute and canonical")
# macOS exposes stable system aliases such as /var -> /private/var. Resolve the
# parent once, then walk that canonical directory descriptor-by-descriptor. The
# configured final component is still opened with O_NOFOLLOW and cannot be a
# symbolic link.
source = os.path.join(os.path.realpath(os.path.dirname(source)), os.path.basename(source))


flags = os.O_RDONLY | getattr(os, "O_CLOEXEC", 0) | getattr(os, "O_NONBLOCK", 0)
no_follow = getattr(os, "O_NOFOLLOW", 0)
directory_flag = getattr(os, "O_DIRECTORY", 0)
if not no_follow or not directory_flag:
    reject("this platform does not support O_NOFOLLOW")


def open_without_symlink_components(path: str):
    components = path.split(os.sep)
    if not path.startswith(os.sep) or not components[-1] or any(
        component in (".", "..") for component in components[1:]
    ):
        reject("release input path must be absolute and canonical")
    directory_fd = os.open(
        os.sep,
        os.O_RDONLY | directory_flag | no_follow | getattr(os, "O_CLOEXEC", 0),
    )
    try:
        for component in components[1:-1]:
            if not component:
                reject("release input path must be absolute and canonical")
            next_fd = os.open(
                component,
                os.O_RDONLY | directory_flag | no_follow | getattr(os, "O_CLOEXEC", 0),
                dir_fd=directory_fd,
            )
            os.close(directory_fd)
            directory_fd = next_fd
        opened_fd = os.open(components[-1], flags | no_follow, dir_fd=directory_fd)
        return opened_fd, directory_fd, components[-1]
    except BaseException:
        os.close(directory_fd)
        raise


try:
    source_fd, source_directory_fd, source_basename = open_without_symlink_components(source)
except OSError as error:
    if error.errno in (
        errno.ELOOP,
        errno.ENOTDIR,
        errno.EFTYPE if hasattr(errno, "EFTYPE") else -1,
    ):
        reject("release input must be a regular file opened without following symbolic links")
    reject("release input cannot be opened securely")

destination_fd = -1
try:
    before = os.fstat(source_fd)
    if not stat.S_ISREG(before.st_mode):
        reject("release input must be a regular file opened without following symbolic links")
    if before.st_size > max_bytes:
        reject(f"{profile} release input exceeds {max_bytes} bytes")

    mode = stat.S_IMODE(before.st_mode)
    effective_uid = os.geteuid()
    if profile in ("private", "runtime"):
        if before.st_uid != effective_uid:
            reject("private release input must be owned by the effective user")
        if mode & 0o077:
            reject("private release input must not grant group or other permissions")
    else:
        if before.st_uid not in (0, effective_uid):
            reject("public release input must be owned by root or the effective user")
        if mode & 0o022:
            reject("public release input must not be group or other writable")

    destination_fd = os.open(
        destination,
        os.O_WRONLY | os.O_CREAT | os.O_EXCL | no_follow | getattr(os, "O_CLOEXEC", 0),
        0o600,
    )
    os.fchmod(destination_fd, 0o600)
    total = 0
    while True:
        chunk = os.read(source_fd, min(65536, max_bytes + 1 - total))
        if not chunk:
            break
        total += len(chunk)
        if total > max_bytes:
            reject(f"{profile} release input exceeds {max_bytes} bytes")
        view = memoryview(chunk)
        while view:
            written = os.write(destination_fd, view)
            view = view[written:]
    os.fsync(destination_fd)

    after = os.fstat(source_fd)
    try:
        path_after = os.stat(source_basename, dir_fd=source_directory_fd, follow_symlinks=False)
    except OSError:
        reject("release input changed while it was being snapshotted")
    stable_fields_before = (
        before.st_dev,
        before.st_ino,
        before.st_mode,
        before.st_size,
        before.st_mtime_ns,
        before.st_ctime_ns,
    )
    stable_fields_after = (
        after.st_dev,
        after.st_ino,
        after.st_mode,
        after.st_size,
        after.st_mtime_ns,
        after.st_ctime_ns,
    )
    if stable_fields_before != stable_fields_after:
        reject("release input changed while it was being snapshotted")
    if not stat.S_ISREG(path_after.st_mode) or (
        path_after.st_dev,
        path_after.st_ino,
    ) != (before.st_dev, before.st_ino):
        reject("release input changed while it was being snapshotted")
finally:
    if destination_fd >= 0:
        os.close(destination_fd)
    os.close(source_fd)
    os.close(source_directory_fd)
PY
  then
    umask "$previous_umask"
    rm -f "$destination"
    die "$profile release input failed secure snapshot validation"
  fi
  umask "$previous_umask"

  printf -v "$output_name" '%s' "$destination"
}

publish_secure_snapshot() {
  local source="$1" destination="$2" previous_umask
  [[ "$destination" == /* ]] || die "secure snapshot destination must be absolute"
  require_command python3
  previous_umask="$(umask)"
  umask 077
  if ! python3 - "$source" "$destination" <<'PY'
import os
import stat
import sys

source, destination = sys.argv[1:]
parent = os.path.dirname(destination)


def reject(message: str) -> None:
    print(f"secure release input rejected: {message}", file=sys.stderr)
    raise SystemExit(65)


try:
    parent_stat = os.lstat(parent)
except OSError:
    reject("secure snapshot destination directory is unavailable")
if not stat.S_ISDIR(parent_stat.st_mode) or os.path.realpath(parent) != parent:
    reject("secure snapshot destination directory must be canonical and must not be a symbolic link")
if parent_stat.st_uid != os.geteuid() or stat.S_IMODE(parent_stat.st_mode) & 0o077:
    reject("secure snapshot destination directory must be private to the effective user")

no_follow = getattr(os, "O_NOFOLLOW", 0)
if not no_follow:
    reject("this platform does not support O_NOFOLLOW")
source_fd = os.open(source, os.O_RDONLY | no_follow | getattr(os, "O_CLOEXEC", 0))
destination_fd = -1
try:
    destination_fd = os.open(
        destination,
        os.O_WRONLY | os.O_CREAT | os.O_EXCL | no_follow | getattr(os, "O_CLOEXEC", 0),
        0o600,
    )
    os.fchmod(destination_fd, 0o600)
    while True:
        chunk = os.read(source_fd, 65536)
        if not chunk:
            break
        view = memoryview(chunk)
        while view:
            written = os.write(destination_fd, view)
            view = view[written:]
    os.fsync(destination_fd)
finally:
    if destination_fd >= 0:
        os.close(destination_fd)
    os.close(source_fd)
PY
  then
    umask "$previous_umask"
    rm -f "$destination"
    die "unable to publish validated runtime snapshot"
  fi
  umask "$previous_umask"
}

assignment_value() {
  local file="$1" key="$2" count
  count="$(grep -c "^${key}=" "$file" || true)"
  [[ "$count" == "1" ]] || die "file must contain exactly one ${key} assignment"
  sed -n "s/^${key}=//p" "$file"
}

non_comment_assignment_count() {
  grep -Evc '^[[:space:]]*(#|$)' "$1" || true
}

lock_value() {
  assignment_value "$1" "$2"
}

validate_lock() {
  local lock_file
  lock_file="$(absolute_path "$1")"
  require_regular_file "$lock_file"

  local revision build_date service key image allowed assignment_count line
  revision="$(lock_value "$lock_file" SOURCE_REVISION)"
  validate_revision "$revision"
  build_date="$(lock_value "$lock_file" BUILD_DATE)"
  [[ "$build_date" =~ ^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}Z$ ]] ||
    die "BUILD_DATE must use UTC RFC 3339 second precision"

  for service in "${ARTIFACT_IMAGES[@]}"; do
    key="$(lock_key "$service")"
    image="$(lock_value "$lock_file" "$key")"
    [[ "$image" =~ ^sha256:[0-9a-f]{64}$ ]] ||
      die "$key must contain only a sha256 image ID"
  done

  assignment_count="$(non_comment_assignment_count "$lock_file")"
  [[ "$assignment_count" == "$(( ${#ARTIFACT_IMAGES[@]} + 2 ))" ]] ||
    die "lock contains missing or additional assignments"
  while IFS= read -r line || [[ -n "$line" ]]; do
    [[ -z "$line" || "$line" == \#* ]] && continue
    allowed=0
    [[ "$line" == SOURCE_REVISION=* || "$line" == BUILD_DATE=* ]] && allowed=1
    for service in "${ARTIFACT_IMAGES[@]}"; do
      key="$(lock_key "$service")"
      [[ "$line" == "$key="* ]] && allowed=1
    done
    [[ "$allowed" == 1 ]] || die "lock contains an unsupported entry"
  done <"$lock_file"
}

image_lock_payload_sha256() {
  local lock_file="$1"
  validate_lock "$lock_file"
  # Revision and build timestamp are provenance, not executable payload. A
  # rollback must change at least one content-addressed image, so compare only
  # the normalized release-image assignments.
  sed -nE '/^[A-Z0-9_]+_RELEASE_IMAGE=sha256:[0-9a-f]{64}$/p' "$lock_file" |
    LC_ALL=C sort | sha256_stdin
}

validate_image_manifest() {
  local artifact_dir="$1" manifest="$artifact_dir/manifest.tsv"
  require_regular_file "$manifest"
  [[ "$(head -n 1 "$manifest")" == $'service\tsource_revision\timage_id\tarchive\tarchive_sha256' ]] ||
    die "image manifest header is invalid"

  local expected_rows="$(( ${#ARTIFACT_IMAGES[@]} + 1 ))" actual_rows
  actual_rows="$(wc -l <"$manifest" | tr -d '[:space:]')"
  [[ "$actual_rows" == "$expected_rows" ]] || die "image manifest has missing or additional rows"

  local service rows expected revision image_id archive archive_sha expected_image
  revision="$(lock_value "$artifact_dir/images.lock.env" SOURCE_REVISION)"
  for service in "${ARTIFACT_IMAGES[@]}"; do
    rows="$(awk -F '\t' -v service="$service" '$1 == service { count++ } END { print count + 0 }' "$manifest")"
    [[ "$rows" == "1" ]] || die "image manifest must contain exactly one $service row"
    IFS=$'\t' read -r _service _revision image_id archive archive_sha < <(
      awk -F '\t' -v service="$service" '$1 == service { print $0 }' "$manifest"
    )
    [[ "$_service" == "$service" && "$_revision" == "$revision" ]] ||
      die "image manifest revision mismatch for $service"
    expected_image="$(lock_value "$artifact_dir/images.lock.env" "$(lock_key "$service")")"
    [[ "$image_id" == "$expected_image" ]] || die "image manifest image ID mismatch for $service"
    [[ "$archive" == "${service}.docker.tar" ]] || die "image manifest archive name mismatch for $service"
    validate_sha256 "$archive_sha"
    require_regular_file "$artifact_dir/$archive"
    [[ "$(sha256_file "$artifact_dir/$archive")" == "$archive_sha" ]] ||
      die "archive checksum mismatch for $service"
  done
}

safe_artifact_relative_path() {
  local path="$1"
  [[ -n "$path" && "$path" != /* && "$path" != *".."* && "$path" != *$'\n'* && "$path" != *$'\r'* ]] ||
    die "artifact manifest contains an unsafe path"
  [[ "$path" =~ ^[A-Za-z0-9._/@+=:-]+$ ]] || die "artifact manifest path contains unsupported characters"
}

generate_root_manifest() {
  local artifact_dir="$1" manifest="$artifact_dir/artifact-manifest.tsv"
  (
    cd "$artifact_dir"
    printf 'path\tkind\tsha256\tbytes\n'
    while IFS= read -r file; do
      file="${file#./}"
      printf '%s\tfile\t%s\t%s\n' "$file" "$(sha256_file "$file")" "$(file_size "$file")"
    done < <(find . -type f ! -name artifact-manifest.tsv ! -name artifact-manifest.sig ! -name .complete | LC_ALL=C sort)
  ) >"$manifest"
}

validate_root_manifest() {
  local artifact_dir="$1" manifest="$artifact_dir/artifact-manifest.tsv"
  require_regular_file "$manifest"
  [[ "$(head -n 1 "$manifest")" == $'path\tkind\tsha256\tbytes' ]] ||
    die "root artifact manifest header is invalid"

  local seen expected_files actual_files path kind expected_hash expected_size extra count=0
  seen="$(mktemp "${TMPDIR:-/tmp}/model-plane-artifact-seen.XXXXXX")"
  expected_files="$(mktemp "${TMPDIR:-/tmp}/model-plane-artifact-expected.XXXXXX")"
  actual_files="$(mktemp "${TMPDIR:-/tmp}/model-plane-artifact-actual.XXXXXX")"
  trap 'rm -f "$seen" "$expected_files" "$actual_files"' RETURN

  while IFS=$'\t' read -r path kind expected_hash expected_size extra || [[ -n "${path:-}" ]]; do
    [[ "$path" == "path" ]] && continue
    [[ -n "$path" && -n "$kind" && -n "$expected_hash" && -n "$expected_size" && -z "${extra:-}" ]] ||
      die "root artifact manifest row is malformed"
    safe_artifact_relative_path "$path"
    [[ "$kind" == "file" ]] || die "root artifact manifest kind is unsupported"
    validate_sha256 "$expected_hash"
    [[ "$expected_size" =~ ^[0-9]+$ ]] || die "root artifact manifest size is invalid"
    if grep -Fqx -- "$path" "$seen"; then
      die "root artifact manifest contains a duplicate path"
    fi
    printf '%s\n' "$path" >>"$seen"
    require_regular_file "$artifact_dir/$path"
    [[ "$(file_size "$artifact_dir/$path")" == "$expected_size" ]] ||
      die "artifact file size mismatch: $path"
    [[ "$(sha256_file "$artifact_dir/$path")" == "$expected_hash" ]] ||
      die "artifact file checksum mismatch: $path"
    printf '%s\n' "$path" >>"$expected_files"
    count=$((count + 1))
  done <"$manifest"
  [[ "$count" -gt 0 ]] || die "root artifact manifest contains no files"

  (
    cd "$artifact_dir"
    find . -type f ! -name artifact-manifest.tsv ! -name artifact-manifest.sig ! -name .complete |
      sed 's#^./##' | LC_ALL=C sort
  ) >"$actual_files"
  LC_ALL=C sort -u "$expected_files" -o "$expected_files"
  diff -u "$expected_files" "$actual_files" >/dev/null ||
    die "artifact contains an unmanifested or missing regular file"
  trap - RETURN
  rm -f "$seen" "$expected_files" "$actual_files"
}

snapshot_signed_root_manifest() {
  local artifact_dir="$1" verify_key="$2" output_name="$3"
  local root_snapshot signature_snapshot key_snapshot
  secure_input_snapshot "$artifact_dir/artifact-manifest.tsv" public 1048576 root_snapshot
  secure_input_snapshot "$artifact_dir/artifact-manifest.sig" public 65536 signature_snapshot
  secure_input_snapshot "$verify_key" public 65536 key_snapshot
  require_command openssl
  openssl dgst -sha256 -verify "$key_snapshot" -signature "$signature_snapshot" \
    "$root_snapshot" >/dev/null 2>&1 ||
    die "artifact manifest snapshot does not match the trusted verification key"
  [[ "$(head -n 1 "$root_snapshot")" == $'path\tkind\tsha256\tbytes' ]] ||
    die "root artifact manifest header is invalid"
  printf -v "$output_name" '%s' "$root_snapshot"
}

snapshot_signed_artifact_member() {
  local artifact_dir="$1" manifest="$2" relative="$3" destination_root="$4"
  local output_name="$5" rows path kind expected_hash expected_size extra source_snapshot destination parent
  safe_artifact_relative_path "$relative"
  rows="$(awk -F '\t' -v path="$relative" '$1 == path { count++ } END { print count + 0 }' "$manifest")"
  [[ "$rows" == "1" ]] || die "signed artifact member is missing or duplicated: $relative"
  IFS=$'\t' read -r path kind expected_hash expected_size extra < <(
    awk -F '\t' -v wanted="$relative" '$1 == wanted { print $0 }' "$manifest"
  )
  [[ "$path" == "$relative" && "$kind" == "file" && -z "${extra:-}" ]] ||
    die "signed artifact member row is malformed: $relative"
  validate_sha256 "$expected_hash"
  [[ "$expected_size" =~ ^[0-9]+$ && "$expected_size" -le 8388608 ]] ||
    die "signed deployability input has an invalid or excessive size: $relative"
  secure_input_snapshot "$artifact_dir/$relative" public 8388608 source_snapshot
  [[ "$(file_size "$source_snapshot")" == "$expected_size" && \
     "$(sha256_file "$source_snapshot")" == "$expected_hash" ]] ||
    die "artifact member changed after signed-root verification: $relative"
  destination="$destination_root/$relative"
  parent="$(dirname "$destination")"
  mkdir -p "$parent"
  chmod 700 "$parent"
  parent="$(canonical_directory "$parent")"
  destination="$parent/$(basename "$destination")"
  publish_secure_snapshot "$source_snapshot" "$destination"
  printf -v "$output_name" '%s' "$destination"
}

create_artifact_render_snapshot() {
  local artifact_dir="$1" verify_key="$2" label="$3" output_root_name="$4" output_manifest_name="$5"
  local manifest_snapshot snapshot_root relative ignored
  snapshot_signed_root_manifest "$artifact_dir" "$verify_key" manifest_snapshot
  ARTIFACT_RENDER_SEQUENCE=$((ARTIFACT_RENDER_SEQUENCE + 1))
  snapshot_root="$(canonical_directory "$SECURE_INPUT_TEMP_DIR")/${label}-${ARTIFACT_RENDER_SEQUENCE}"
  [[ ! -e "$snapshot_root" ]] || die "artifact render snapshot path already exists"
  mkdir "$snapshot_root"
  chmod 700 "$snapshot_root"
  snapshot_root="$(canonical_directory "$snapshot_root")"
  for relative in \
    images.lock.env config-policy.tsv runtime-public-policy.env compatibility-gates.env \
    scripts/release-artifact.sh scripts/compose.sh; do
    snapshot_signed_artifact_member "$artifact_dir" "$manifest_snapshot" "$relative" \
      "$snapshot_root" ignored
  done
  if awk -F '\t' '$1 == "rollback-runtime-evidence.env" { found = 1 } END { exit(found ? 0 : 1) }' \
    "$manifest_snapshot"; then
    snapshot_signed_artifact_member "$artifact_dir" "$manifest_snapshot" \
      rollback-runtime-evidence.env "$snapshot_root" ignored
  fi
  for relative in "${DEPLOY_FILES[@]}"; do
    snapshot_signed_artifact_member "$artifact_dir" "$manifest_snapshot" "deploy/$relative" \
      "$snapshot_root" ignored
  done
  printf -v "$output_root_name" '%s' "$snapshot_root"
  printf -v "$output_manifest_name" '%s' "$manifest_snapshot"
}

is_audited_runtime_secret_key() {
  local candidate="$1" key
  for key in "${RUNTIME_SECRET_KEYS[@]}"; do
    [[ "$candidate" == "$key" ]] && return 0
  done
  return 1
}

is_artifact_owned_config_key() {
  local candidate="$1" image
  case "$candidate" in
    SOURCE_REVISION|BUILD_DATE) return 0 ;;
  esac
  for image in "${ARTIFACT_IMAGES[@]}"; do
    [[ "$candidate" == "$(lock_key "$image")" ]] && return 0
  done
  return 1
}

config_partition_for_key() {
  local key="$1"
  if is_audited_runtime_secret_key "$key"; then
    printf 'secret\n'
  elif is_artifact_owned_config_key "$key"; then
    printf 'artifact\n'
  elif [[ "$key" == *_RELEASE_IMAGE ]]; then
    die "release image config key is not owned by the artifact image set: $key"
  elif [[ "$key" =~ (PASSWORD|TOKEN|SECRET|KEY|CREDENTIAL) ]]; then
    die "credential-shaped config key is not in the audited secret set: $key"
  else
    printf 'public\n'
  fi
}

policy_partition() {
  local policy="$1" key="$2" count partition
  count="$(awk -F '\t' -v key="$key" 'NR > 1 && $1 == key { count++ } END { print count + 0 }' "$policy")"
  [[ "$count" == "1" ]] || return 1
  partition="$(awk -F '\t' -v key="$key" 'NR > 1 && $1 == key { print $2 }' "$policy")"
  printf '%s\n' "$partition"
}

validate_config_policy() {
  local policy="$1" key partition extra expected count=0
  require_regular_file "$policy"
  [[ "$(head -n 1 "$policy")" == $'key\tpartition' ]] || die "config policy header is invalid"
  while IFS=$'\t' read -r key partition extra || [[ -n "${key:-}" ]]; do
    [[ "$key" == "key" ]] && continue
    [[ "$key" =~ ^[A-Z][A-Z0-9_]*$ && -z "${extra:-}" ]] ||
      die "config policy contains an invalid key"
    case "$partition" in public|secret|artifact) ;; *) die "config policy partition is invalid" ;; esac
    expected="$(config_partition_for_key "$key")"
    [[ "$partition" == "$expected" ]] ||
      die "config policy assigns $key to the wrong partition"
    count=$((count + 1))
  done <"$policy"
  [[ "$count" -gt 0 ]] || die "config policy contains no keys"
  [[ "$(sed '1d' "$policy" | cut -f1 | LC_ALL=C sort | uniq -d | wc -l | tr -d '[:space:]')" == "0" ]] ||
    die "config policy contains duplicate keys"
}

validate_restricted_dotenv() {
  local file="$1" description="$2" line key seen assignments=0
  local assignment_pattern='^[A-Z][A-Z0-9_]*=[A-Za-z0-9._~:/@,+%?&=*-]*$'
  seen="$(mktemp "${TMPDIR:-/tmp}/model-plane-dotenv-seen.XXXXXX")"
  trap 'rm -f "$seen"' RETURN
  while IFS= read -r line || [[ -n "$line" ]]; do
    [[ -z "$line" ]] && continue
    [[ "$line" =~ $assignment_pattern ]] ||
      die "$description violates the restricted dotenv grammar"
    key="${line%%=*}"
    if grep -Fqx -- "$key" "$seen"; then
      die "$description violates the restricted dotenv grammar: duplicate key"
    fi
    printf '%s\n' "$key" >>"$seen"
    assignments=$((assignments + 1))
  done <"$file"
  [[ "$assignments" -gt 0 ]] || die "$description contains no assignments"
  trap - RETURN
  rm -f "$seen"
}

validate_https_public_url() {
  local name="$1" value="$2" authority
  [[ "$value" == https://* && "$value" != *[[:space:]]* && "$value" != *"@"* && "$value" != *"?"* && "$value" != *"#"* ]] ||
    die "$name must be a canonical HTTPS URL without credentials, query, or fragment"
  authority="${value#https://}"
  authority="${authority%%/*}"
  [[ -n "$authority" && "$authority" != .* && "$authority" != *..* ]] ||
    die "$name has an invalid HTTPS authority"
}

validate_runtime_public_policy() {
  local policy_source="$1" gates_source="$2" config_source="$3"
  local policy_file gates_file config_file
  secure_input_snapshot "$policy_source" public 65536 policy_file
  secure_input_snapshot "$gates_source" public 65536 gates_file
  secure_input_snapshot "$config_source" public 65536 config_file
  validate_runtime_public_policy_files "$policy_file" "$gates_file" "$config_file"
}

validate_runtime_public_policy_files() {
  local policy_file="$1" gates_file="$2" config_file="$3"
  local issuer jwks zdr line key value partition
  validate_config_policy "$config_file"
  validate_restricted_dotenv "$policy_file" "runtime public policy"
  while IFS= read -r line || [[ -n "$line" ]]; do
    [[ -z "$line" ]] && continue
    key="${line%%=*}"
    partition="$(policy_partition "$config_file" "$key" || true)"
    [[ -n "$partition" ]] ||
      die "runtime public policy key is not allowed by the artifact policy"
    [[ "$partition" == "public" ]] ||
      die "signed runtime public policy permits public keys only"
    value="${line#*=}"
    if [[ "$key" =~ (_URL|_ENDPOINT|_BASE)$ && "$value" == *://* && "$value" == *"@"* ]]; then
      die "runtime public URL must not contain credentials"
    fi
  done <"$policy_file"

  issuer="$(assignment_value "$policy_file" AUTH_CORE_ISSUER)"
  jwks="$(assignment_value "$policy_file" AUTH_CORE_JWKS_URL)"
  zdr="$(assignment_value "$policy_file" AZURE_OPENAI_ZDR_CONFIRMED)"
  validate_https_public_url AUTH_CORE_ISSUER "$issuer"
  validate_https_public_url AUTH_CORE_JWKS_URL "$jwks"
  # The issuer is the canonical token claim while the JWKS URL is a network
  # route; deployments commonly use different authorities. Both values are
  # independently signed. A passed release gate must bind their live pairing
  # to evidence rather than inferring topology from string concatenation.
  if [[ "$(assignment_value "$gates_file" STATUS)" == "passed" ]]; then
    [[ "$(grep -c '^AUTH_IDENTITY_EVIDENCE_SHA256=' "$gates_file" || true)" == "1" ]] ||
      die "passed release requires Auth issuer/JWKS live-binding evidence"
    validate_evidence_sha256 AUTH_IDENTITY_EVIDENCE_SHA256 \
      "$(assignment_value "$gates_file" AUTH_IDENTITY_EVIDENCE_SHA256)"
  fi
  [[ "$zdr" == "false" || "$zdr" == "true" ]] ||
    die "AZURE_OPENAI_ZDR_CONFIRMED must be true or false"
  if [[ "$zdr" == "true" ]]; then
    local required route_value endpoint api_version region chat_deployments
    local embedding_deployments provider_order residency legacy
    [[ "$(compatibility_gate_status "$gates_file" "$(assignment_value "$gates_file" SOURCE_REVISION)")" == "passed" ]] ||
      die "ZDR provider confirmation requires passed compatibility gates"
    [[ "$(assignment_value "$gates_file" ZDR_RETENTION_PATH)" == "zdr-provider-route-attested" ]] ||
      die "ZDR provider confirmation requires provider-route attestation"
    for required in AZURE_OPENAI_ENDPOINT AZURE_OPENAI_API_VERSION AZURE_OPENAI_REGION \
      AZURE_OPENAI_CHAT_DEPLOYMENTS AZURE_OPENAI_EMBEDDING_DEPLOYMENTS \
      INFERENCE_PROVIDER_ORDER MODEL_PLANE_RESIDENCY; do
      [[ "$(grep -c "^${required}=" "$policy_file" || true)" == "1" ]] ||
        die "ZDR provider confirmation requires signed $required"
      route_value="$(assignment_value "$policy_file" "$required")"
      [[ -n "$route_value" ]] ||
        die "ZDR provider confirmation requires signed $required"
    done
    endpoint="$(assignment_value "$policy_file" AZURE_OPENAI_ENDPOINT)"
    api_version="$(assignment_value "$policy_file" AZURE_OPENAI_API_VERSION)"
    region="$(assignment_value "$policy_file" AZURE_OPENAI_REGION)"
    chat_deployments="$(assignment_value "$policy_file" AZURE_OPENAI_CHAT_DEPLOYMENTS)"
    embedding_deployments="$(assignment_value "$policy_file" AZURE_OPENAI_EMBEDDING_DEPLOYMENTS)"
    provider_order="$(assignment_value "$policy_file" INFERENCE_PROVIDER_ORDER)"
    residency="$(assignment_value "$policy_file" MODEL_PLANE_RESIDENCY)"
    validate_https_public_url AZURE_OPENAI_ENDPOINT "$endpoint"
    [[ "$api_version" =~ ^[0-9]{4}-[0-9]{2}-[0-9]{2}(-preview)?$ ]] ||
      die "ZDR provider route has an invalid Azure API version"
    [[ "$region" =~ ^[a-z0-9-]+$ && "$residency" == "$region" ]] ||
      die "ZDR provider route region and residency must match"
    [[ "$chat_deployments" =~ ^[A-Za-z0-9._-]+(,[A-Za-z0-9._-]+)*$ ]] ||
      die "ZDR provider route has an invalid chat deployment catalog"
    [[ "$embedding_deployments" =~ ^[A-Za-z0-9._-]+(,[A-Za-z0-9._-]+)*$ ]] ||
      die "ZDR provider route has an invalid embedding deployment catalog"
    [[ "${provider_order%%,*}" == "azure" || "${provider_order%%,*}" == "azure-openai" ]] ||
      die "ZDR provider order must start with azure"
    for legacy in AZURE_OPENAI_DEPLOYMENT AZURE_OPENAI_EMBEDDING_DEPLOYMENT; do
      if grep -q "^${legacy}=" "$policy_file" && [[ -n "$(assignment_value "$policy_file" "$legacy")" ]]; then
        die "ZDR provider route forbids legacy deployment aliases"
      fi
    done
  fi
}

validate_runtime_config() {
  local runtime_source policy_source public_policy_source gates_source
  local runtime_file policy_file public_policy gates_file line key partition assignments
  runtime_source="$(absolute_path "$1")"
  policy_source="$(absolute_path "$2")"
  public_policy_source="$(absolute_path "$3")"
  gates_source="$(absolute_path "$4")"
  secure_input_snapshot "$runtime_source" runtime 1048576 runtime_file
  secure_input_snapshot "$policy_source" public 65536 policy_file
  secure_input_snapshot "$public_policy_source" public 65536 public_policy
  secure_input_snapshot "$gates_source" public 65536 gates_file
  validate_config_policy "$policy_file"
  validate_runtime_public_policy_files "$public_policy" "$gates_file" "$policy_file"
  validate_restricted_dotenv "$runtime_file" "runtime config"

  while IFS= read -r line || [[ -n "$line" ]]; do
    [[ -z "$line" ]] && continue
    key="${line%%=*}"
    partition="$(policy_partition "$policy_file" "$key" || true)"
    [[ -n "$partition" ]] ||
      die "runtime config key is not allowed by the artifact policy"
    [[ "$partition" == "secret" ]] ||
      die "external runtime config permits secret keys only"
  done <"$runtime_file"
  assignments="$(assignment_value "$public_policy" AZURE_OPENAI_ZDR_CONFIRMED)"
  if [[ "$assignments" == "true" ]]; then
    [[ "$(grep -c '^AZURE_OPENAI_API_KEY=' "$runtime_file" || true)" == "1" ]] ||
      die "ZDR provider route requires AZURE_OPENAI_API_KEY in the secret runtime file"
    [[ "$(assignment_value "$runtime_file" AZURE_OPENAI_API_KEY)" != "" ]] ||
      die "ZDR provider route requires AZURE_OPENAI_API_KEY in the secret runtime file"
  fi
}

snapshot_runtime_config() {
  local runtime_source="$1" policy_file="$2" public_policy="$3" gates_file="$4" destination="$5"
  local runtime_snapshot
  runtime_source="$(absolute_path "$runtime_source")"
  secure_input_snapshot "$runtime_source" runtime 1048576 runtime_snapshot
  validate_runtime_config "$runtime_snapshot" "$policy_file" "$public_policy" "$gates_file"
  publish_secure_snapshot "$runtime_snapshot" "$destination"
}

secret_schema_sha256() {
  local policy="$1"
  validate_config_policy "$policy"
  awk -F '\t' 'NR > 1 && $2 == "secret" { print $1 }' "$policy" |
    LC_ALL=C sort | sha256_stdin
}

runtime_keyset_sha256() {
  local runtime_file="$1"
  validate_restricted_dotenv "$runtime_file" "rollback runtime config"
  sed -nE 's/^([A-Z][A-Z0-9_]*)=.*/\1/p' "$runtime_file" |
    LC_ALL=C sort | sha256_stdin
}

validate_rollback_runtime_evidence() {
  local evidence_source="$1" expected_manifest_sha="$2" policy_source="$3" runtime_source="$4"
  local evidence policy runtime line key assignments expected_schema expected_runtime_keys
  secure_input_snapshot "$evidence_source" public 65536 evidence
  secure_input_snapshot "$policy_source" public 65536 policy
  secure_input_snapshot "$runtime_source" runtime 1048576 runtime
  validate_restricted_dotenv "$evidence" "rollback runtime evidence"
  assignments="$(non_comment_assignment_count "$evidence")"
  [[ "$assignments" == "6" ]] ||
    die "rollback runtime evidence must contain exactly six assignments"
  while IFS= read -r line || [[ -n "$line" ]]; do
    [[ -z "$line" ]] && continue
    key="${line%%=*}"
    case "$key" in
      ROLLBACK_RUNTIME_EVIDENCE_VERSION|ROLLBACK_ARTIFACT_MANIFEST_SHA256|SECRET_SCHEMA_SHA256|RUNTIME_KEYSET_SHA256|SECRET_VERSION_REFERENCE_SHA256|STATUS) ;;
      *) die "rollback runtime evidence contains an unsupported assignment" ;;
    esac
  done <"$evidence"
  [[ "$(assignment_value "$evidence" ROLLBACK_RUNTIME_EVIDENCE_VERSION)" == "1" ]] ||
    die "rollback runtime evidence version is unsupported"
  [[ "$(assignment_value "$evidence" STATUS)" == "rollback-config-renderable" ]] ||
    die "rollback runtime evidence is not approved for config renderability"
  validate_sha256 "$(assignment_value "$evidence" ROLLBACK_ARTIFACT_MANIFEST_SHA256)"
  validate_sha256 "$(assignment_value "$evidence" SECRET_SCHEMA_SHA256)"
  validate_sha256 "$(assignment_value "$evidence" RUNTIME_KEYSET_SHA256)"
  validate_evidence_sha256 SECRET_VERSION_REFERENCE_SHA256 \
    "$(assignment_value "$evidence" SECRET_VERSION_REFERENCE_SHA256)"
  [[ "$(assignment_value "$evidence" ROLLBACK_ARTIFACT_MANIFEST_SHA256)" == "$expected_manifest_sha" ]] ||
    die "rollback runtime evidence is bound to a different rollback artifact"
  expected_schema="$(secret_schema_sha256 "$policy")"
  [[ "$(assignment_value "$evidence" SECRET_SCHEMA_SHA256)" == "$expected_schema" ]] ||
    die "rollback runtime evidence secret schema does not match the rollback artifact"
  expected_runtime_keys="$(runtime_keyset_sha256 "$runtime")"
  [[ "$(assignment_value "$evidence" RUNTIME_KEYSET_SHA256)" == "$expected_runtime_keys" ]] ||
    die "rollback runtime evidence keyset does not match the supplied secret runtime"
}

validate_two_column_file_manifest() {
  local artifact_dir="$1" manifest="$2" header="$3" prefix="$4" expected_paths="$5"
  require_regular_file "$manifest"
  [[ "$(head -n 1 "$manifest")" == "$header" ]] || die "manifest header is invalid: $(basename "$manifest")"

  local path expected_hash extra rows=0 expected_path found
  while IFS=$'\t' read -r path expected_hash extra || [[ -n "${path:-}" ]]; do
    [[ "$path" == "${header%%$'\t'*}" ]] && continue
    [[ -n "$path" && -n "$expected_hash" && -z "${extra:-}" ]] || die "manifest row is malformed: $(basename "$manifest")"
    safe_artifact_relative_path "$path"
    [[ "$path" == "$prefix"* ]] || die "manifest path is outside its allowed prefix"
    validate_sha256 "$expected_hash"
    require_regular_file "$artifact_dir/$path"
    [[ "$(sha256_file "$artifact_dir/$path")" == "$expected_hash" ]] ||
      die "manifest checksum mismatch: $path"
    rows=$((rows + 1))
  done <"$manifest"
  [[ "$rows" -gt 0 ]] || die "manifest contains no rows: $(basename "$manifest")"
  [[ "$(sed '1d' "$manifest" | cut -f1 | LC_ALL=C sort | uniq -d | wc -l | tr -d '[:space:]')" == "0" ]] ||
    die "manifest contains duplicate paths: $(basename "$manifest")"

  while IFS= read -r expected_path || [[ -n "$expected_path" ]]; do
    [[ -z "$expected_path" ]] && continue
    found="$(awk -F '\t' -v path="$expected_path" '$1 == path { count++ } END { print count + 0 }' "$manifest")"
    [[ "$found" == "1" ]] || die "manifest must contain exactly one required path: $expected_path"
  done <<<"$expected_paths"
}

validate_cross_plane_dependencies() {
  local artifact_dir="$1" file="$artifact_dir/cross-plane-dependencies.tsv"
  require_regular_file "$file"
  [[ "$(head -n 1 "$file")" == $'dependency\trepo_revision\tgit_tree\tenv_keyset_sha256' ]] ||
    die "cross-plane dependency header is invalid"

  local dependency revision tree keyset extra rows=0 spec expected count
  while IFS=$'\t' read -r dependency revision tree keyset extra || [[ -n "${dependency:-}" ]]; do
    [[ "$dependency" == "dependency" ]] && continue
    [[ -n "$dependency" && -n "$revision" && -n "$tree" && -n "$keyset" && -z "${extra:-}" ]] ||
      die "cross-plane dependency row is malformed"
    validate_revision "$revision"
    validate_revision "$tree"
    validate_sha256 "$keyset"
    case "$dependency" in
      control-plane|data-plane-v2|frontend-v3|ingestion-plane|application-plane) ;;
      *) die "cross-plane dependency is not an approved runtime boundary" ;;
    esac
    rows=$((rows + 1))
  done <"$file"
  [[ "$rows" == "${#CROSS_PLANE_DEPENDENCIES[@]}" ]] || die "cross-plane dependency count is invalid"
  for spec in "${CROSS_PLANE_DEPENDENCIES[@]}"; do
    expected="${spec%%|*}"
    count="$(awk -F '\t' -v dependency="$expected" '$1 == dependency { count++ } END { print count + 0 }' "$file")"
    [[ "$count" == "1" ]] || die "cross-plane dependency is missing or duplicated: $expected"
  done
}

validate_metadata() {
  local artifact_dir="$1" metadata="$artifact_dir/artifact-metadata.env"
  require_regular_file "$metadata"
  local expected_keys=(
    ARTIFACT_FORMAT_VERSION SOURCE_REVISION BUILD_DATE SOURCE_DATE_EPOCH MODEL_PLANE_GIT_TREE
    CONFIG_POLICY_SHA256 RUNTIME_PUBLIC_POLICY_SHA256 COMPOSE_INPUTS_SHA256 MIGRATION_MANIFEST_SHA256
    CROSS_PLANE_DEPENDENCIES_SHA256 COMPATIBILITY_GATES_SHA256
  )
  local key value line allowed assignments
  assignments="$(non_comment_assignment_count "$metadata")"
  [[ "$assignments" == "${#expected_keys[@]}" ]] || die "artifact metadata contains missing or additional assignments"
  while IFS= read -r line || [[ -n "$line" ]]; do
    [[ -z "$line" || "$line" == \#* ]] && continue
    [[ "$line" == *=* ]] || die "artifact metadata contains a malformed line"
    key="${line%%=*}"
    allowed=0
    for value in "${expected_keys[@]}"; do
      [[ "$key" == "$value" ]] && allowed=1
    done
    [[ "$allowed" == 1 ]] || die "artifact metadata contains an unsupported assignment"
  done <"$metadata"

  [[ "$(assignment_value "$metadata" ARTIFACT_FORMAT_VERSION)" == "$ARTIFACT_FORMAT_VERSION" ]] ||
    die "artifact format version is unsupported"
  validate_revision "$(assignment_value "$metadata" SOURCE_REVISION)"
  [[ "$(assignment_value "$metadata" BUILD_DATE)" =~ ^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}Z$ ]] ||
    die "artifact metadata build date is invalid"
  [[ "$(assignment_value "$metadata" SOURCE_DATE_EPOCH)" =~ ^[0-9]+$ ]] ||
    die "artifact metadata source date epoch is invalid"
  validate_revision "$(assignment_value "$metadata" MODEL_PLANE_GIT_TREE)"

  [[ "$(assignment_value "$metadata" SOURCE_REVISION)" == "$(lock_value "$artifact_dir/images.lock.env" SOURCE_REVISION)" ]] ||
    die "artifact metadata and image lock revisions differ"
  [[ "$(assignment_value "$metadata" BUILD_DATE)" == "$(lock_value "$artifact_dir/images.lock.env" BUILD_DATE)" ]] ||
    die "artifact metadata and image lock build dates differ"

  local metadata_key artifact_file
  for metadata_key in CONFIG_POLICY_SHA256 RUNTIME_PUBLIC_POLICY_SHA256 COMPOSE_INPUTS_SHA256 MIGRATION_MANIFEST_SHA256 CROSS_PLANE_DEPENDENCIES_SHA256 COMPATIBILITY_GATES_SHA256; do
    validate_sha256 "$(assignment_value "$metadata" "$metadata_key")"
  done
  [[ "$(assignment_value "$metadata" CONFIG_POLICY_SHA256)" == "$(sha256_file "$artifact_dir/config-policy.tsv")" ]] ||
    die "config policy hash differs from metadata"
  [[ "$(assignment_value "$metadata" RUNTIME_PUBLIC_POLICY_SHA256)" == "$(sha256_file "$artifact_dir/runtime-public-policy.env")" ]] ||
    die "runtime public policy hash differs from metadata"
  [[ "$(assignment_value "$metadata" COMPOSE_INPUTS_SHA256)" == "$(sha256_file "$artifact_dir/compose-inputs.tsv")" ]] ||
    die "Compose input hash differs from metadata"
  [[ "$(assignment_value "$metadata" MIGRATION_MANIFEST_SHA256)" == "$(sha256_file "$artifact_dir/migration-manifest.tsv")" ]] ||
    die "migration manifest hash differs from metadata"
  [[ "$(assignment_value "$metadata" CROSS_PLANE_DEPENDENCIES_SHA256)" == "$(sha256_file "$artifact_dir/cross-plane-dependencies.tsv")" ]] ||
    die "cross-plane dependency hash differs from metadata"
  [[ "$(assignment_value "$metadata" COMPATIBILITY_GATES_SHA256)" == "$(sha256_file "$artifact_dir/compatibility-gates.env")" ]] ||
    die "compatibility gate hash differs from metadata"
}

compatibility_gate_status() {
  local gates="$1" revision="$2" status line assignments
  require_regular_file "$gates"
  [[ "$(assignment_value "$gates" COMPATIBILITY_GATES_VERSION)" == "1" ]] ||
    die "compatibility gates version is unsupported"
  [[ "$(assignment_value "$gates" SOURCE_REVISION)" == "$revision" ]] ||
    die "compatibility gates do not match the artifact source revision"
  status="$(assignment_value "$gates" STATUS)"
  case "$status" in
    not-attested)
      assignments="$(non_comment_assignment_count "$gates")"
      [[ "$assignments" == "3" ]] || die "unattested compatibility gates contain unsupported assignments"
      ;;
    passed)
      local gate retention_path
      for gate in PROTOCOL_COMPATIBILITY MIGRATION_COMPATIBILITY LIVE_AUTHORIZATION APPROVAL_CONTINUATION; do
        [[ "$(assignment_value "$gates" "$gate")" == "passed" ]] || die "required compatibility gate is not passed: $gate"
      done
      retention_path="$(assignment_value "$gates" ZDR_RETENTION_PATH)"
      case "$retention_path" in
        zdr-provider-route-attested|authoritative-non-zdr-policy-attested) ;;
        *) die "ZDR retention path is not independently attested or authoritative" ;;
      esac
      validate_evidence_sha256 ZDR_EVIDENCE_SHA256 \
        "$(assignment_value "$gates" ZDR_EVIDENCE_SHA256)"
      validate_evidence_sha256 AUTH_IDENTITY_EVIDENCE_SHA256 \
        "$(assignment_value "$gates" AUTH_IDENTITY_EVIDENCE_SHA256)"
      validate_evidence_sha256 ROLLBACK_ARTIFACT_MANIFEST_SHA256 \
        "$(assignment_value "$gates" ROLLBACK_ARTIFACT_MANIFEST_SHA256)"
      assignments="$(non_comment_assignment_count "$gates")"
      [[ "$assignments" == "11" ]] || die "passed compatibility gates contain unsupported assignments"
      ;;
    *) die "compatibility gate status is invalid" ;;
  esac
  printf '%s\n' "$status"
}

validate_artifact_payload() {
  local artifact_dir="$1" expected_deploy spec name
  for expected_deploy in "${DEPLOY_FILES[@]}"; do
    require_regular_file "$artifact_dir/deploy/$expected_deploy"
  done
  require_regular_file "$artifact_dir/scripts/release-artifact.sh"
  require_regular_file "$artifact_dir/scripts/compose.sh"
  validate_config_policy "$artifact_dir/config-policy.tsv"
  validate_runtime_public_policy "$artifact_dir/runtime-public-policy.env" \
    "$artifact_dir/compatibility-gates.env" "$artifact_dir/config-policy.tsv"

  expected_deploy=""
  for name in "${DEPLOY_FILES[@]}"; do
    expected_deploy+="deploy/$name"$'\n'
  done
  validate_two_column_file_manifest "$artifact_dir" "$artifact_dir/compose-inputs.tsv" $'path\tsha256' "deploy/" "$expected_deploy"
  [[ "$(($(wc -l <"$artifact_dir/compose-inputs.tsv") - 1))" == "${#DEPLOY_FILES[@]}" ]] ||
    die "Compose input manifest contains missing or additional files"
  local compose_path compose_hash compose_extra approved
  while IFS=$'\t' read -r compose_path compose_hash compose_extra || [[ -n "${compose_path:-}" ]]; do
    [[ "$compose_path" == "path" ]] && continue
    approved=0
    for name in "${DEPLOY_FILES[@]}"; do
      [[ "$compose_path" == "deploy/$name" ]] && approved=1
    done
    [[ "$approved" == "1" ]] || die "Compose input manifest includes an unapproved file"
  done <"$artifact_dir/compose-inputs.tsv"

  validate_two_column_file_manifest "$artifact_dir" "$artifact_dir/migration-manifest.tsv" $'path\tsha256' "migrations/" ""
  for spec in "${MIGRATION_SOURCES[@]}"; do
    name="${spec%%|*}"
    [[ "$(awk -F '\t' -v prefix="migrations/$name/" 'index($1, prefix) == 1 { count++ } END { print count + 0 }' "$artifact_dir/migration-manifest.tsv")" -gt 0 ]] ||
      die "migration snapshot is missing: $name"
  done
  validate_cross_plane_dependencies "$artifact_dir"
}

verify_signature() {
  local artifact_dir="$1" manifest="$artifact_dir/artifact-manifest.tsv"
  local signature="$artifact_dir/artifact-manifest.sig" embedded_key="$artifact_dir/signing-public-key.pem"
  local verify_key="${2:-${MODEL_PLANE_ARTIFACT_VERIFY_KEY:-}}"
  local require_trusted_signer="${3:-0}" has_signature=0 verification_key
  [[ "$require_trusted_signer" == "0" || "$require_trusted_signer" == "1" ]] ||
    die "signature verification mode is invalid"
  [[ -e "$signature" ]] && has_signature=1
  if [[ "$has_signature" == "1" ]]; then
    require_regular_file "$signature"
    require_regular_file "$embedded_key"
    require_command openssl
    if [[ -n "$verify_key" ]]; then
      secure_input_snapshot "$verify_key" public 65536 verification_key
      openssl dgst -sha256 -verify "$verification_key" -signature "$signature" "$manifest" >/dev/null 2>&1 ||
        die "artifact signature does not match the configured verification key"
    else
      secure_input_snapshot "$embedded_key" public 65536 verification_key
      openssl dgst -sha256 -verify "$verification_key" -signature "$signature" "$manifest" >/dev/null 2>&1 ||
        die "artifact self-contained signature is invalid"
    fi
  elif [[ -e "$embedded_key" ]]; then
    die "artifact has a public signing key without a signature"
  elif [[ -n "$verify_key" ]]; then
    die "configured artifact verification key requires a signed artifact"
  fi

  if [[ "$require_trusted_signer" == "1" ]]; then
    [[ "$has_signature" == "1" ]] || die "release mode requires a signed artifact"
    [[ -n "$verify_key" ]] ||
      die "release mode requires MODEL_PLANE_ARTIFACT_VERIFY_KEY as a trusted signer anchor"
  fi
}

verify_artifact_integrity() {
  local artifact_dir verify_key="${2:-}" require_trusted_signer="${3:-0}"
  artifact_dir="$(absolute_path "$1")"
  [[ -d "$artifact_dir" ]] || die "artifact directory does not exist: $artifact_dir"
  [[ ! -L "$artifact_dir" ]] || die "artifact directory must not be a symbolic link"
  [[ -z "$(find "$artifact_dir" -type l -print -quit)" ]] ||
    die "artifact must not contain symbolic links"
  require_regular_file "$artifact_dir/.complete"
  require_regular_file "$artifact_dir/images.lock.env"
  validate_lock "$artifact_dir/images.lock.env"
  validate_image_manifest "$artifact_dir"
  validate_root_manifest "$artifact_dir"
  validate_metadata "$artifact_dir"
  validate_artifact_payload "$artifact_dir"
  compatibility_gate_status "$artifact_dir/compatibility-gates.env" "$(lock_value "$artifact_dir/images.lock.env" SOURCE_REVISION)" >/dev/null
  verify_signature "$artifact_dir" "$verify_key" "$require_trusted_signer"
}

rollback_artifact_directory() {
  local configured="${MODEL_PLANE_ROLLBACK_ARTIFACT_DIR:-}"
  [[ -n "$configured" ]] ||
    die "release mode requires MODEL_PLANE_ROLLBACK_ARTIFACT_DIR as an external rollback artifact locator"
  [[ "$configured" == /* ]] ||
    die "MODEL_PLANE_ROLLBACK_ARTIFACT_DIR must be an absolute external artifact directory"
  [[ -d "$configured" && ! -L "$configured" ]] ||
    die "rollback artifact directory does not exist: $configured"
  canonical_directory "$configured"
}

rollback_verification_key() {
  local verify_key="${MODEL_PLANE_ROLLBACK_ARTIFACT_VERIFY_KEY:-${MODEL_PLANE_ARTIFACT_VERIFY_KEY:-}}"
  [[ -n "$verify_key" ]] ||
    die "release mode requires MODEL_PLANE_ROLLBACK_ARTIFACT_VERIFY_KEY or MODEL_PLANE_ARTIFACT_VERIFY_KEY"
  require_regular_file "$verify_key"
  printf '%s\n' "$verify_key"
}

rollback_runtime_environment_file() {
  local configured="${MODEL_PLANE_ROLLBACK_RUNTIME_ENV_FILE:-}"
  [[ -n "$configured" ]] ||
    die "release mode requires MODEL_PLANE_ROLLBACK_RUNTIME_ENV_FILE for deployability validation"
  [[ "$configured" == /* ]] ||
    die "MODEL_PLANE_ROLLBACK_RUNTIME_ENV_FILE must be an absolute external runtime file"
  printf '%s\n' "$configured"
}

validate_rollback_artifact_binding() {
  local candidate_dir="$1" preflight_gates="$2" preflight_revision="$3"
  local expected_manifest_sha rollback_dir rollback_verify_key rollback_runtime_file rollback_runtime_snapshot rollback_evidence
  local rollback_view rollback_manifest candidate_view="" candidate_manifest="" final_manifest ignored
  local actual_manifest_sha candidate_manifest_sha rollback_revision candidate_revision
  local rollback_image_payload candidate_image_payload candidate_verify_key="" gates revision gates_snapshot

  # A stored candidate is mutable until its signed members have been copied to
  # the private render view. Derive every binding field from that one trusted
  # view; never reopen the candidate gate or image lock by pathname. Preflight
  # has no candidate yet, so snapshot its external gate before the first read.
  if [[ -n "$candidate_dir" ]]; then
    candidate_verify_key="${MODEL_PLANE_ARTIFACT_VERIFY_KEY:-}"
    [[ -n "$candidate_verify_key" ]] ||
      die "candidate binding requires a trusted artifact verification key"
    create_artifact_render_snapshot "$candidate_dir" "$candidate_verify_key" \
      candidate-render-view candidate_view candidate_manifest
    candidate_manifest_sha="$(sha256_file "$candidate_manifest")"
    gates="$candidate_view/compatibility-gates.env"
    revision="$(lock_value "$candidate_view/images.lock.env" SOURCE_REVISION)"
  else
    [[ -n "$preflight_gates" ]] || die "rollback preflight requires compatibility gates"
    secure_input_snapshot "$(absolute_path "$preflight_gates")" public 65536 gates_snapshot
    gates="$gates_snapshot"
    revision="$preflight_revision"
    validate_revision "$revision"
  fi
  [[ "$(compatibility_gate_status "$gates" "$revision")" == "passed" ]] ||
    die "release mode requires a fully attested compatibility gate file"
  expected_manifest_sha="$(assignment_value "$gates" ROLLBACK_ARTIFACT_MANIFEST_SHA256)"
  validate_sha256 "$expected_manifest_sha"

  rollback_dir="$(rollback_artifact_directory)"
  if [[ -n "$candidate_dir" ]]; then
    [[ "$rollback_dir" != "$(canonical_directory "$candidate_dir")" ]] ||
      die "candidate artifact cannot be its own rollback artifact"
  fi

  rollback_verify_key="$(rollback_verification_key)"
  # Verify the complete offline payload first, then copy every input consumed
  # by render validation through O_NOFOLLOW into a private directory. Each copy
  # is re-hashed against a freshly signature-verified root manifest, eliminating
  # path reopens between trust verification and Compose consumption.
  verify_artifact_integrity "$rollback_dir" "$rollback_verify_key" 1
  [[ -x "$rollback_dir/scripts/release-artifact.sh" && -x "$rollback_dir/scripts/compose.sh" ]] ||
    die "deployability validation requires executable artifact-contained runners"
  create_artifact_render_snapshot "$rollback_dir" "$rollback_verify_key" \
    rollback-render-view rollback_view rollback_manifest
  actual_manifest_sha="$(sha256_file "$rollback_manifest")"
  [[ "$actual_manifest_sha" == "$expected_manifest_sha" ]] ||
    die "rollback artifact root manifest SHA-256 does not match ROLLBACK_ARTIFACT_MANIFEST_SHA256"

  if [[ -n "$candidate_view" ]]; then
    [[ "$candidate_manifest_sha" != "$actual_manifest_sha" ]] ||
      die "candidate artifact cannot use a byte-identical artifact as rollback"
  fi

  rollback_revision="$(lock_value "$rollback_view/images.lock.env" SOURCE_REVISION)"
  [[ "$rollback_revision" != "$revision" ]] ||
    die "rollback artifact source revision must differ from the candidate source revision"

  if [[ -n "$candidate_view" ]]; then
    candidate_revision="$(lock_value "$candidate_view/images.lock.env" SOURCE_REVISION)"
    [[ "$candidate_revision" == "$revision" ]] ||
      die "candidate artifact source revision differs from its compatibility gate"
    rollback_image_payload="$(image_lock_payload_sha256 "$rollback_view/images.lock.env")"
    candidate_image_payload="$(image_lock_payload_sha256 "$candidate_view/images.lock.env")"
    [[ "$rollback_image_payload" != "$candidate_image_payload" ]] ||
      die "rollback artifact image-lock payload must differ from the candidate image-lock payload"
  fi

  rollback_runtime_file="$(rollback_runtime_environment_file)"
  secure_input_snapshot "$rollback_runtime_file" runtime 1048576 rollback_runtime_snapshot
  if [[ -n "$candidate_view" ]]; then
    rollback_evidence="$candidate_view/rollback-runtime-evidence.env"
    require_regular_file "$rollback_evidence"
  else
    rollback_evidence="${MODEL_PLANE_ROLLBACK_RUNTIME_EVIDENCE_FILE:-}"
    [[ -n "$rollback_evidence" ]] ||
      die "release mode requires MODEL_PLANE_ROLLBACK_RUNTIME_EVIDENCE_FILE"
    [[ "$rollback_evidence" == /* ]] ||
      die "MODEL_PLANE_ROLLBACK_RUNTIME_EVIDENCE_FILE must be an absolute external evidence file"
  fi
  validate_rollback_runtime_evidence "$rollback_evidence" "$actual_manifest_sha" \
    "$rollback_view/config-policy.tsv" "$rollback_runtime_snapshot"

  # This is deliberately a renderability proof, not a live readiness claim.
  # The current signed candidate verifier runs a fixed adapter over immutable
  # rollback snapshots, so older v3 predecessors need no new command and no
  # recursive predecessor chain is traversed.
  render_artifact_snapshot "$rollback_view" "$rollback_runtime_snapshot"

  # Detect any mutation of the stored artifact while snapshots were being
  # rendered. Production storage must additionally enforce read-only/immutable
  # retention; this final pass makes local concurrent replacement fail closed.
  verify_artifact_integrity "$rollback_dir" "$rollback_verify_key" 1
  snapshot_signed_root_manifest "$rollback_dir" "$rollback_verify_key" final_manifest
  [[ "$(sha256_file "$final_manifest")" == "$actual_manifest_sha" ]] ||
    die "rollback artifact changed during deployability validation"
  [[ -x "$rollback_dir/scripts/release-artifact.sh" && -x "$rollback_dir/scripts/compose.sh" ]] ||
    die "rollback artifact runner permissions changed during deployability validation"
  if [[ -n "$candidate_view" ]]; then
    verify_artifact_integrity "$candidate_dir" "$candidate_verify_key" 1
    snapshot_signed_root_manifest "$candidate_dir" "$candidate_verify_key" final_manifest
    [[ "$(sha256_file "$final_manifest")" == "$candidate_manifest_sha" ]] ||
      die "candidate artifact changed during rollback binding validation"
  fi
}

verify_artifact() {
  local artifact_dir require_trusted_signer=0
  artifact_dir="$(absolute_path "$1")"
  if release_mode_enabled; then
    require_trusted_signer=1
  fi
  verify_artifact_integrity "$artifact_dir" "${MODEL_PLANE_ARTIFACT_VERIFY_KEY:-}" "$require_trusted_signer"
  if release_mode_enabled; then
    validate_rollback_artifact_binding "$artifact_dir" "" ""
  fi
}

repository_root() {
  git -C "$ROOT_DIR" rev-parse --show-toplevel
}

git_file_to() {
  local repo_root="$1" revision="$2" repo_path="$3" destination="$4"
  mkdir -p "$(dirname "$destination")"
  git -C "$repo_root" show "$revision:$repo_path" >"$destination" ||
    die "release source is missing required file: $repo_path"
}

default_build_date() {
  local repo_root="$1" revision="$2" epoch
  epoch="$(git -C "$repo_root" show -s --format=%ct "$revision")"
  if date -u -d "@$epoch" +%Y-%m-%dT%H:%M:%SZ >/dev/null 2>&1; then
    date -u -d "@$epoch" +%Y-%m-%dT%H:%M:%SZ
  else
    date -u -r "$epoch" +%Y-%m-%dT%H:%M:%SZ
  fi
}

print_build_command() {
  local service="$1" context="$2" dockerfile="$3" revision="$4" build_date="$5" source_date_epoch="$6" output_dir="$7"
  local tag="model-plane-release/${service}:${revision}"
  printf 'docker build --pull=false --build-arg SOURCE_REVISION=%q --build-arg BUILD_DATE=%q --build-arg SOURCE_DATE_EPOCH=%q --label org.opencontainers.image.revision=%q --label org.opencontainers.image.created=%q --tag %q --file %q %q\n' \
    "$revision" "$build_date" "$source_date_epoch" "$revision" "$build_date" "$tag" "$dockerfile" "$context"
  printf 'docker image save --output %q %q\n' "$output_dir/${service}.docker.tar" "$tag"
}

print_infrastructure_image_commands() {
  local image="$1" reference="$2" output_dir="$3"
  printf 'docker image pull %q\n' "$reference"
  printf 'docker image save --output %q %q\n' "$output_dir/${image}.docker.tar" "$reference"
}

extract_env_keys_from_git_file() {
  local repo_root="$1" revision="$2" repo_path="$3"
  git -C "$repo_root" show "$revision:$repo_path" |
    sed -nE 's/^([A-Za-z_][A-Za-z0-9_]*)=.*/\1/p'
}

write_config_policy() {
  local repo_root="$1" revision="$2" destination="$3" temp_keys key
  temp_keys="$(mktemp "${TMPDIR:-/tmp}/model-plane-config-keys.XXXXXX")"
  trap 'rm -f "$temp_keys"' RETURN
  local deploy_file
  for deploy_file in "${DEPLOY_FILES[@]}"; do
    git -C "$repo_root" show "$revision:$MODEL_PLANE_REPO_PATH/deploy/$deploy_file" 2>/dev/null |
      grep -Eo '\$\{[A-Za-z_][A-Za-z0-9_]*' | sed 's/^\${//' >>"$temp_keys" || true
  done
  LC_ALL=C sort -u "$temp_keys" -o "$temp_keys"
  {
    printf 'key\tpartition\n'
    while IFS= read -r key || [[ -n "$key" ]]; do
      [[ -n "$key" ]] || continue
      printf '%s\t%s\n' "$key" "$(config_partition_for_key "$key")"
    done <"$temp_keys"
  } >"$destination"
  validate_config_policy "$destination"
  trap - RETURN
  rm -f "$temp_keys"
}

write_compose_input_manifest() {
  local artifact_dir="$1" destination="$artifact_dir/compose-inputs.tsv" deploy_file
  printf 'path\tsha256\n' >"$destination"
  for deploy_file in "${DEPLOY_FILES[@]}"; do
    printf 'deploy/%s\t%s\n' "$deploy_file" "$(sha256_file "$artifact_dir/deploy/$deploy_file")" >>"$destination"
  done
}

write_migration_snapshot() {
  local repo_root="$1" revision="$2" artifact_dir="$3" manifest="$artifact_dir/migration-manifest.tsv"
  local spec name source_dir repo_file relative destination
  printf 'path\tsha256\n' >"$manifest"
  for spec in "${MIGRATION_SOURCES[@]}"; do
    name="${spec%%|*}"
    source_dir="${spec#*|}"
    while IFS= read -r repo_file || [[ -n "$repo_file" ]]; do
      [[ -n "$repo_file" ]] || continue
      relative="${repo_file#$MODEL_PLANE_REPO_PATH/$source_dir/}"
      destination="$artifact_dir/migrations/$name/$relative"
      git_file_to "$repo_root" "$revision" "$repo_file" "$destination"
      printf 'migrations/%s/%s\t%s\n' "$name" "$relative" "$(sha256_file "$destination")" >>"$manifest"
    done < <(git -C "$repo_root" ls-tree -r --name-only "$revision" -- "$MODEL_PLANE_REPO_PATH/$source_dir")
  done
}

write_cross_plane_dependencies() {
  local repo_root="$1" revision="$2" destination="$3"
  local spec name directory config_file tree keyset
  printf 'dependency\trepo_revision\tgit_tree\tenv_keyset_sha256\n' >"$destination"
  for spec in "${CROSS_PLANE_DEPENDENCIES[@]}"; do
    name="${spec%%|*}"
    directory="${spec#*|}"
    directory="${directory%%|*}"
    config_file="${spec##*|}"
    tree="$(git -C "$repo_root" rev-parse "$revision:$directory")"
    keyset="$(extract_env_keys_from_git_file "$repo_root" "$revision" "$config_file" | LC_ALL=C sort -u | sha256_stdin)"
    printf '%s\t%s\t%s\t%s\n' "$name" "$revision" "$tree" "$keyset" >>"$destination"
  done
}

write_compatibility_gates() {
  local revision="$1" destination="$2" source_file="${MODEL_PLANE_COMPATIBILITY_GATES_FILE:-}" source_snapshot
  if [[ -n "$source_file" ]]; then
    secure_input_snapshot "$source_file" public 65536 source_snapshot
    cp "$source_snapshot" "$destination"
  else
    printf 'COMPATIBILITY_GATES_VERSION=1\nSOURCE_REVISION=%s\nSTATUS=not-attested\n' "$revision" >"$destination"
  fi
  compatibility_gate_status "$destination" "$revision" >/dev/null
  if release_mode_enabled; then
    [[ -n "$source_file" ]] || die "release mode requires MODEL_PLANE_COMPATIBILITY_GATES_FILE"
    [[ "$(compatibility_gate_status "$destination" "$revision")" == "passed" ]] ||
      die "release mode requires fully passed compatibility gates"
  fi
}

write_runtime_public_policy() {
  local destination="$1" gates_file="$2" config_policy="$3"
  local source_file="${MODEL_PLANE_RUNTIME_PUBLIC_POLICY_FILE:-}" source_snapshot
  if [[ -n "$source_file" ]]; then
    secure_input_snapshot "$source_file" public 65536 source_snapshot
    cp "$source_snapshot" "$destination"
  else
    printf 'AUTH_CORE_ISSUER=https://invalid.example/api/convex-auth\nAUTH_CORE_JWKS_URL=https://invalid.example/api/convex-auth/jwks\nAZURE_OPENAI_ZDR_CONFIRMED=false\n' >"$destination"
  fi
  validate_runtime_public_policy "$destination" "$gates_file" "$config_policy"
  if release_mode_enabled; then
    [[ -n "$source_file" ]] || die "release mode requires MODEL_PLANE_RUNTIME_PUBLIC_POLICY_FILE"
  fi
}

write_rollback_runtime_evidence() {
  local destination="$1" source_file="${MODEL_PLANE_ROLLBACK_RUNTIME_EVIDENCE_FILE:-}" source_snapshot
  if [[ -n "$source_file" ]]; then
    secure_input_snapshot "$source_file" public 65536 source_snapshot
    cp "$source_snapshot" "$destination"
  else
    printf 'ROLLBACK_RUNTIME_EVIDENCE_VERSION=1\nSTATUS=not-attested\n' >"$destination"
  fi
  if release_mode_enabled; then
    [[ -n "$source_file" ]] ||
      die "release mode requires MODEL_PLANE_ROLLBACK_RUNTIME_EVIDENCE_FILE"
  fi
}

preflight_release_inputs() {
  local revision="$1" repo_root="$2" gates_source="${MODEL_PLANE_COMPATIBILITY_GATES_FILE:-}"
  local signing_source="${MODEL_PLANE_ARTIFACT_SIGNING_KEY:-}"
  local verify_source="${MODEL_PLANE_ARTIFACT_VERIFY_KEY:-}"
  local public_policy_source="${MODEL_PLANE_RUNTIME_PUBLIC_POLICY_FILE:-}"
  local signing_public_source="${MODEL_PLANE_ARTIFACT_SIGNING_PUBLIC_KEY:-}"
  local rollback_verify_source="${MODEL_PLANE_ROLLBACK_ARTIFACT_VERIFY_KEY:-}"
  local rollback_runtime_source="${MODEL_PLANE_ROLLBACK_RUNTIME_ENV_FILE:-}"
  local rollback_evidence_source="${MODEL_PLANE_ROLLBACK_RUNTIME_EVIDENCE_FILE:-}"
  local gates_file signing_key verify_key public_policy signing_public_key rollback_verify_key
  local rollback_runtime rollback_evidence config_policy
  release_mode_enabled || return
  [[ -n "$gates_source" ]] ||
    die "release mode requires MODEL_PLANE_COMPATIBILITY_GATES_FILE before any Docker build"
  secure_input_snapshot "$gates_source" public 65536 gates_file
  [[ "$(compatibility_gate_status "$gates_file" "$revision")" == "passed" ]] ||
    die "release mode requires fully passed compatibility gates before any Docker build"
  [[ -n "$signing_source" ]] ||
    die "release mode requires MODEL_PLANE_ARTIFACT_SIGNING_KEY before any Docker build"
  secure_input_snapshot "$signing_source" private 65536 signing_key
  [[ -n "$verify_source" ]] ||
    die "release mode requires MODEL_PLANE_ARTIFACT_VERIFY_KEY before any Docker build"
  secure_input_snapshot "$verify_source" public 65536 verify_key
  [[ -n "$public_policy_source" ]] ||
    die "release mode requires MODEL_PLANE_RUNTIME_PUBLIC_POLICY_FILE before any Docker build"
  secure_input_snapshot "$public_policy_source" public 65536 public_policy
  if [[ -n "$signing_public_source" ]]; then
    secure_input_snapshot "$signing_public_source" public 65536 signing_public_key
    export MODEL_PLANE_ARTIFACT_SIGNING_PUBLIC_KEY="$signing_public_key"
  fi
  if [[ -n "$rollback_verify_source" ]]; then
    secure_input_snapshot "$rollback_verify_source" public 65536 rollback_verify_key
    export MODEL_PLANE_ROLLBACK_ARTIFACT_VERIFY_KEY="$rollback_verify_key"
  fi
  [[ -n "$rollback_runtime_source" ]] ||
    die "release mode requires MODEL_PLANE_ROLLBACK_RUNTIME_ENV_FILE before any Docker build"
  secure_input_snapshot "$rollback_runtime_source" runtime 1048576 rollback_runtime
  export MODEL_PLANE_ROLLBACK_RUNTIME_ENV_FILE="$rollback_runtime"
  [[ -n "$rollback_evidence_source" ]] ||
    die "release mode requires MODEL_PLANE_ROLLBACK_RUNTIME_EVIDENCE_FILE before any Docker build"
  secure_input_snapshot "$rollback_evidence_source" public 65536 rollback_evidence
  export MODEL_PLANE_ROLLBACK_RUNTIME_EVIDENCE_FILE="$rollback_evidence"

  export MODEL_PLANE_COMPATIBILITY_GATES_FILE="$gates_file"
  export MODEL_PLANE_ARTIFACT_SIGNING_KEY="$signing_key"
  export MODEL_PLANE_ARTIFACT_VERIFY_KEY="$verify_key"
  export MODEL_PLANE_RUNTIME_PUBLIC_POLICY_FILE="$public_policy"
  config_policy="$SECURE_INPUT_TEMP_DIR/config-policy-preflight.tsv"
  write_config_policy "$repo_root" "$revision" "$config_policy"
  validate_runtime_public_policy "$public_policy" "$gates_file" "$config_policy"
  require_command openssl
  # The compatibility file contains only a content digest. Bind it to an
  # externally located, trusted-signed rollback artifact before Docker can
  # build, pull, inspect, or save a candidate image.
  validate_rollback_artifact_binding "" "$gates_file" "$revision"
}

write_metadata() {
  local artifact_dir="$1" revision="$2" build_date="$3" source_date_epoch="$4" model_tree="$5" destination="$artifact_dir/artifact-metadata.env"
  printf 'ARTIFACT_FORMAT_VERSION=%s\nSOURCE_REVISION=%s\nBUILD_DATE=%s\nSOURCE_DATE_EPOCH=%s\nMODEL_PLANE_GIT_TREE=%s\n' \
    "$ARTIFACT_FORMAT_VERSION" "$revision" "$build_date" "$source_date_epoch" "$model_tree" >"$destination"
  printf 'CONFIG_POLICY_SHA256=%s\n' "$(sha256_file "$artifact_dir/config-policy.tsv")" >>"$destination"
  printf 'RUNTIME_PUBLIC_POLICY_SHA256=%s\n' "$(sha256_file "$artifact_dir/runtime-public-policy.env")" >>"$destination"
  printf 'COMPOSE_INPUTS_SHA256=%s\n' "$(sha256_file "$artifact_dir/compose-inputs.tsv")" >>"$destination"
  printf 'MIGRATION_MANIFEST_SHA256=%s\n' "$(sha256_file "$artifact_dir/migration-manifest.tsv")" >>"$destination"
  printf 'CROSS_PLANE_DEPENDENCIES_SHA256=%s\n' "$(sha256_file "$artifact_dir/cross-plane-dependencies.tsv")" >>"$destination"
  printf 'COMPATIBILITY_GATES_SHA256=%s\n' "$(sha256_file "$artifact_dir/compatibility-gates.env")" >>"$destination"
}

sign_root_manifest_if_configured() {
  local artifact_dir="$1" signing_key_source="${MODEL_PLANE_ARTIFACT_SIGNING_KEY:-}"
  local public_key_source="${MODEL_PLANE_ARTIFACT_SIGNING_PUBLIC_KEY:-}"
  local signing_key public_key release_verify_key
  if [[ -z "$signing_key_source" ]]; then
    release_mode_enabled && die "release mode requires MODEL_PLANE_ARTIFACT_SIGNING_KEY"
    return
  fi
  secure_input_snapshot "$signing_key_source" private 65536 signing_key
  require_command openssl
  if [[ -n "$public_key_source" ]]; then
    secure_input_snapshot "$public_key_source" public 65536 public_key
    cp "$public_key" "$artifact_dir/signing-public-key.pem"
  else
    openssl pkey -in "$signing_key" -pubout -out "$artifact_dir/signing-public-key.pem" >/dev/null 2>&1 ||
      die "unable to derive a public key from the configured signing key"
  fi
  generate_root_manifest "$artifact_dir"
  openssl dgst -sha256 -sign "$signing_key" -out "$artifact_dir/artifact-manifest.sig" "$artifact_dir/artifact-manifest.tsv" >/dev/null 2>&1 ||
    die "unable to sign root artifact manifest"
  openssl dgst -sha256 -verify "$artifact_dir/signing-public-key.pem" \
    -signature "$artifact_dir/artifact-manifest.sig" "$artifact_dir/artifact-manifest.tsv" >/dev/null 2>&1 ||
    die "configured signing key does not match the embedded public key"
  if release_mode_enabled; then
    [[ -n "${MODEL_PLANE_ARTIFACT_VERIFY_KEY:-}" ]] ||
      die "release mode requires MODEL_PLANE_ARTIFACT_VERIFY_KEY"
    secure_input_snapshot "$MODEL_PLANE_ARTIFACT_VERIFY_KEY" public 65536 release_verify_key
    openssl dgst -sha256 -verify "$release_verify_key" \
      -signature "$artifact_dir/artifact-manifest.sig" "$artifact_dir/artifact-manifest.tsv" >/dev/null 2>&1 ||
      die "configured release signing and verification keys do not match"
  fi
}

build_artifact() {
  local mode="$1" repo_root revision build_date source_date_epoch output_dir
  [[ "${#INFRASTRUCTURE_IMAGES[@]}" == "${#INFRASTRUCTURE_REFS[@]}" ]] ||
    die "infrastructure image names and references are inconsistent"
  if [[ "$mode" == "dry-run" ]]; then
    revision="${SOURCE_REVISION:-$(git -C "$ROOT_DIR" rev-parse HEAD)}"
    build_date="${BUILD_DATE:-1970-01-01T00:00:00Z}"
    source_date_epoch="${SOURCE_DATE_EPOCH:-0}"
    validate_revision "$revision"
    output_dir="$(absolute_path "${2:-.release-artifacts/$revision}")"
    local i
    for i in "${!SERVICES[@]}"; do
      print_build_command "${SERVICES[$i]}" "${CONTEXTS[$i]}" "${DOCKERFILES[$i]}" "$revision" "$build_date" "$source_date_epoch" "$output_dir"
    done
    for i in "${!INFRASTRUCTURE_IMAGES[@]}"; do
      print_infrastructure_image_commands "${INFRASTRUCTURE_IMAGES[$i]}" "${INFRASTRUCTURE_REFS[$i]}" "$output_dir"
    done
    printf 'snapshot immutable Compose, config policy, migrations, scripts, and cross-plane revision records into %q\n' "$output_dir"
    return
  fi

  repo_root="$(repository_root)"
  revision="${SOURCE_REVISION:-$(git -C "$repo_root" rev-parse HEAD)}"
  validate_revision "$revision"
  build_date="${BUILD_DATE:-$(default_build_date "$repo_root" "$revision")}"
  [[ "$build_date" =~ ^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}Z$ ]] ||
    die "BUILD_DATE must use UTC RFC 3339 second precision"
  source_date_epoch="${SOURCE_DATE_EPOCH:-$(git -C "$repo_root" show -s --format=%ct "$revision")}"
  [[ "$source_date_epoch" =~ ^[0-9]+$ ]] || die "SOURCE_DATE_EPOCH must be an integer epoch"
  output_dir="$(absolute_path "${2:-.release-artifacts/$revision}")"

  # Release evidence and signer trust are checked before a Docker command can
  # build, pull or otherwise mutate local image state.
  preflight_release_inputs "$revision" "$repo_root"

  local head_revision
  head_revision="$(git -C "$repo_root" rev-parse HEAD)"
  [[ "$revision" == "$head_revision" ]] || die "SOURCE_REVISION must equal the checked-out commit for a build"
  if [[ -n "$(git -C "$repo_root" status --porcelain --untracked-files=all -- "$MODEL_PLANE_REPO_PATH")" ]]; then
    die "Model Plane source is dirty; commit or isolate the exact candidate before building"
  fi
  git -C "$repo_root" cat-file -e "$revision:$MODEL_PLANE_REPO_PATH" 2>/dev/null ||
    die "SOURCE_REVISION does not contain the Model Plane source tree"
  [[ ! -e "$output_dir" ]] || die "refusing to overwrite artifact: $output_dir"

  local parent_dir="${output_dir%/*}" temp_dir="${output_dir}.tmp.$$"
  mkdir -p "$parent_dir"
  [[ ! -e "$temp_dir" ]] || die "temporary artifact path exists: $temp_dir"
  mkdir "$temp_dir"
  trap 'rm -rf "$temp_dir"; cleanup_secure_input_snapshots' EXIT

  # Snapshot the externally supplied gate before invoking Docker. The rollback
  # locator itself remains process-only; the copied gate retains only its
  # approved content digest and is rebound below before the first image build.
  write_compatibility_gates "$revision" "$temp_dir/compatibility-gates.env"
  write_config_policy "$repo_root" "$revision" "$temp_dir/config-policy.tsv"
  write_runtime_public_policy "$temp_dir/runtime-public-policy.env" \
    "$temp_dir/compatibility-gates.env" "$temp_dir/config-policy.tsv"
  write_rollback_runtime_evidence "$temp_dir/rollback-runtime-evidence.env"
  if release_mode_enabled; then
    validate_rollback_artifact_binding "" "$temp_dir/compatibility-gates.env" "$revision"
  fi
  require_command docker

  local i service context dockerfile tag image_id archive archive_sha key deploy_file
  printf '# generated; contains image IDs only, never credentials\n' >"$temp_dir/images.lock.env"
  printf 'SOURCE_REVISION=%s\nBUILD_DATE=%s\n' "$revision" "$build_date" >>"$temp_dir/images.lock.env"
  printf 'service\tsource_revision\timage_id\tarchive\tarchive_sha256\n' >"$temp_dir/manifest.tsv"

  for i in "${!SERVICES[@]}"; do
    service="${SERVICES[$i]}"
    context="${CONTEXTS[$i]}"
    dockerfile="${DOCKERFILES[$i]}"
    [[ -r "$dockerfile" ]] || die "Dockerfile is missing: $dockerfile"
    tag="model-plane-release/${service}:${revision}"

    docker build --pull=false \
      --build-arg "SOURCE_REVISION=$revision" \
      --build-arg "BUILD_DATE=$build_date" \
      --build-arg "SOURCE_DATE_EPOCH=$source_date_epoch" \
      --label "org.opencontainers.image.revision=$revision" \
      --label "org.opencontainers.image.created=$build_date" \
      --tag "$tag" \
      --file "$dockerfile" \
      "$context"

    image_id="$(docker image inspect --format '{{.Id}}' "$tag")"
    [[ "$image_id" =~ ^sha256:[0-9a-f]{64}$ ]] || die "Docker returned an invalid image ID for $service"
    archive="$temp_dir/${service}.docker.tar"
    docker image save --output "$archive" "$tag"
    archive_sha="$(sha256_file "$archive")"
    key="$(lock_key "$service")"
    printf '%s=%s\n' "$key" "$image_id" >>"$temp_dir/images.lock.env"
    printf '%s\t%s\t%s\t%s\t%s\n' "$service" "$revision" "$image_id" "${service}.docker.tar" "$archive_sha" >>"$temp_dir/manifest.tsv"
  done

  # Infrastructure images are part of the signed artifact too. Resolve their
  # configured references once, record the local content ID, and archive every
  # layer so candidate and rollback restores never depend on a registry.
  local reference
  for i in "${!INFRASTRUCTURE_IMAGES[@]}"; do
    service="${INFRASTRUCTURE_IMAGES[$i]}"
    reference="${INFRASTRUCTURE_REFS[$i]}"
    docker image pull "$reference"
    image_id="$(docker image inspect --format '{{.Id}}' "$reference")"
    [[ "$image_id" =~ ^sha256:[0-9a-f]{64}$ ]] || die "Docker returned an invalid image ID for $service"
    archive="$temp_dir/${service}.docker.tar"
    docker image save --output "$archive" "$image_id"
    archive_sha="$(sha256_file "$archive")"
    key="$(lock_key "$service")"
    printf '%s=%s\n' "$key" "$image_id" >>"$temp_dir/images.lock.env"
    printf '%s\t%s\t%s\t%s\t%s\n' "$service" "$revision" "$image_id" "${service}.docker.tar" "$archive_sha" >>"$temp_dir/manifest.tsv"
  done

  for deploy_file in "${DEPLOY_FILES[@]}"; do
    git_file_to "$repo_root" "$revision" "$MODEL_PLANE_REPO_PATH/deploy/$deploy_file" "$temp_dir/deploy/$deploy_file"
  done
  git_file_to "$repo_root" "$revision" "$MODEL_PLANE_REPO_PATH/scripts/release-artifact.sh" "$temp_dir/scripts/release-artifact.sh"
  git_file_to "$repo_root" "$revision" "$MODEL_PLANE_REPO_PATH/scripts/compose.sh" "$temp_dir/scripts/compose.sh"
  chmod +x "$temp_dir/scripts/release-artifact.sh" "$temp_dir/scripts/compose.sh"
  write_compose_input_manifest "$temp_dir"
  write_migration_snapshot "$repo_root" "$revision" "$temp_dir"
  write_cross_plane_dependencies "$repo_root" "$revision" "$temp_dir/cross-plane-dependencies.tsv"
  write_metadata "$temp_dir" "$revision" "$build_date" "$source_date_epoch" \
    "$(git -C "$repo_root" rev-parse "$revision:$MODEL_PLANE_REPO_PATH")"
  generate_root_manifest "$temp_dir"
  sign_root_manifest_if_configured "$temp_dir"
  touch "$temp_dir/.complete"

  verify_artifact "$temp_dir"
  mv "$temp_dir" "$output_dir"
  trap cleanup_secure_input_snapshots EXIT
  chmod -R a-w "$output_dir"
  verify_artifact "$output_dir"
  echo "immutable release artifact created: $output_dir"
}

restore_artifact() {
  local artifact_dir
  artifact_dir="$(absolute_path "$1")"
  verify_artifact "$artifact_dir"
  require_command docker

  local service key expected loaded
  for service in "${ARTIFACT_IMAGES[@]}"; do
    docker image load --input "$artifact_dir/${service}.docker.tar" >/dev/null
    key="$(lock_key "$service")"
    expected="$(lock_value "$artifact_dir/images.lock.env" "$key")"
    loaded="$(docker image inspect --format '{{.Id}}' "$expected")"
    [[ "$loaded" == "$expected" ]] || die "restored image ID mismatch for $service"
  done
  echo "immutable release artifact restored and verified: $artifact_dir"
}

render_artifact_snapshot() {
  local artifact_dir="$1" runtime_source="$2" runtime_input runtime_snapshot value variable
  local -a compose_files env_files docker_env
  artifact_dir="$(canonical_directory "$artifact_dir")"
  require_command bash
  bash -n "$artifact_dir/scripts/release-artifact.sh" "$artifact_dir/scripts/compose.sh" ||
    die "artifact-contained deployment runner has invalid shell syntax"

  # Snapshot the external runtime once and keep only the validated secret
  # snapshot alive while Compose consumes it. Public authority/ZDR values are
  # layered from the signed artifact policy ahead of those external secrets.
  secure_input_snapshot "$runtime_source" runtime 1048576 runtime_input
  runtime_snapshot="$(canonical_directory "$SECURE_INPUT_TEMP_DIR")/deployability-runtime-${ARTIFACT_RENDER_SEQUENCE}-${SECURE_INPUT_SEQUENCE}.env"
  snapshot_runtime_config "$runtime_input" \
    "$artifact_dir/config-policy.tsv" \
    "$artifact_dir/runtime-public-policy.env" \
    "$artifact_dir/compatibility-gates.env" \
    "$runtime_snapshot"

  compose_files=(
    -f "$artifact_dir/deploy/docker-compose.yml"
    -f "$artifact_dir/deploy/docker-compose.production.yml"
    -f "$artifact_dir/deploy/docker-compose.release.yml"
  )
  env_files=(
    --env-file "$artifact_dir/runtime-public-policy.env"
    --env-file "$runtime_snapshot"
    --env-file "$artifact_dir/images.lock.env"
  )
  require_command docker
  docker_env=(env -i "PATH=$PATH")
  [[ -n "${HOME:-}" ]] && docker_env+=("HOME=$HOME")
  for variable in DOCKER_HOST DOCKER_CONTEXT DOCKER_CONFIG DOCKER_TLS DOCKER_TLS_VERIFY DOCKER_CERT_PATH DOCKER_API_VERSION DOCKER_DEFAULT_PLATFORM; do
    value="${!variable:-}"
    [[ -n "$value" ]] && docker_env+=("$variable=$value")
  done
  "${docker_env[@]}" docker compose "${env_files[@]}" "${compose_files[@]}" config --quiet
}

validate_artifact_deployability() {
  local artifact_dir contained_root runtime_source render_view render_manifest final_manifest
  release_mode_enabled || die "deployability validation requires release mode"
  artifact_dir="$(canonical_directory "$(absolute_path "$1")")"
  contained_root="$(canonical_directory "$ROOT_DIR")"
  [[ "$artifact_dir" == "$contained_root" ]] ||
    die "deployability validation must run from the artifact-contained verifier"
  [[ -n "${MODEL_PLANE_ARTIFACT_VERIFY_KEY:-}" ]] ||
    die "deployability validation requires a trusted artifact verification key"
  [[ -n "${MODEL_PLANE_RUNTIME_ENV_FILE:-}" ]] ||
    die "deployability validation requires MODEL_PLANE_RUNTIME_ENV_FILE"
  runtime_source="$MODEL_PLANE_RUNTIME_ENV_FILE"
  [[ "$runtime_source" == /* ]] ||
    die "MODEL_PLANE_RUNTIME_ENV_FILE must be an absolute external runtime file"

  verify_artifact_integrity "$artifact_dir" "$MODEL_PLANE_ARTIFACT_VERIFY_KEY" 1
  [[ -x "$artifact_dir/scripts/release-artifact.sh" && -x "$artifact_dir/scripts/compose.sh" ]] ||
    die "deployability validation requires executable artifact-contained runners"
  create_artifact_render_snapshot "$artifact_dir" "$MODEL_PLANE_ARTIFACT_VERIFY_KEY" \
    standalone-render-view render_view render_manifest
  render_artifact_snapshot "$render_view" "$runtime_source"
  verify_artifact_integrity "$artifact_dir" "$MODEL_PLANE_ARTIFACT_VERIFY_KEY" 1
  snapshot_signed_root_manifest "$artifact_dir" "$MODEL_PLANE_ARTIFACT_VERIFY_KEY" final_manifest
  [[ "$(sha256_file "$final_manifest")" == "$(sha256_file "$render_manifest")" ]] ||
    die "artifact changed during deployability validation"
}

compose_artifact() {
  local artifact_dir operation="$2"
  local -a compose_args
  artifact_dir="$(absolute_path "$1")"
  case "$operation" in
    config)
      # Validate the signed merge without rendering resolved runtime secrets.
      compose_args=(config --quiet)
      ;;
    deploy)
      # Fixed immutable deployment: no caller-selected overlays, services,
      # commands, pulls, or builds.
      compose_args=(up -d --wait --no-build)
      ;;
    *)
      die "unsupported artifact Compose operation"
      ;;
  esac
  [[ -n "${MODEL_PLANE_RUNTIME_ENV_FILE:-}" ]] ||
    die "artifact Compose requires MODEL_PLANE_RUNTIME_ENV_FILE; runtime secrets are not bundled"
  MODEL_PLANE_RELEASE_MODE=1 verify_artifact "$artifact_dir"
  require_regular_file "$artifact_dir/scripts/compose.sh"
  env \
    MODEL_PLANE_RELEASE_MODE=1 \
    MODEL_PLANE_PRODUCTION=1 \
    MODEL_PLANE_RUNTIME_ENV_FILE="$MODEL_PLANE_RUNTIME_ENV_FILE" \
    "$artifact_dir/scripts/compose.sh" "${compose_args[@]}"
}

command="${1:-}"
case "$command" in
  dry-run | build)
    build_artifact "$command" "${2:-}"
    ;;
  verify)
    [[ $# == 2 ]] || { usage >&2; exit 2; }
    verify_artifact "$2"
    echo "release artifact verified"
    ;;
  restore)
    [[ $# == 2 ]] || { usage >&2; exit 2; }
    restore_artifact "$2"
    ;;
  compose)
    [[ $# == 3 && "$3" == "config" ]] || { usage >&2; exit 2; }
    compose_artifact "$2" config
    ;;
  deploy)
    [[ $# == 2 ]] || { usage >&2; exit 2; }
    compose_artifact "$2" deploy
    ;;
  validate-deployability)
    [[ $# == 2 ]] || { usage >&2; exit 2; }
    validate_artifact_deployability "$2"
    ;;
  validate-lock)
    [[ $# == 2 ]] || { usage >&2; exit 2; }
    validate_lock "$2"
    ;;
  validate-runtime-config)
    [[ $# == 5 ]] || { usage >&2; exit 2; }
    validate_runtime_config "$2" "$3" "$4" "$5"
    ;;
  snapshot-runtime-config)
    [[ $# == 6 ]] || { usage >&2; exit 2; }
    snapshot_runtime_config "$2" "$3" "$4" "$5" "$6"
    ;;
  *)
    usage >&2
    exit 2
    ;;
esac
