#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

revision="0123456789abcdef0123456789abcdef01234567"
gates="$TMP_DIR/compatibility-gates.env"
public_policy="$TMP_DIR/runtime-public-policy.env"
config_policy="$TMP_DIR/config-policy.tsv"
runtime_env="$TMP_DIR/runtime.env"
private_key="$TMP_DIR/signing-private.pem"
verify_key="$TMP_DIR/signing-public.pem"

printf 'COMPATIBILITY_GATES_VERSION=1\nSOURCE_REVISION=%s\nSTATUS=passed\n' "$revision" >"$gates"
printf 'PROTOCOL_COMPATIBILITY=passed\nMIGRATION_COMPATIBILITY=passed\nLIVE_AUTHORIZATION=passed\n' >>"$gates"
printf 'APPROVAL_CONTINUATION=passed\nZDR_RETENTION_PATH=authoritative-non-zdr-policy-attested\n' >>"$gates"
printf 'ZDR_EVIDENCE_SHA256=%064d\nAUTH_IDENTITY_EVIDENCE_SHA256=%064d\n' 1 2 >>"$gates"
printf 'ROLLBACK_ARTIFACT_MANIFEST_SHA256=%064d\n' 3 >>"$gates"
printf 'AUTH_CORE_ISSUER=https://auth.release.example/api/convex-auth\n' >"$public_policy"
printf 'AUTH_CORE_JWKS_URL=https://jwks.release.example/keys/model-plane\n' >>"$public_policy"
printf 'AZURE_OPENAI_ZDR_CONFIRMED=false\n' >>"$public_policy"
printf 'key\tpartition\nAUTH_CORE_ISSUER\tpublic\nAUTH_CORE_JWKS_URL\tpublic\n' >"$config_policy"
printf 'AZURE_OPENAI_ZDR_CONFIRMED\tpublic\nMODEL_POSTGRES_PASSWORD\tsecret\n' >>"$config_policy"
printf 'MODEL_POSTGRES_PASSWORD=fixture-only\n' >"$runtime_env"

command -v openssl >/dev/null 2>&1 || {
  echo "openssl is required for release input security coverage" >&2
  exit 1
}
openssl genpkey -algorithm RSA -pkeyopt rsa_keygen_bits:2048 -out "$private_key" >/dev/null 2>&1
openssl pkey -in "$private_key" -pubout -out "$verify_key" >/dev/null 2>&1
chmod 600 "$private_key" "$runtime_env"
chmod 644 "$verify_key" "$gates" "$public_policy" "$config_policy"

expect_rejection() {
  local name="$1" expected="$2" output
  output="$TMP_DIR/${name}.out"
  shift 2
  if "$@" >"$output" 2>&1; then
    echo "release input security test unexpectedly accepted $name" >&2
    exit 1
  fi
  if ! grep -F "$expected" "$output" >/dev/null; then
    echo "release input security test returned the wrong rejection for $name" >&2
    sed -n '1,20p' "$output" >&2
    exit 1
  fi
}

validate_runtime() {
  "$ROOT_DIR/scripts/release-artifact.sh" validate-runtime-config \
    "$1" "$config_policy" "$2" "$3"
}

release_preflight() {
  MODEL_PLANE_RELEASE_MODE=1 \
  MODEL_PLANE_COMPATIBILITY_GATES_FILE="$1" \
  MODEL_PLANE_ARTIFACT_SIGNING_KEY="$2" \
  MODEL_PLANE_ARTIFACT_VERIFY_KEY="$3" \
  MODEL_PLANE_RUNTIME_PUBLIC_POLICY_FILE="$4" \
  MODEL_PLANE_ROLLBACK_ARTIFACT_DIR="$TMP_DIR" \
  MODEL_PLANE_ROLLBACK_ARTIFACT_VERIFY_KEY="$3" \
  SOURCE_REVISION="$revision" BUILD_DATE=2026-07-16T00:00:00Z SOURCE_DATE_EPOCH=0 \
    "$ROOT_DIR/scripts/release-artifact.sh" build "$TMP_DIR/never-built"
}

# Legitimate private and public inputs remain accepted. The direct runtime
# validator does not need Docker, a repository build, or secret material.
validate_runtime "$runtime_env" "$public_policy" "$gates"

snapshot_parent="$TMP_DIR/private-snapshot-parent"
mkdir "$snapshot_parent"
chmod 700 "$snapshot_parent"
snapshot_parent="$(cd "$snapshot_parent" && pwd -P)"
(
  umask 0777
  "$ROOT_DIR/scripts/release-artifact.sh" snapshot-runtime-config \
    "$runtime_env" "$config_policy" "$public_policy" "$gates" \
    "$snapshot_parent/runtime.env"
)
test "$(stat -f '%Lp' "$snapshot_parent/runtime.env" 2>/dev/null || stat -c '%a' "$snapshot_parent/runtime.env")" = "600"
cmp -s "$runtime_env" "$snapshot_parent/runtime.env"

insecure_runtime="$TMP_DIR/insecure-runtime.env"
cp "$runtime_env" "$insecure_runtime"
chmod 640 "$insecure_runtime"
expect_rejection runtime-permissions 'private release input must not grant group or other permissions' \
  validate_runtime "$insecure_runtime" "$public_policy" "$gates"

oversized_runtime="$TMP_DIR/oversized-runtime.env"
cp "$runtime_env" "$oversized_runtime"
awk 'BEGIN { printf "#"; for (i = 0; i < 1048576; i++) printf "x"; printf "\n" }' >>"$oversized_runtime"
chmod 600 "$oversized_runtime"
expect_rejection runtime-size 'runtime release input exceeds 1048576 bytes' \
  validate_runtime "$oversized_runtime" "$public_policy" "$gates"

runtime_symlink="$TMP_DIR/runtime-link.env"
ln -s "$runtime_env" "$runtime_symlink"
expect_rejection runtime-symlink 'release input must be a regular file opened without following symbolic links' \
  validate_runtime "$runtime_symlink" "$public_policy" "$gates"

insecure_gates="$TMP_DIR/insecure-gates.env"
cp "$gates" "$insecure_gates"
chmod 666 "$insecure_gates"
expect_rejection gates-permissions 'public release input must not be group or other writable' \
  release_preflight "$insecure_gates" "$private_key" "$verify_key" "$public_policy"

oversized_gates="$TMP_DIR/oversized-gates.env"
cp "$gates" "$oversized_gates"
awk 'BEGIN { printf "#"; for (i = 0; i < 65536; i++) printf "x"; printf "\n" }' >>"$oversized_gates"
chmod 644 "$oversized_gates"
expect_rejection gates-size 'public release input exceeds 65536 bytes' \
  release_preflight "$oversized_gates" "$private_key" "$verify_key" "$public_policy"

insecure_private_key="$TMP_DIR/insecure-private.pem"
cp "$private_key" "$insecure_private_key"
chmod 644 "$insecure_private_key"
expect_rejection signing-key-permissions 'private release input must not grant group or other permissions' \
  release_preflight "$gates" "$insecure_private_key" "$verify_key" "$public_policy"

oversized_private_key="$TMP_DIR/oversized-private.pem"
cp "$private_key" "$oversized_private_key"
awk 'BEGIN { for (i = 0; i < 65536; i++) printf "x" }' >>"$oversized_private_key"
chmod 600 "$oversized_private_key"
expect_rejection signing-key-size 'private release input exceeds 65536 bytes' \
  release_preflight "$gates" "$oversized_private_key" "$verify_key" "$public_policy"

insecure_verify_key="$TMP_DIR/insecure-public.pem"
cp "$verify_key" "$insecure_verify_key"
chmod 666 "$insecure_verify_key"
expect_rejection verification-key-permissions 'public release input must not be group or other writable' \
  release_preflight "$gates" "$private_key" "$insecure_verify_key" "$public_policy"

oversized_verify_key="$TMP_DIR/oversized-public.pem"
cp "$verify_key" "$oversized_verify_key"
awk 'BEGIN { for (i = 0; i < 65536; i++) printf "x" }' >>"$oversized_verify_key"
chmod 644 "$oversized_verify_key"
expect_rejection verification-key-size 'public release input exceeds 65536 bytes' \
  release_preflight "$gates" "$private_key" "$oversized_verify_key" "$public_policy"

insecure_public_policy="$TMP_DIR/insecure-public-policy.env"
cp "$public_policy" "$insecure_public_policy"
chmod 666 "$insecure_public_policy"
expect_rejection public-policy-permissions 'public release input must not be group or other writable' \
  release_preflight "$gates" "$private_key" "$verify_key" "$insecure_public_policy"

oversized_public_policy="$TMP_DIR/oversized-public-policy.env"
cp "$public_policy" "$oversized_public_policy"
awk 'BEGIN { printf "#"; for (i = 0; i < 65536; i++) printf "x"; printf "\n" }' >>"$oversized_public_policy"
chmod 644 "$oversized_public_policy"
expect_rejection public-policy-size 'public release input exceeds 65536 bytes' \
  release_preflight "$gates" "$private_key" "$verify_key" "$oversized_public_policy"

fifo="$TMP_DIR/runtime.fifo"
mkfifo "$fifo"
expect_rejection runtime-fifo 'release input must be a regular file opened without following symbolic links' \
  validate_runtime "$fifo" "$public_policy" "$gates"

echo "release input security contracts: ok"
