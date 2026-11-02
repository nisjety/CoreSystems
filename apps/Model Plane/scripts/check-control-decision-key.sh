#!/usr/bin/env bash
set -euo pipefail

# Preflight for the Control Space decision verification key (finding F11).
#
# Control signs Space decisions with a key it generates once and records in its
# ledger (apps/Control Plane/.env.generated-secrets). Model Plane services verify
# those decisions with CONTROL_SPACE_DECISION_KEY_ID and
# CONTROL_SPACE_DECISION_PUBLIC_KEY_BASE64. If the Model Plane env chain resolves
# to a different id or key than the ledger -- typically a stale copy left in
# deploy/.env, which wins over the ledger -- every Control-signed decision is
# refused with a bare "untrusted key" error. This script catches that before
# the stack is (re)started.
#
# It resolves the effective values exactly as scripts/compose.sh does and
# compares them with the ledger:
#
#   1. <Control Plane>/.env.generated-secrets   (when readable)
#   2. deploy/.env                              (later files win)
#   3. the calling process environment          (Compose lets it override both)
#
# Output is key ids, key lengths and short SHA-256 fingerprints only; no key
# value is ever printed. The fingerprint is the first 8 bytes of sha256(raw
# public key) as hex, the same tag capability-core, orchestrator-core and
# session-core log at start-up.
#
# Exit status: 0 the effective key matches the ledger; 1 mismatch (or an
# unusable key); 2 the check could not run (ledger missing, usage error).
#
# Usage:
#   scripts/check-control-decision-key.sh
#   scripts/check-control-decision-key.sh --ledger FILE --model-env FILE [--model-env FILE ...]
#
# --ledger overrides the Control ledger path. --model-env replaces the default
# Model Plane chain; repeat it in compose.sh order (later files win), e.g. for an
# artifact deployment pass its runtime env file.
#
# Opt-in gate for compose.sh (not wired automatically):
#   scripts/check-control-decision-key.sh && scripts/compose.sh up -d

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

KEY_ID_VAR="CONTROL_SPACE_DECISION_KEY_ID"
PUBLIC_KEY_VAR="CONTROL_SPACE_DECISION_PUBLIC_KEY_BASE64"

die() {
  echo "check-control-decision-key: $*" >&2
  exit 2
}

ledger_file="$ROOT_DIR/../Control Plane/.env.generated-secrets"
model_files=()
custom_model_files=0
while [[ $# -gt 0 ]]; do
  case "$1" in
    --ledger)
      [[ $# -ge 2 ]] || die "--ledger needs a file"
      ledger_file="$2"
      shift 2
      ;;
    --model-env)
      [[ $# -ge 2 ]] || die "--model-env needs a file"
      model_files+=("$2")
      custom_model_files=1
      shift 2
      ;;
    -h | --help)
      sed -n '3,39p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
      exit 0
      ;;
    *) die "unknown argument: $1" ;;
  esac
done

if [[ "$custom_model_files" == "0" ]]; then
  # Same order and conditions as compose.sh: the Control ledger first, so a
  # plane-local deploy/.env can still override it deliberately.
  if [[ -r "$ROOT_DIR/../Control Plane/.env.generated-secrets" ]]; then
    model_files+=("$ROOT_DIR/../Control Plane/.env.generated-secrets")
  fi
  model_files+=("$ROOT_DIR/deploy/.env")
fi

[[ -r "$ledger_file" && -f "$ledger_file" ]] ||
  die "Control ledger is not readable: $ledger_file"

# env_file_value FILE NAME -> the last assignment of NAME in FILE, with the
# quoting and inline-comment rules Compose applies to an env file. Prints
# nothing (status 1) when NAME is not assigned. Never echoes anything else.
env_file_value() {
  local file="$1" name="$2" line value found=1
  [[ -r "$file" && -f "$file" ]] || return 1
  while IFS= read -r line || [[ -n "$line" ]]; do
    line="${line%$'\r'}"
    line="${line#"${line%%[![:space:]]*}"}"
    [[ -z "$line" || "$line" == \#* ]] && continue
    line="${line#export }"
    line="${line#"${line%%[![:space:]]*}"}"
    [[ "$line" == "$name="* ]] || continue
    value="${line#"$name"=}"
    if [[ "$value" == \"* ]]; then
      value="${value#\"}"
      value="${value%%\"*}"
    elif [[ "$value" == \'* ]]; then
      value="${value#\'}"
      value="${value%%\'*}"
    else
      value="${value%%[[:space:]]#*}"
      value="${value%"${value##*[![:space:]]}"}"
    fi
    found=0
    RESULT="$value"
  done <"$file"
  return "$found"
}

# Resolve NAME over the ordered files plus the process environment. Sets
# EFFECTIVE (the value) and EFFECTIVE_SOURCE (a label for the report).
resolve_effective() {
  local name="$1" file
  EFFECTIVE=""
  EFFECTIVE_SOURCE="unset"
  for file in "${model_files[@]}"; do
    RESULT=""
    if env_file_value "$file" "$name"; then
      EFFECTIVE="$RESULT"
      EFFECTIVE_SOURCE="$(basename -- "$(dirname -- "$file")")/$(basename -- "$file")"
    fi
  done
  if [[ -n "${!name:-}" ]]; then
    EFFECTIVE="${!name}"
    EFFECTIVE_SOURCE="process environment"
  fi
}

sha256_hex() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum | cut -d' ' -f1
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 | cut -d' ' -f1
  else
    openssl dgst -sha256 -r | cut -d' ' -f1
  fi
}

# key_fingerprint VALUE -> 16 hex chars, or status 1 when VALUE is not a base64
# or base64url encoding of exactly 32 bytes. URL-safe is read first (that is what
# Control emits); standard base64 and padding are tolerated like the services do.
key_fingerprint() {
  local value="$1" std raw_len
  value="${value//$'\r'/}"
  value="${value#"${value%%[![:space:]]*}"}"
  value="${value%"${value##*[![:space:]]}"}"
  [[ -n "$value" ]] || return 1
  [[ "$value" =~ ^[A-Za-z0-9_+/=-]+$ ]] || return 1
  std="${value//-/+}"
  std="${std//_//}"
  std="${std%%=*}"
  case $((${#std} % 4)) in
    1) return 1 ;;
    2) std="${std}==" ;;
    3) std="${std}=" ;;
  esac
  raw_len="$(printf '%s' "$std" | base64 -d 2>/dev/null | wc -c | tr -d '[:space:]')" || return 1
  [[ "$raw_len" == "32" ]] || return 1
  printf '%s' "$std" | base64 -d 2>/dev/null | sha256_hex | cut -c1-16
}

# describe_side LABEL KEYID PUBLICKEY ID_SOURCE KEY_SOURCE -> prints the side's
# report lines and sets FP to the public key fingerprint (empty when unusable).
describe_side() {
  local label="$1" key_id="$2" public_key="$3" source_id="$4" source_key="$5"
  FP=""
  if [[ -z "$key_id" ]]; then
    printf '  %-6s key id      : (not set)\n' "$label"
  else
    printf '  %-6s key id      : %s   [%s]\n' "$label" "$key_id" "$source_id"
  fi
  if [[ -z "$public_key" ]]; then
    printf '  %-6s public key  : (not set)\n' "$label"
  elif FP="$(key_fingerprint "$public_key")"; then
    printf '  %-6s public key  : fingerprint %s   [%s]\n' "$label" "$FP" "$source_key"
  else
    FP=""
    printf '  %-6s public key  : UNPARSEABLE (%d chars, not base64/base64url of 32 bytes)   [%s]\n' \
      "$label" "${#public_key}" "$source_key"
  fi
}

ledger_id=""
ledger_key=""
RESULT=""
if env_file_value "$ledger_file" "$KEY_ID_VAR"; then ledger_id="$RESULT"; fi
RESULT=""
if env_file_value "$ledger_file" "$PUBLIC_KEY_VAR"; then ledger_key="$RESULT"; fi
[[ -n "$ledger_id" && -n "$ledger_key" ]] ||
  die "the Control ledger does not define both $KEY_ID_VAR and $PUBLIC_KEY_VAR: $ledger_file"

resolve_effective "$KEY_ID_VAR"
model_id="$EFFECTIVE"
model_id_source="$EFFECTIVE_SOURCE"
resolve_effective "$PUBLIC_KEY_VAR"
model_key="$EFFECTIVE"
model_key_source="$EFFECTIVE_SOURCE"

echo "Control Space decision key preflight"
describe_side "Ledger" "$ledger_id" "$ledger_key" "$(basename -- "$ledger_file")" "$(basename -- "$ledger_file")"
ledger_fp="$FP"
describe_side "Model" "$model_id" "$model_key" "$model_id_source" "$model_key_source"
model_fp="$FP"

status=0
if [[ -z "$ledger_fp" ]]; then
  echo "FAIL: the Control ledger public key is not a valid 32-byte key" >&2
  status=1
fi
if [[ -z "$model_id" || -z "$model_key" ]]; then
  echo "FAIL: the Model Plane env chain does not resolve both $KEY_ID_VAR and $PUBLIC_KEY_VAR" >&2
  status=1
elif [[ -z "$model_fp" ]]; then
  echo "FAIL: the Model Plane public key is not a valid 32-byte key; services refuse to start with it" >&2
  status=1
fi
if [[ -n "$model_id" && "$model_id" != "$ledger_id" ]]; then
  echo "FAIL: key id mismatch (Model '$model_id' from $model_id_source, ledger '$ledger_id')" >&2
  status=1
fi
if [[ -n "$model_fp" && -n "$ledger_fp" && "$model_fp" != "$ledger_fp" ]]; then
  echo "FAIL: public key mismatch (Model fingerprint $model_fp from $model_key_source, ledger fingerprint $ledger_fp)" >&2
  status=1
fi

if [[ -n "${CONTROL_SPACE_DECISION_PUBLIC_KEYS_JSON:-}" ]]; then
  echo "NOTE: CONTROL_SPACE_DECISION_PUBLIC_KEYS_JSON is set in the process environment; session-core reads that key set instead, and this script does not compare it." >&2
fi

if [[ "$status" == "0" ]]; then
  echo "OK: the Model Plane env chain resolves the same decision key as the Control ledger"
else
  echo "Remove the stale CONTROL_SPACE_DECISION_* lines from the overriding file (see the sources above) so the Control ledger value is used, then recreate capability-core, orchestrator-core, execution-core and session-core." >&2
fi
exit "$status"
