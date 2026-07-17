#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

revision="0123456789abcdef0123456789abcdef01234567"
config_policy="$TMP_DIR/config-policy.tsv"
gates_non_zdr="$TMP_DIR/gates-non-zdr.env"
gates_zdr="$TMP_DIR/gates-zdr.env"
public_non_zdr="$TMP_DIR/public-non-zdr.env"
runtime_secrets="$TMP_DIR/runtime-secrets.env"

printf 'key\tpartition\n' >"$config_policy"
printf 'AUTH_CORE_ISSUER\tpublic\nAUTH_CORE_JWKS_URL\tpublic\nAUTH_CORE_URL\tpublic\n' >>"$config_policy"
printf 'AZURE_OPENAI_ZDR_CONFIRMED\tpublic\nAZURE_OPENAI_ENDPOINT\tpublic\n' >>"$config_policy"
printf 'AZURE_OPENAI_API_VERSION\tpublic\nAZURE_OPENAI_REGION\tpublic\n' >>"$config_policy"
printf 'AZURE_OPENAI_CHAT_DEPLOYMENTS\tpublic\nAZURE_OPENAI_EMBEDDING_DEPLOYMENTS\tpublic\n' >>"$config_policy"
printf 'AZURE_OPENAI_DEPLOYMENT\tpublic\nAZURE_OPENAI_EMBEDDING_DEPLOYMENT\tpublic\n' >>"$config_policy"
printf 'INFERENCE_PROVIDER_ORDER\tpublic\nMODEL_PLANE_RESIDENCY\tpublic\n' >>"$config_policy"
printf 'AZURE_OPENAI_API_KEY\tsecret\nMODEL_NATS_RUNTIME_PASSWORD\tsecret\n' >>"$config_policy"
printf 'MODEL_POSTGRES_PASSWORD\tsecret\n' >>"$config_policy"
printf 'SOURCE_REVISION\tartifact\nBUILD_DATE\tartifact\nMODEL_GATEWAY_RELEASE_IMAGE\tartifact\n' >>"$config_policy"

write_gates() {
  local destination="$1" retention_path="$2"
  printf 'COMPATIBILITY_GATES_VERSION=1\nSOURCE_REVISION=%s\nSTATUS=passed\n' \
    "$revision" >"$destination"
  printf 'PROTOCOL_COMPATIBILITY=passed\nMIGRATION_COMPATIBILITY=passed\nLIVE_AUTHORIZATION=passed\n' \
    >>"$destination"
  printf 'APPROVAL_CONTINUATION=passed\nZDR_RETENTION_PATH=%s\n' "$retention_path" \
    >>"$destination"
  printf 'ZDR_EVIDENCE_SHA256=%064d\nAUTH_IDENTITY_EVIDENCE_SHA256=%064d\n' 1 2 \
    >>"$destination"
  printf 'ROLLBACK_ARTIFACT_MANIFEST_SHA256=%064d\n' 3 \
    >>"$destination"
}

write_gates "$gates_non_zdr" authoritative-non-zdr-policy-attested
write_gates "$gates_zdr" zdr-provider-route-attested

printf 'AUTH_CORE_ISSUER=https://auth.release.example/api/convex-auth\n' >"$public_non_zdr"
printf 'AUTH_CORE_JWKS_URL=https://jwks.release.example/keys/model-plane\n' >>"$public_non_zdr"
printf 'AUTH_CORE_URL=https://auth.release.example\n' >>"$public_non_zdr"
printf 'AZURE_OPENAI_ZDR_CONFIRMED=false\nINFERENCE_PROVIDER_ORDER=azure,anthropic,openai\n' \
  >>"$public_non_zdr"
printf 'MODEL_POSTGRES_PASSWORD=fixture-only\n' >"$runtime_secrets"
# Compatibility with the currently committed v3 Compose input. The working
# tree uses per-principal NATS credentials, but rollback/source snapshots may
# still contain this audited legacy secret until that migration is released.
printf 'MODEL_NATS_RUNTIME_PASSWORD=legacy-fixture-only\n' >>"$runtime_secrets"
chmod 644 "$config_policy" "$gates_non_zdr" "$gates_zdr" "$public_non_zdr"
chmod 600 "$runtime_secrets"

validate_runtime() {
  "$ROOT_DIR/scripts/release-artifact.sh" validate-runtime-config \
    "$1" "$config_policy" "$2" "$3"
}

expect_rejection() {
  local name="$1" expected="$2" output
  shift 2
  output="$TMP_DIR/$name.out"
  if "$@" >"$output" 2>&1; then
    echo "release runtime partition unexpectedly accepted $name" >&2
    exit 1
  fi
  if ! grep -F "$expected" "$output" >/dev/null; then
    echo "release runtime partition returned the wrong rejection for $name" >&2
    sed -n '1,20p' "$output" >&2
    exit 1
  fi
}

# GREEN target: the external runtime file contains secrets only. Every public
# override is rooted in the signed public policy; provenance/images come only
# from the artifact lock.
validate_runtime "$runtime_secrets" "$public_non_zdr" "$gates_non_zdr"

gates_missing_auth_evidence="$TMP_DIR/gates-missing-auth-evidence.env"
grep -v '^AUTH_IDENTITY_EVIDENCE_SHA256=' "$gates_non_zdr" >"$gates_missing_auth_evidence"
chmod 644 "$gates_missing_auth_evidence"
expect_rejection missing-auth-evidence 'passed release requires Auth issuer/JWKS live-binding evidence' \
  validate_runtime "$runtime_secrets" "$public_non_zdr" "$gates_missing_auth_evidence"

gates_placeholder_auth="$TMP_DIR/gates-placeholder-auth.env"
sed 's/^AUTH_IDENTITY_EVIDENCE_SHA256=.*/AUTH_IDENTITY_EVIDENCE_SHA256=0000000000000000000000000000000000000000000000000000000000000000/' \
  "$gates_non_zdr" >"$gates_placeholder_auth"
chmod 644 "$gates_placeholder_auth"
expect_rejection placeholder-auth-evidence 'AUTH_IDENTITY_EVIDENCE_SHA256 must not be a placeholder digest' \
  validate_runtime "$runtime_secrets" "$public_non_zdr" "$gates_placeholder_auth"

public_in_secret="$TMP_DIR/public-in-secret.env"
cp "$runtime_secrets" "$public_in_secret"
printf 'AUTH_CORE_ISSUER=https://unsigned.example/api/convex-auth\n' >>"$public_in_secret"
chmod 600 "$public_in_secret"
expect_rejection public-in-secret 'external runtime config permits secret keys only' \
  validate_runtime "$public_in_secret" "$public_non_zdr" "$gates_non_zdr"

artifact_in_secret="$TMP_DIR/artifact-in-secret.env"
cp "$runtime_secrets" "$artifact_in_secret"
printf 'SOURCE_REVISION=ffffffffffffffffffffffffffffffffffffffff\n' >>"$artifact_in_secret"
chmod 600 "$artifact_in_secret"
expect_rejection artifact-in-secret 'external runtime config permits secret keys only' \
  validate_runtime "$artifact_in_secret" "$public_non_zdr" "$gates_non_zdr"

unknown_secret="$TMP_DIR/unknown-secret.env"
cp "$runtime_secrets" "$unknown_secret"
printf 'UNREVIEWED_API_CREDENTIAL=attacker\n' >>"$unknown_secret"
chmod 600 "$unknown_secret"
expect_rejection unknown-secret 'runtime config key is not allowed by the artifact policy' \
  validate_runtime "$unknown_secret" "$public_non_zdr" "$gates_non_zdr"

secret_in_public="$TMP_DIR/secret-in-public.env"
cp "$public_non_zdr" "$secret_in_public"
printf 'AZURE_OPENAI_API_KEY=must-not-enter-signed-artifact\n' >>"$secret_in_public"
chmod 644 "$secret_in_public"
expect_rejection secret-in-public 'signed runtime public policy permits public keys only' \
  validate_runtime "$runtime_secrets" "$secret_in_public" "$gates_non_zdr"

artifact_in_public="$TMP_DIR/artifact-in-public.env"
cp "$public_non_zdr" "$artifact_in_public"
printf 'MODEL_GATEWAY_RELEASE_IMAGE=sha256:%064d\n' 9 >>"$artifact_in_public"
chmod 644 "$artifact_in_public"
expect_rejection artifact-in-public 'signed runtime public policy permits public keys only' \
  validate_runtime "$runtime_secrets" "$artifact_in_public" "$gates_non_zdr"

unknown_public="$TMP_DIR/unknown-public.env"
cp "$public_non_zdr" "$unknown_public"
printf 'UNREVIEWED_FEATURE_FLAG=true\n' >>"$unknown_public"
chmod 644 "$unknown_public"
expect_rejection unknown-public 'runtime public policy key is not allowed by the artifact policy' \
  validate_runtime "$runtime_secrets" "$unknown_public" "$gates_non_zdr"

credential_drift_policy="$TMP_DIR/credential-drift-policy.tsv"
cp "$config_policy" "$credential_drift_policy"
printf 'NEW_PROVIDER_CREDENTIAL\tpublic\n' >>"$credential_drift_policy"
chmod 644 "$credential_drift_policy"
expect_rejection credential-drift 'credential-shaped config key is not in the audited secret set' \
  "$ROOT_DIR/scripts/release-artifact.sh" validate-runtime-config \
  "$runtime_secrets" "$credential_drift_policy" "$public_non_zdr" "$gates_non_zdr"

expanded_public="$TMP_DIR/expanded-public.env"
cp "$public_non_zdr" "$expanded_public"
printf 'AUTH_CORE_URL=${AUTH_CORE_ISSUER}\n' >>"$expanded_public"
chmod 644 "$expanded_public"
expect_rejection public-expansion 'restricted dotenv' \
  validate_runtime "$runtime_secrets" "$expanded_public" "$gates_non_zdr"

for attack in expansion quoted export whitespace duplicate; do
  hostile="$TMP_DIR/hostile-$attack.env"
  cp "$runtime_secrets" "$hostile"
  case "$attack" in
    expansion) printf 'AZURE_OPENAI_API_KEY=${MODEL_POSTGRES_PASSWORD}\n' >>"$hostile" ;;
    quoted) printf 'AZURE_OPENAI_API_KEY="quoted"\n' >>"$hostile" ;;
    export) printf 'export AZURE_OPENAI_API_KEY=exported\n' >>"$hostile" ;;
    whitespace) printf 'AZURE_OPENAI_API_KEY=contains whitespace\n' >>"$hostile" ;;
    duplicate) printf 'MODEL_POSTGRES_PASSWORD=second-value\n' >>"$hostile" ;;
  esac
  chmod 600 "$hostile"
  expect_rejection "dotenv-$attack" 'restricted dotenv' \
    validate_runtime "$hostile" "$public_non_zdr" "$gates_non_zdr"
done

# A true ZDR claim must identify the exact route that the independent evidence
# covers. Legacy deployment aliases stay empty to prevent precedence ambiguity.
public_zdr="$TMP_DIR/public-zdr.env"
printf 'AUTH_CORE_ISSUER=https://auth.release.example/api/convex-auth\n' >"$public_zdr"
printf 'AUTH_CORE_JWKS_URL=https://jwks.release.example/keys/model-plane\n' >>"$public_zdr"
printf 'AUTH_CORE_URL=https://auth.release.example\nAZURE_OPENAI_ZDR_CONFIRMED=true\n' >>"$public_zdr"
printf 'AZURE_OPENAI_ENDPOINT=https://release-resource.openai.azure.com\n' >>"$public_zdr"
printf 'AZURE_OPENAI_API_VERSION=2025-01-01-preview\nAZURE_OPENAI_REGION=swedencentral\n' >>"$public_zdr"
printf 'AZURE_OPENAI_CHAT_DEPLOYMENTS=gpt-5-mini,gpt-4o-mini\n' >>"$public_zdr"
printf 'AZURE_OPENAI_EMBEDDING_DEPLOYMENTS=text-embedding-3-small\n' >>"$public_zdr"
printf 'AZURE_OPENAI_DEPLOYMENT=\nAZURE_OPENAI_EMBEDDING_DEPLOYMENT=\n' >>"$public_zdr"
printf 'INFERENCE_PROVIDER_ORDER=azure,anthropic,openai\nMODEL_PLANE_RESIDENCY=swedencentral\n' \
  >>"$public_zdr"
chmod 644 "$public_zdr"

runtime_zdr="$TMP_DIR/runtime-zdr.env"
cp "$runtime_secrets" "$runtime_zdr"
printf 'AZURE_OPENAI_API_KEY=fixture-provider-key\n' >>"$runtime_zdr"
chmod 600 "$runtime_zdr"
validate_runtime "$runtime_zdr" "$public_zdr" "$gates_zdr"

gates_placeholder_zdr="$TMP_DIR/gates-placeholder-zdr.env"
sed 's/^ZDR_EVIDENCE_SHA256=.*/ZDR_EVIDENCE_SHA256=0000000000000000000000000000000000000000000000000000000000000000/' \
  "$gates_zdr" >"$gates_placeholder_zdr"
chmod 644 "$gates_placeholder_zdr"
expect_rejection placeholder-zdr-evidence 'ZDR_EVIDENCE_SHA256 must not be a placeholder digest' \
  validate_runtime "$runtime_zdr" "$public_zdr" "$gates_placeholder_zdr"

gates_placeholder_rollback="$TMP_DIR/gates-placeholder-rollback.env"
sed 's/^ROLLBACK_ARTIFACT_MANIFEST_SHA256=.*/ROLLBACK_ARTIFACT_MANIFEST_SHA256=0000000000000000000000000000000000000000000000000000000000000000/' \
  "$gates_zdr" >"$gates_placeholder_rollback"
chmod 644 "$gates_placeholder_rollback"
expect_rejection placeholder-rollback-evidence 'ROLLBACK_ARTIFACT_MANIFEST_SHA256 must not be a placeholder digest' \
  validate_runtime "$runtime_zdr" "$public_zdr" "$gates_placeholder_rollback"

for required in AZURE_OPENAI_ENDPOINT AZURE_OPENAI_API_VERSION AZURE_OPENAI_REGION \
  AZURE_OPENAI_CHAT_DEPLOYMENTS AZURE_OPENAI_EMBEDDING_DEPLOYMENTS \
  INFERENCE_PROVIDER_ORDER MODEL_PLANE_RESIDENCY; do
  incomplete="$TMP_DIR/zdr-missing-$required.env"
  grep -v "^${required}=" "$public_zdr" >"$incomplete"
  chmod 644 "$incomplete"
  expect_rejection "zdr-missing-$required" "ZDR provider confirmation requires signed $required" \
    validate_runtime "$runtime_zdr" "$incomplete" "$gates_zdr"
done

wrong_residency="$TMP_DIR/zdr-wrong-residency.env"
sed 's/MODEL_PLANE_RESIDENCY=swedencentral/MODEL_PLANE_RESIDENCY=eastus/' \
  "$public_zdr" >"$wrong_residency"
chmod 644 "$wrong_residency"
expect_rejection zdr-wrong-residency 'ZDR provider route region and residency must match' \
  validate_runtime "$runtime_zdr" "$wrong_residency" "$gates_zdr"

wrong_order="$TMP_DIR/zdr-wrong-order.env"
sed 's/INFERENCE_PROVIDER_ORDER=azure,anthropic,openai/INFERENCE_PROVIDER_ORDER=anthropic,azure,openai/' \
  "$public_zdr" >"$wrong_order"
chmod 644 "$wrong_order"
expect_rejection zdr-wrong-order 'ZDR provider order must start with azure' \
  validate_runtime "$runtime_zdr" "$wrong_order" "$gates_zdr"

legacy_route="$TMP_DIR/zdr-legacy-route.env"
sed 's/AZURE_OPENAI_DEPLOYMENT=/AZURE_OPENAI_DEPLOYMENT=unsigned-legacy-route/' \
  "$public_zdr" >"$legacy_route"
chmod 644 "$legacy_route"
expect_rejection zdr-legacy-route 'ZDR provider route forbids legacy deployment aliases' \
  validate_runtime "$runtime_zdr" "$legacy_route" "$gates_zdr"

missing_provider_key="$TMP_DIR/zdr-missing-provider-key.env"
cp "$runtime_secrets" "$missing_provider_key"
chmod 600 "$missing_provider_key"
expect_rejection zdr-missing-provider-key 'ZDR provider route requires AZURE_OPENAI_API_KEY in the secret runtime file' \
  validate_runtime "$missing_provider_key" "$public_zdr" "$gates_zdr"

echo "release runtime partition contracts: ok"
