#!/usr/bin/env bash
set -Eeuo pipefail
umask 077

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CORE_ROOT="${CORE_ROOT_OVERRIDE:-$(cd "$SCRIPT_DIR/.." && pwd)}"
DRY_RUN=false
ROTATE_NATS=false
ROTATE_MODEL_EXECUTION_KEY_ACTION=""

while (( $# > 0 )); do
  case "$1" in
    --dry-run) DRY_RUN=true ;;
    --rotate-nats) ROTATE_NATS=true ;;
    --rotate-model-execution-attestation-key=*)
      ROTATE_MODEL_EXECUTION_KEY_ACTION="${1#*=}"
      case "$ROTATE_MODEL_EXECUTION_KEY_ACTION" in
        stage|promote|prune) ;;
        *)
          printf 'Usage: --rotate-model-execution-attestation-key=stage|promote|prune\n' >&2
          exit 2
          ;;
      esac
      ;;
    *)
      printf 'Usage: %s [--dry-run] [--rotate-nats] [--rotate-model-execution-attestation-key=stage|promote|prune]\n' "$0" >&2
      exit 2
      ;;
  esac
  shift
done

for command_name in awk jq openssl mktemp; do
  if ! command -v "$command_name" >/dev/null 2>&1; then
    printf 'Required command is unavailable: %s\n' "$command_name" >&2
    exit 1
  fi
done

ROOT_ENV="$CORE_ROOT/.env"
DATA_ENV="$CORE_ROOT/apps/Data Plane v2/.env"
CONTROL_ENV="$CORE_ROOT/apps/Control Plane/.env"
INGESTION_ENV="$CORE_ROOT/apps/Ingestion Plane/.env"
# These plane-level files are this script's own LEDGER of shared secrets: the
# running stacks read `.env.generated-secrets` and each `<core>/.env`, and this
# script syncs outward into them. Do NOT "fix" the paths below by repointing
# them at `.env.generated-secrets` — measured 2026-09-04 with --dry-run on a
# live machine: targeting the ledger reports 21 pending changes (all outbound
# syncs into Data Plane v2 / Model deploy / verevonv3), whereas repointing to
# `.env.generated-secrets` reports 62, because those files are sparse (7-8 keys
# for Ingestion/Application) so every ledger-owned secret becomes a fresh MINT
# — silently rotating credentials the fleet still authenticates with.
#
# If all three are absent the guard below refuses every run: on this machine
# they had been renamed to `.env.bootstrap-orphan`. Restore those, do not
# repoint.
#
# Ingestion Plane retired a shared plane-level .env for runtime purposes
# (2026-07-17, see run-ingestion-plane.sh's own header comment): each core
# owns its own <core>/.env, injected per-service via compose's `env_file:`.
# INGESTION_ENV above is therefore NOT read by the actual running stack for
# anything integration-corev2-specific — confirmed empirically: it does not
# even exist on a live dev machine, while integration-corev2/.env does, and
# already carries a real (not placeholder) attestation-key entry that nothing
# in this script could have produced. Provider-write attestation provisioning
# targets this real file directly instead.
INTEGRATION_COREV2_ENV="$CORE_ROOT/apps/Ingestion Plane/integration-corev2/.env"
MODEL_ENV="$CORE_ROOT/apps/Model Plane/deploy/.env"
APPLICATION_ENV="$CORE_ROOT/apps/Application Plane/.env"
FRONTEND_ENV="$CORE_ROOT/apps/Frontend Plane/verevonv3/.env"

ENV_FILES=(
  "$ROOT_ENV"
  "$DATA_ENV"
  "$CONTROL_ENV"
  "$INGESTION_ENV"
  "$MODEL_ENV"
  "$APPLICATION_ENV"
  "$FRONTEND_ENV"
)

LOCK_DIR="$CORE_ROOT/.bootstrap-runtime-environment.lock"
LOCK_HELD=false

# Logs go to STDERR, never stdout. `ensure_secret`/`ensure_value` return the
# secret by printing it, and every caller captures that with `$(...)` — so a
# log written to stdout was swallowed into the captured value instead of being
# shown. Under --dry-run that both HID every ensure_* change from the report
# (making the "require ZERO would-configure lines" gate unsound) and prefixed
# the log text onto the value each sync_value then compared.
log() { printf '[runtime-env] %s\n' "$*" >&2; }

release_lock() {
  if [[ "$LOCK_HELD" == "true" ]]; then
    rm -f "$LOCK_DIR/pid"
    rmdir "$LOCK_DIR" 2>/dev/null || true
  fi
}

acquire_lock() {
  local existing_pid=""
  if mkdir "$LOCK_DIR" 2>/dev/null; then
    printf '%s\n' "$$" > "$LOCK_DIR/pid"
    LOCK_HELD=true
  else
    if [[ -f "$LOCK_DIR/pid" ]]; then
      IFS= read -r existing_pid < "$LOCK_DIR/pid" || true
    fi
    if [[ "$existing_pid" =~ ^[0-9]+$ ]] && kill -0 "$existing_pid" 2>/dev/null; then
      printf 'Runtime environment bootstrap is already running (pid %s).\n' "$existing_pid" >&2
      return 1
    fi
    rm -f "$LOCK_DIR/pid"
    rmdir "$LOCK_DIR" 2>/dev/null || true
    mkdir "$LOCK_DIR"
    printf '%s\n' "$$" > "$LOCK_DIR/pid"
    LOCK_HELD=true
  fi
  trap release_lock EXIT
}

strip_quotes() {
  local value="$1"
  if [[ "$value" == \"*\" && ${#value} -ge 2 ]]; then
    value="${value#\"}"
    value="${value%\"}"
  elif [[ "$value" == \'*\' && ${#value} -ge 2 ]]; then
    value="${value#\'}"
    value="${value%\'}"
  fi
  printf '%s' "$value"
}

dotenv_get() {
  local file="$1" key="$2" value
  [[ -f "$file" ]] || return 1
  value="$(awk -v key="$key" 'index($0, key "=") == 1 { print substr($0, length(key) + 2); exit }' "$file")"
  [[ -n "$value" ]] || return 1
  strip_quotes "$value"
}

is_placeholder() {
  local value
  value="$(printf '%s' "$1" | tr '[:upper:]' '[:lower:]')"
  [[ -z "$value" || "$value" == "change-me" || "$value" == "changeme" || \
     "$value" == "password" || "$value" == "secret" || \
     "$value" == replace-with-* || "$value" == your-* || \
     "$value" == *"_placeholder" || "$value" == *-secret || \
     "$value" == "<"*">" || \
     "$value" == dev-super-secret* ]]
}

random_secret() { openssl rand -hex 32; }

random_base64_32() { openssl rand -base64 32 | tr -d '\n'; }

ensure_env_file() {
  local file="$1"
  if [[ "$DRY_RUN" == "true" ]]; then
    return 0
  fi
  mkdir -p "$(dirname "$file")"
  if [[ ! -f "$file" ]]; then
    printf '# Generated local runtime environment. Keep untracked.\n' > "$file"
  fi
  chmod 600 "$file"
}

upsert_env() {
  local file="$1" key="$2" value="$3" current tmp
  current="$(dotenv_get "$file" "$key" 2>/dev/null || true)"
  if [[ "$current" == "$value" ]]; then
    return 0
  fi
  if [[ "$DRY_RUN" == "true" ]]; then
    log "would configure $(basename "$(dirname "$file")")/$key"
    return 0
  fi

  tmp="$(mktemp "${file}.tmp.XXXXXX")"
  awk -v key="$key" -v value="$value" '
    BEGIN { replaced=0 }
    index($0, key "=") == 1 {
      if (!replaced) print key "=" value
      replaced=1
      next
    }
    { print }
    END { if (!replaced) print key "=" value }
  ' "$file" > "$tmp"
  chmod 600 "$tmp"
  mv "$tmp" "$file"
}

ensure_secret() {
  local file="$1" key="$2" value
  value="$(dotenv_get "$file" "$key" 2>/dev/null || true)"
  if is_placeholder "$value"; then
    value="$(random_secret)"
    upsert_env "$file" "$key" "$value"
  fi
  printf '%s' "$value"
}

ensure_value() {
  local file="$1" key="$2" default_value="$3" value
  value="$(dotenv_get "$file" "$key" 2>/dev/null || true)"
  if is_placeholder "$value"; then
    value="$default_value"
    upsert_env "$file" "$key" "$value"
  fi
  printf '%s' "$value"
}

ensure_base64_32_secret() {
  local file="$1" key="$2" value decoded_size
  value="$(dotenv_get "$file" "$key" 2>/dev/null || true)"
  decoded_size="$(printf '%s' "$value" | openssl base64 -d -A 2>/dev/null | wc -c | tr -d ' ' || true)"
  if is_placeholder "$value" || [[ "$decoded_size" != "32" ]]; then
    value="$(random_base64_32)"
    upsert_env "$file" "$key" "$value"
  fi
  printf '%s' "$value"
}

sync_value() {
  local key="$1" value="$2"
  shift 2
  local file
  for file in "$@"; do
    upsert_env "$file" "$key" "$value"
  done
}

key_pair_matches() {
  local private_key="$1" public_key="$2" private_fingerprint public_fingerprint
  [[ -s "$private_key" && -s "$public_key" ]] || return 1
  private_fingerprint="$(openssl pkey -in "$private_key" -pubout -outform DER 2>/dev/null | openssl dgst -sha256 2>/dev/null)"
  public_fingerprint="$(openssl pkey -pubin -in "$public_key" -outform DER 2>/dev/null | openssl dgst -sha256 2>/dev/null)"
  [[ -n "$private_fingerprint" && "$private_fingerprint" == "$public_fingerprint" ]]
}

ensure_event_keypair() {
  local domain="$1"
  local key_dir="$CORE_ROOT/apps/Data Plane v2/.secrets/event-keys"
  local private_key="$key_dir/$domain-events.pem"
  local public_key="$key_dir/$domain-events.pub"
  # What gets STORED in $DATA_ENV must stay project-relative. These variables
  # are compose BIND-MOUNT SOURCES (docker-compose.yml mounts
  # `${..._KEY_PATH:-.secrets/event-keys/<domain>-events.pem}` at
  # /run/event-keys/...), resolved against the Data Plane project directory,
  # and compose's own defaults are exactly these relative paths. Storing the
  # $CORE_ROOT-absolute form hands Docker an MSYS path ("/c/dev/...") it cannot
  # resolve as a Windows host path, and Docker then auto-vivifies an empty
  # DIRECTORY at the mount source — the "Is a directory" crash-loop documented
  # under `retrieval` below. Filesystem work keeps the absolute paths.
  local private_key_env=".secrets/event-keys/$domain-events.pem"
  local public_key_env=".secrets/event-keys/$domain-events.pub"

  if [[ "$DRY_RUN" == "true" ]]; then
    if [[ ! -s "$private_key" || ! -s "$public_key" ]]; then
      log "would generate $domain event-signing keypair"
    fi
  else
    mkdir -p "$key_dir"
    chmod 700 "$key_dir"
    if ! key_pair_matches "$private_key" "$public_key"; then
      local private_tmp public_tmp
      private_tmp="$(mktemp "$key_dir/$domain-private.pem.tmp.XXXXXX")"
      public_tmp="$(mktemp "$key_dir/$domain-public.pem.tmp.XXXXXX")"
      openssl genpkey -algorithm RSA -pkeyopt rsa_keygen_bits:3072 -out "$private_tmp" >/dev/null 2>&1
      openssl pkey -in "$private_tmp" -pubout -out "$public_tmp" >/dev/null 2>&1
      chmod 600 "$private_tmp"
      chmod 644 "$public_tmp"
      mv "$private_tmp" "$private_key"
      mv "$public_tmp" "$public_key"
    fi
    chmod 600 "$private_key"
    chmod 644 "$public_key"
  fi

  case "$domain" in
    documents)
      upsert_env "$DATA_ENV" DOCUMENTS_EVENT_SIGNING_PRIVATE_KEY_PATH "$private_key_env"
      upsert_env "$DATA_ENV" DOCUMENTS_EVENT_VERIFYING_PUBLIC_KEY_PATH "$public_key_env"
      ;;
    index)
      upsert_env "$DATA_ENV" INDEX_EVENT_SIGNING_PRIVATE_KEY_PATH "$private_key_env"
      upsert_env "$DATA_ENV" INDEX_EVENT_VERIFYING_PUBLIC_KEY_PATH "$public_key_env"
      ;;
    embedding)
      upsert_env "$DATA_ENV" EMBEDDING_EVENT_SIGNING_PRIVATE_KEY_PATH "$private_key_env"
      upsert_env "$DATA_ENV" EMBEDDING_EVENT_VERIFYING_PUBLIC_KEY_PATH "$public_key_env"
      ;;
    wiki)
      upsert_env "$DATA_ENV" WIKI_EVENT_SIGNING_PRIVATE_KEY_PATH "$private_key_env"
      upsert_env "$DATA_ENV" WIKI_EVENT_VERIFYING_PUBLIC_KEY_PATH "$public_key_env"
      ;;
    retrieval)
      # Missing from this function's domain list until 2026-08-20 — retrieval
      # was the one signed-event domain docker-compose.yml references
      # (RETRIEVAL_EVENT_SIGNING_PRIVATE_KEY_PATH / _VERIFYING_PUBLIC_KEY_PATH)
      # that nothing ever generated, so the bind-mount source never existed
      # and Docker auto-vivified an empty directory at both paths instead —
      # crash-looping retrieval-engine itself (missing its own signing key)
      # and data-orchestrator-go (missing retrieval's public verifying key
      # for signed cost events) with an unrelated-looking "Is a directory".
      upsert_env "$DATA_ENV" RETRIEVAL_EVENT_SIGNING_PRIVATE_KEY_PATH "$private_key_env"
      upsert_env "$DATA_ENV" RETRIEVAL_EVENT_VERIFYING_PUBLIC_KEY_PATH "$public_key_env"
      ;;
  esac
}

# ensure_ed25519_attestation_key — provision the conversation provider-write
# Ed25519 signing key as base64(64-byte Go ed25519.PrivateKey) = base64(seed||pub),
# the exact format conversation-core's config parser expects (ed25519.PrivateKey,
# len == PrivateKeySize). The matching public key + key id are recorded for
# integration-corev2's runtime verification (not a startup requirement there).
# openssl-only: the last 32 bytes of the PKCS8 DER are the raw seed; the last 32
# bytes of the SPKI DER are the raw public key.
ensure_ed25519_attestation_key() {
  local current decoded_len
  current="$(dotenv_get "$APPLICATION_ENV" CONVERSATION_PROVIDER_WRITE_ATTESTATION_PRIVATE_KEY 2>/dev/null || true)"
  decoded_len="$(printf '%s' "$current" | openssl base64 -d -A 2>/dev/null | wc -c | tr -d ' ' || true)"
  if [[ "$decoded_len" == "64" ]]; then
    return 0
  fi
  if [[ "$DRY_RUN" == "true" ]]; then
    log "would generate conversation provider-write Ed25519 attestation key"
    return 0
  fi

  local priv_b64 pub_b64 check_len
  if openssl genpkey -algorithm ED25519 -out /dev/null >/dev/null 2>&1; then
    # OpenSSL 3+: last 32 bytes of the PKCS8 DER are the raw seed; last 32 of the
    # SPKI DER are the raw public key. seed||pub == Go's 64-byte ed25519.PrivateKey.
    local pem seed_bin pub_bin
    pem="$(mktemp)"; seed_bin="$(mktemp)"; pub_bin="$(mktemp)"
    openssl genpkey -algorithm ED25519 -out "$pem" >/dev/null 2>&1
    openssl pkey -in "$pem" -outform DER 2>/dev/null | tail -c 32 > "$seed_bin"
    openssl pkey -in "$pem" -pubout -outform DER 2>/dev/null | tail -c 32 > "$pub_bin"
    priv_b64="$(cat "$seed_bin" "$pub_bin" | openssl base64 -A)"
    pub_b64="$(openssl base64 -A -in "$pub_bin")"
    rm -f "$pem" "$seed_bin" "$pub_bin"
  elif command -v go >/dev/null 2>&1; then
    # macOS ships LibreSSL, which lacks ED25519 genpkey. Fall back to Go's
    # crypto/ed25519 — the exact library conversation-core parses the key with.
    local godir gen
    godir="$(mktemp -d)"
    cat > "$godir/main.go" <<'GOEOF'
package main
import ("crypto/ed25519";"crypto/rand";"encoding/base64";"fmt";"os")
func main(){pub,priv,err:=ed25519.GenerateKey(rand.Reader);if err!=nil{fmt.Fprintln(os.Stderr,err);os.Exit(1)};fmt.Println(base64.StdEncoding.EncodeToString(priv));fmt.Println(base64.StdEncoding.EncodeToString(pub))}
GOEOF
    gen="$(cd "$godir" && GO111MODULE=off go run main.go 2>/dev/null || true)"
    rm -rf "$godir"
    priv_b64="$(printf '%s\n' "$gen" | sed -n '1p')"
    pub_b64="$(printf '%s\n' "$gen" | sed -n '2p')"
  else
    printf '[runtime-env] ERROR: need OpenSSL 3 (ED25519) or Go to generate the attestation key\n' >&2
    return 1
  fi

  check_len="$(printf '%s' "$priv_b64" | openssl base64 -d -A 2>/dev/null | wc -c | tr -d ' ' || true)"
  if [[ "$check_len" != "64" ]]; then
    printf '[runtime-env] ERROR: generated Ed25519 attestation key is %s bytes, expected 64\n' "$check_len" >&2
    return 1
  fi
  upsert_env "$APPLICATION_ENV" CONVERSATION_PROVIDER_WRITE_ATTESTATION_PRIVATE_KEY "$priv_b64"
  ensure_env_file "$INTEGRATION_COREV2_ENV"
  upsert_env "$INTEGRATION_COREV2_ENV" CONVERSATION_PROVIDER_WRITE_ATTESTATION_PUBLIC_KEY "$pub_b64"
  upsert_env "$INTEGRATION_COREV2_ENV" CONVERSATION_PROVIDER_WRITE_ATTESTATION_KEY_ID "conversation-provider-write-v1"
}

# ensure_model_execution_attestation_key — provision the model-execution
# (execution-core, Model Plane) provider-write Ed25519 signing key as a plain
# base64(32-byte seed). Unlike conversation-core's key above, execution-core's
# Rust signer (ed25519-dalek) owns both ends of this key end to end, so it
# does not need Go's seed||pub 64-byte concatenation convention — just the
# raw seed. The matching public key + key id are recorded for
# integration-corev2's trusted-key registry (see
# assemble_provider_write_attestation_keys_json below).
# generate_ed25519_seed_keypair OUT_PRIV_VAR OUT_PUB_VAR — shared generation
# logic (openssl 3 ED25519 preferred, Go crypto/ed25519 fallback for LibreSSL
# hosts) producing a raw base64 32-byte seed + base64 32-byte public key.
# Writes results into the two caller-named variables (bash indirect
# assignment) rather than returning printf'd lines, so callers do not need to
# re-split output the way the original inline duplicate of this logic did.
# Factored out because rotate_model_execution_attestation_key below needs the
# exact same generation, just written to different destination variables than
# ensure_model_execution_attestation_key's first-generation path.
generate_ed25519_seed_keypair() {
  # Deliberately NOT named priv_b64/pub_b64: bash locals are dynamically
  # scoped, so printf -v "$__out_priv_var" below would silently target THIS
  # function's own local instead of the caller's if the names collided
  # (every caller of this function names its own output variables
  # priv_b64/pub_b64) — caught by rotate_drive.sh's scratch test, which found
  # ensure_model_execution_attestation_key writing an empty private key.
  local __out_priv_var="$1" __out_pub_var="$2"
  local _gen_priv _gen_pub _gen_check_len
  if openssl genpkey -algorithm ED25519 -out /dev/null >/dev/null 2>&1; then
    local _gen_pem _gen_seed_bin _gen_pub_bin
    _gen_pem="$(mktemp)"; _gen_seed_bin="$(mktemp)"; _gen_pub_bin="$(mktemp)"
    openssl genpkey -algorithm ED25519 -out "$_gen_pem" >/dev/null 2>&1
    openssl pkey -in "$_gen_pem" -outform DER 2>/dev/null | tail -c 32 > "$_gen_seed_bin"
    openssl pkey -in "$_gen_pem" -pubout -outform DER 2>/dev/null | tail -c 32 > "$_gen_pub_bin"
    _gen_priv="$(openssl base64 -A -in "$_gen_seed_bin")"
    _gen_pub="$(openssl base64 -A -in "$_gen_pub_bin")"
    rm -f "$_gen_pem" "$_gen_seed_bin" "$_gen_pub_bin"
  elif command -v go >/dev/null 2>&1; then
    # macOS ships LibreSSL, which lacks ED25519 genpkey. Fall back to Go's
    # crypto/ed25519, keeping only the 32-byte seed half (priv[:32]) since
    # ed25519-dalek's SigningKey is the seed alone, not Go's concatenation.
    local _gen_dir _gen_out
    _gen_dir="$(mktemp -d)"
    cat > "$_gen_dir/main.go" <<'GOEOF'
package main
import ("crypto/ed25519";"crypto/rand";"encoding/base64";"fmt";"os")
func main(){pub,priv,err:=ed25519.GenerateKey(rand.Reader);if err!=nil{fmt.Fprintln(os.Stderr,err);os.Exit(1)};fmt.Println(base64.StdEncoding.EncodeToString(priv[:32]));fmt.Println(base64.StdEncoding.EncodeToString(pub))}
GOEOF
    _gen_out="$(cd "$_gen_dir" && GO111MODULE=off go run main.go 2>/dev/null || true)"
    rm -rf "$_gen_dir"
    _gen_priv="$(printf '%s\n' "$_gen_out" | sed -n '1p')"
    _gen_pub="$(printf '%s\n' "$_gen_out" | sed -n '2p')"
  else
    printf '[runtime-env] ERROR: need OpenSSL 3 (ED25519) or Go to generate the attestation key\n' >&2
    return 1
  fi
  _gen_check_len="$(printf '%s' "$_gen_priv" | openssl base64 -d -A 2>/dev/null | wc -c | tr -d ' ' || true)"
  if [[ "$_gen_check_len" != "32" ]]; then
    printf '[runtime-env] ERROR: generated Ed25519 seed is %s bytes, expected 32\n' "$_gen_check_len" >&2
    return 1
  fi
  printf -v "$__out_priv_var" '%s' "$_gen_priv"
  printf -v "$__out_pub_var" '%s' "$_gen_pub"
}

ensure_model_execution_attestation_key() {
  local current decoded_len current_kid
  current="$(dotenv_get "$MODEL_ENV" EXECUTION_CORE_PROVIDER_WRITE_ATTESTATION_PRIVATE_KEY 2>/dev/null || true)"
  decoded_len="$(printf '%s' "$current" | openssl base64 -d -A 2>/dev/null | wc -c | tr -d ' ' || true)"
  current_kid="$(dotenv_get "$MODEL_ENV" EXECUTION_CORE_PROVIDER_WRITE_ATTESTATION_KEY_ID 2>/dev/null || true)"
  # Both halves are required: attestation.rs::Attestation::from_env() reads
  # the kid from THIS SAME file (MODEL_ENV), not from integration-corev2's —
  # a private key with no matching local kid leaves the signer silently
  # disabled (from_env() returns None). Caught 2026-08-04 provisioning the
  # real dev stack: an earlier version of this function wrote the kid only to
  # INTEGRATION_COREV2_ENV, so the private key existed but the signer never
  # activated.
  if [[ "$decoded_len" == "32" && -n "$current_kid" ]]; then
    return 0
  fi
  if [[ "$DRY_RUN" == "true" ]]; then
    log "would generate model-execution provider-write Ed25519 attestation key"
    return 0
  fi

  local priv_b64 pub_b64
  if [[ "$decoded_len" == "32" ]]; then
    # Private key already valid, only the local kid is missing — reuse the
    # existing key rather than rotating it out from under a signer that may
    # already be running with it.
    priv_b64="$current"
    pub_b64="$(dotenv_get "$INTEGRATION_COREV2_ENV" EXECUTION_CORE_PROVIDER_WRITE_ATTESTATION_PUBLIC_KEY 2>/dev/null || true)"
    if [[ -z "$pub_b64" ]]; then
      printf '[runtime-env] ERROR: %s has a valid private key but %s has no matching public key — cannot safely derive one without regenerating the pair\n' "$MODEL_ENV" "$INTEGRATION_COREV2_ENV" >&2
      return 1
    fi
  else
    generate_ed25519_seed_keypair priv_b64 pub_b64 || return 1
  fi
  upsert_env "$MODEL_ENV" EXECUTION_CORE_PROVIDER_WRITE_ATTESTATION_PRIVATE_KEY "$priv_b64"
  upsert_env "$MODEL_ENV" EXECUTION_CORE_PROVIDER_WRITE_ATTESTATION_KEY_ID "model-execution-provider-write-v1"
  ensure_env_file "$INTEGRATION_COREV2_ENV"
  upsert_env "$INTEGRATION_COREV2_ENV" EXECUTION_CORE_PROVIDER_WRITE_ATTESTATION_PUBLIC_KEY "$pub_b64"
  upsert_env "$INTEGRATION_COREV2_ENV" EXECUTION_CORE_PROVIDER_WRITE_ATTESTATION_KEY_ID "model-execution-provider-write-v1"
}

# --- Prod key rotation (provisioned now, not exercised in dev) -------------
#
# The trusted-key registry (integration-corev2's INTEGRATION_PROVIDER_WRITE_
# ATTESTATION_KEYS_JSON) is a list keyed by kid, and each attestation JWS is
# short-lived (30s TTL, see execution-core's attestation.rs). That makes
# rotation additive and low-risk by construction: register the new key
# alongside the old one, cut the signer over, wait out one TTL window (a
# couple of minutes for safety, not days), then remove the old key. Nothing
# below touches an active credential — it only stages a NEXT key and, later,
# promotes/prunes it on explicit command. See the runbook in
# verevon-roadmap.md for the full operational sequence.
#
# rotate_model_execution_attestation_key — stage a new keypair as NEXT
# (distinct kid, versioned by incrementing the current active kid's trailing
# number) without touching the active signing key. Safe to run in prod: the
# active key keeps signing until an operator explicitly promotes NEXT.
rotate_model_execution_attestation_key() {
  local current_kid next_version next_kid priv_b64 pub_b64
  current_kid="$(dotenv_get "$INTEGRATION_COREV2_ENV" EXECUTION_CORE_PROVIDER_WRITE_ATTESTATION_KEY_ID 2>/dev/null || true)"
  if [[ -z "$current_kid" ]]; then
    printf '[runtime-env] ERROR: no active model-execution attestation key to rotate from (run ensure_model_execution_attestation_key first)\n' >&2
    return 1
  fi
  if [[ "$current_kid" =~ -v([0-9]+)$ ]]; then
    next_version=$(( BASH_REMATCH[1] + 1 ))
  else
    next_version=2
  fi
  next_kid="model-execution-provider-write-v${next_version}"
  if [[ "$DRY_RUN" == "true" ]]; then
    log "would stage model-execution attestation key rotation: $current_kid -> $next_kid"
    return 0
  fi
  generate_ed25519_seed_keypair priv_b64 pub_b64 || return 1
  upsert_env "$MODEL_ENV" EXECUTION_CORE_PROVIDER_WRITE_ATTESTATION_PRIVATE_KEY_NEXT "$priv_b64"
  ensure_env_file "$INTEGRATION_COREV2_ENV"
  upsert_env "$INTEGRATION_COREV2_ENV" EXECUTION_CORE_PROVIDER_WRITE_ATTESTATION_PUBLIC_KEY_NEXT "$pub_b64"
  upsert_env "$INTEGRATION_COREV2_ENV" EXECUTION_CORE_PROVIDER_WRITE_ATTESTATION_KEY_ID_NEXT "$next_kid"
  log "staged model-execution attestation key rotation: $current_kid -> $next_kid (NEXT; not yet active — run assemble_provider_write_attestation_keys_json then promote when ready)"
}

# promote_model_execution_attestation_key — cut the signer over to the staged
# NEXT key. The prior active key moves to PREVIOUS (kept trusted, so any
# token it already signed still verifies for its remaining TTL) rather than
# being deleted outright; prune_model_execution_attestation_key_previous
# removes it once the bake period has passed.
promote_model_execution_attestation_key() {
  local next_priv next_pub next_kid current_pub current_kid
  next_priv="$(dotenv_get "$MODEL_ENV" EXECUTION_CORE_PROVIDER_WRITE_ATTESTATION_PRIVATE_KEY_NEXT 2>/dev/null || true)"
  next_pub="$(dotenv_get "$INTEGRATION_COREV2_ENV" EXECUTION_CORE_PROVIDER_WRITE_ATTESTATION_PUBLIC_KEY_NEXT 2>/dev/null || true)"
  next_kid="$(dotenv_get "$INTEGRATION_COREV2_ENV" EXECUTION_CORE_PROVIDER_WRITE_ATTESTATION_KEY_ID_NEXT 2>/dev/null || true)"
  if [[ -z "$next_priv" || -z "$next_pub" || -z "$next_kid" ]]; then
    printf '[runtime-env] ERROR: no staged NEXT key to promote (run rotate_model_execution_attestation_key first)\n' >&2
    return 1
  fi
  if [[ "$DRY_RUN" == "true" ]]; then
    log "would promote staged model-execution attestation key $next_kid to active"
    return 0
  fi
  current_pub="$(dotenv_get "$INTEGRATION_COREV2_ENV" EXECUTION_CORE_PROVIDER_WRITE_ATTESTATION_PUBLIC_KEY 2>/dev/null || true)"
  current_kid="$(dotenv_get "$INTEGRATION_COREV2_ENV" EXECUTION_CORE_PROVIDER_WRITE_ATTESTATION_KEY_ID 2>/dev/null || true)"
  if [[ -n "$current_pub" && -n "$current_kid" ]]; then
    upsert_env "$INTEGRATION_COREV2_ENV" EXECUTION_CORE_PROVIDER_WRITE_ATTESTATION_PUBLIC_KEY_PREVIOUS "$current_pub"
    upsert_env "$INTEGRATION_COREV2_ENV" EXECUTION_CORE_PROVIDER_WRITE_ATTESTATION_KEY_ID_PREVIOUS "$current_kid"
  fi
  upsert_env "$MODEL_ENV" EXECUTION_CORE_PROVIDER_WRITE_ATTESTATION_PRIVATE_KEY "$next_priv"
  # execution-core's own signer (attestation.rs::from_env) reads its kid from
  # MODEL_ENV, not INTEGRATION_COREV2_ENV — both must move together or the
  # signer keeps embedding the stale kid after a promote.
  upsert_env "$MODEL_ENV" EXECUTION_CORE_PROVIDER_WRITE_ATTESTATION_KEY_ID "$next_kid"
  upsert_env "$INTEGRATION_COREV2_ENV" EXECUTION_CORE_PROVIDER_WRITE_ATTESTATION_PUBLIC_KEY "$next_pub"
  upsert_env "$INTEGRATION_COREV2_ENV" EXECUTION_CORE_PROVIDER_WRITE_ATTESTATION_KEY_ID "$next_kid"
  upsert_env "$MODEL_ENV" EXECUTION_CORE_PROVIDER_WRITE_ATTESTATION_PRIVATE_KEY_NEXT ""
  upsert_env "$INTEGRATION_COREV2_ENV" EXECUTION_CORE_PROVIDER_WRITE_ATTESTATION_PUBLIC_KEY_NEXT ""
  upsert_env "$INTEGRATION_COREV2_ENV" EXECUTION_CORE_PROVIDER_WRITE_ATTESTATION_KEY_ID_NEXT ""
  log "promoted model-execution attestation key $next_kid to active (previous key $current_kid kept trusted — prune after the bake period)"
}

# prune_model_execution_attestation_key_previous — remove the superseded key
# from the trusted registry. Only call this after the bake period (minutes,
# given the 30s JWS TTL — not the days typical for long-lived credentials).
prune_model_execution_attestation_key_previous() {
  if [[ "$DRY_RUN" == "true" ]]; then
    log "would prune the previous (superseded) model-execution attestation key"
    return 0
  fi
  upsert_env "$INTEGRATION_COREV2_ENV" EXECUTION_CORE_PROVIDER_WRITE_ATTESTATION_PUBLIC_KEY_PREVIOUS ""
  upsert_env "$INTEGRATION_COREV2_ENV" EXECUTION_CORE_PROVIDER_WRITE_ATTESTATION_KEY_ID_PREVIOUS ""
  log "pruned the previous model-execution attestation key from the trusted registry"
}

# assemble_provider_write_attestation_keys_json — build the consumable
# INTEGRATION_PROVIDER_WRITE_ATTESTATION_KEYS_JSON array integration-corev2
# actually reads (internal/config/config.go). Previously nothing did this:
# the per-issuer PUBLIC_KEY/_KEY_ID vars above were written but never
# combined, so a fresh bootstrap left this array empty and the whole
# write-attestation mechanism unconfigured (ParseTrustedKeysJSON fails closed
# on an empty value — this would have silently blocked conversation-core's
# existing provider-write sends too, not just execution-core's new one).
# Idempotent: safe to re-run whenever either key is (re)generated; includes
# only the issuers whose public key material actually exists yet.
# assemble_provider_write_attestation_keys_json also includes the
# model-execution NEXT/PREVIOUS slots when present, so a staged rotation
# (rotate_model_execution_attestation_key) is trusted immediately for
# validation, and a just-promoted key's predecessor
# (promote_model_execution_attestation_key) stays trusted through its bake
# period until explicitly pruned. Each slot has its own distinct kid, so
# multiple simultaneous model-execution entries never collide (the verifier
# requires globally unique kids, not unique (issuer, kid) pairs — see
# integration-corev2's ParseTrustedKeysJSON).
assemble_provider_write_attestation_keys_json() {
  local entries=""
  local issuer kid_var pub_var kid pub entry
  for issuer_spec in \
    "conversation-core:CONVERSATION_PROVIDER_WRITE_ATTESTATION_KEY_ID:CONVERSATION_PROVIDER_WRITE_ATTESTATION_PUBLIC_KEY" \
    "model-execution:EXECUTION_CORE_PROVIDER_WRITE_ATTESTATION_KEY_ID:EXECUTION_CORE_PROVIDER_WRITE_ATTESTATION_PUBLIC_KEY" \
    "model-execution:EXECUTION_CORE_PROVIDER_WRITE_ATTESTATION_KEY_ID_NEXT:EXECUTION_CORE_PROVIDER_WRITE_ATTESTATION_PUBLIC_KEY_NEXT" \
    "model-execution:EXECUTION_CORE_PROVIDER_WRITE_ATTESTATION_KEY_ID_PREVIOUS:EXECUTION_CORE_PROVIDER_WRITE_ATTESTATION_PUBLIC_KEY_PREVIOUS"
  do
    IFS=':' read -r issuer kid_var pub_var <<< "$issuer_spec"
    kid="$(dotenv_get "$INTEGRATION_COREV2_ENV" "$kid_var" 2>/dev/null || true)"
    pub="$(dotenv_get "$INTEGRATION_COREV2_ENV" "$pub_var" 2>/dev/null || true)"
    if [[ -n "$kid" && -n "$pub" ]]; then
      entry="{\"issuer\":\"${issuer}\",\"kid\":\"${kid}\",\"public_key\":\"${pub}\"}"
      if [[ -n "$entries" ]]; then
        entries="${entries},${entry}"
      else
        entries="$entry"
      fi
    fi
  done
  if [[ -z "$entries" ]]; then
    log "no provider-write attestation public keys generated yet; leaving INTEGRATION_PROVIDER_WRITE_ATTESTATION_KEYS_JSON untouched"
    return 0
  fi
  if [[ "$DRY_RUN" == "true" ]]; then
    log "would assemble INTEGRATION_PROVIDER_WRITE_ATTESTATION_KEYS_JSON"
    return 0
  fi
  ensure_env_file "$INTEGRATION_COREV2_ENV"
  upsert_env "$INTEGRATION_COREV2_ENV" INTEGRATION_PROVIDER_WRITE_ATTESTATION_KEYS_JSON "[${entries}]"
}

# A rotation action is a standalone operation, not a step in the full
# provisioning sequence below: it touches only the one key it's asked to
# touch and exits immediately, so it is safe to run against a live prod
# environment without re-running (or risking) anything else this script does.
# See the runbook in verevon-roadmap.md for the full operational sequence.
if [[ -n "$ROTATE_MODEL_EXECUTION_KEY_ACTION" ]]; then
  case "$ROTATE_MODEL_EXECUTION_KEY_ACTION" in
    stage) rotate_model_execution_attestation_key && assemble_provider_write_attestation_keys_json ;;
    promote) promote_model_execution_attestation_key && assemble_provider_write_attestation_keys_json ;;
    prune) prune_model_execution_attestation_key_previous && assemble_provider_write_attestation_keys_json ;;
  esac
  exit $?
fi

if [[ "$DRY_RUN" == "false" ]]; then
  acquire_lock
fi

# REFUSE to bootstrap over a fleet whose env files are missing.
#
# `ensure_secret` mints a fresh value whenever it cannot READ an existing one, so
# an absent env file is indistinguishable from "no secret yet" — and this script
# then silently ROTATES credentials the running fleet is still authenticating
# with. Measured on a live deployment: with apps/{Control,Ingestion,Application}
# Plane/.env absent, a dry run reported 24 values it would (re)configure,
# including INTERNAL_API_KEY across six planes and VEREVON_NATS_TOKEN across four.
# Applying that would have broken cross-plane auth fleet-wide with no error, only
# 401s appearing minutes later.
#
# So: creating a MISSING env file is only safe on a genuinely new machine. If any
# plane is already running, the operator must say so explicitly.
missing_env_files=()
for env_file in "${ENV_FILES[@]}"; do
  [[ -f "$env_file" ]] || missing_env_files+=("$env_file")
done
if (( ${#missing_env_files[@]} > 0 )) && [[ "${ALLOW_CREATING_ENV_FILES:-false}" != "true" ]]; then
  log "REFUSING to run: ${#missing_env_files[@]} env file(s) do not exist."
  for env_file in "${missing_env_files[@]}"; do
    log "  missing: ${env_file#"$CORE_ROOT/"}"
  done
  log ""
  log "Creating them mints NEW secrets for every value they should already hold,"
  log "which rotates credentials a running fleet still authenticates with."
  log ""
  log "If this is a fresh machine with nothing deployed, re-run with:"
  log "  ALLOW_CREATING_ENV_FILES=true $0 $*"
  log "Otherwise restore the env files first (each plane's"
  log ".env.generated-secrets is loaded last by compose and wins), then re-run"
  log "with --dry-run and require ZERO 'would configure' lines before applying."
  exit 1
fi

for env_file in "${ENV_FILES[@]}"; do
  ensure_env_file "$env_file"
done

# Fleet-owned shared credentials. Control Plane is authoritative.
internal_api_key="$(ensure_secret "$CONTROL_ENV" INTERNAL_API_KEY)"
sync_value INTERNAL_API_KEY "$internal_api_key" "$ROOT_ENV" "$DATA_ENV" "$INGESTION_ENV" "$MODEL_ENV" "$APPLICATION_ENV" "$FRONTEND_ENV"
sync_value AUTH_CORE_INTERNAL_API_KEY "$internal_api_key" "$INGESTION_ENV"

verevon_nats_token="$(ensure_secret "$CONTROL_ENV" VEREVON_NATS_TOKEN)"
sync_value VEREVON_NATS_TOKEN "$verevon_nats_token" "$DATA_ENV" "$INGESTION_ENV" "$APPLICATION_ENV" "$FRONTEND_ENV"

# Data Plane private infrastructure and signed event domains.
if [[ "$ROTATE_NATS" == "true" ]]; then
  upsert_env "$DATA_ENV" DATAPLANE_NATS_TOKEN "$(random_secret)"
fi
ensure_secret "$DATA_ENV" POSTGRES_PASSWORD >/dev/null
ensure_secret "$DATA_ENV" DATAPLANE_DRAGONFLY_PASSWORD >/dev/null
ensure_secret "$DATA_ENV" DATAPLANE_NATS_TOKEN >/dev/null
ensure_secret "$DATA_ENV" QDRANT_API_KEY >/dev/null
ensure_value "$DATA_ENV" MINIO_ROOT_USER "verevon-data" >/dev/null
ensure_secret "$DATA_ENV" MINIO_ROOT_PASSWORD >/dev/null
for event_domain in documents index embedding wiki retrieval; do
  ensure_event_keypair "$event_domain"
done

# Control Plane service-to-service credentials.
ensure_value "$CONTROL_ENV" DB_USER "controlplane" >/dev/null
ensure_secret "$CONTROL_ENV" DB_PASSWORD >/dev/null
ensure_secret "$CONTROL_ENV" DRAGONFLY_PASSWORD >/dev/null
ensure_secret "$CONTROL_ENV" NATS_TOKEN >/dev/null
ensure_secret "$CONTROL_ENV" JWT_SECRET >/dev/null
ensure_secret "$CONTROL_ENV" INTERNAL_SERVICE_SECRET >/dev/null
ensure_secret "$CONTROL_ENV" GRAFANA_ADMIN_PASSWORD >/dev/null
session_core_token="$(ensure_secret "$CONTROL_ENV" SESSION_CORE_SERVICE_TOKEN)"
user_gateway_token="$(ensure_secret "$CONTROL_ENV" USER_CORE_GATEWAY_TOKEN)"
user_session_token="$(ensure_secret "$CONTROL_ENV" USER_CORE_SESSION_TOKEN)"
user_org_token="$(ensure_secret "$CONTROL_ENV" USER_CORE_ORG_TOKEN)"
user_auth_token="$(ensure_secret "$CONTROL_ENV" USER_CORE_AUTH_TOKEN)"
# integration-corev2's scoped principal on auth-core's /internal/oauth/*
# (Microsoft sign-in hand-off re-mint). Control Plane owns it because auth-core
# validates it (registry entry `integration-core-primary` in the Control Plane
# compose); Ingestion mirrors it under the name integration-api reads.
integration_auth_internal_token="$(ensure_secret "$CONTROL_ENV" INTEGRATION_AUTH_INTERNAL_SERVICE_TOKEN)"
sync_value AUTH_CORE_OAUTH_SERVICE_TOKEN "$integration_auth_internal_token" "$INGESTION_ENV"
user_documents_token="$(ensure_secret "$CONTROL_ENV" USER_CORE_DOCUMENTS_TOKEN)"
user_retrieval_token="$(ensure_secret "$CONTROL_ENV" USER_CORE_RETRIEVAL_TOKEN)"
# Dedicated per-caller token for the auth-core -> user-core membership projection
# endpoint (consumed by both auth-core and user-core in the Control Plane .env).
ensure_secret "$CONTROL_ENV" USER_CORE_MEMBERSHIP_SERVICE_TOKEN >/dev/null
control_policy_key="$(ensure_secret "$CONTROL_ENV" CONTROL_POLICY_SERVICE_API_KEY)"

sync_value USER_CORE_DOCUMENTS_TOKEN "$user_documents_token" "$DATA_ENV"
sync_value USER_CORE_RETRIEVAL_TOKEN "$user_retrieval_token" "$DATA_ENV"
sync_value SESSION_CORE_SERVICE_TOKEN "$session_core_token" "$FRONTEND_ENV"
sync_value USER_CORE_GATEWAY_TOKEN "$user_gateway_token" "$FRONTEND_ENV"
sync_value CONTROL_POLICY_SERVICE_API_KEY "$control_policy_key" "$DATA_ENV"

# Ingestion Plane private credentials and scoped Auth Core callers.
ensure_secret "$INGESTION_ENV" INGESTION_PG_PASSWORD >/dev/null
ensure_secret "$INGESTION_ENV" INGESTION_DRAGONFLY_PASSWORD >/dev/null
ensure_secret "$INGESTION_ENV" INGESTION_NATS_TOKEN >/dev/null
ensure_secret "$INGESTION_ENV" AUTOCOMPLETE_INTERNAL_TOKEN >/dev/null
ensure_secret "$INGESTION_ENV" QDRANT_API_KEY >/dev/null
ensure_secret "$INGESTION_ENV" QUARRY_CONTROL_API_KEY >/dev/null
ensure_secret "$INGESTION_ENV" QUARRY_INTERNAL_SECRET >/dev/null
ensure_secret "$INGESTION_ENV" QUARRY_RUNTIME_AUTH_TOKEN >/dev/null
ensure_base64_32_secret "$INGESTION_ENV" INTEGRATION_CREDENTIALS_ENCRYPTION_KEY >/dev/null
ensure_secret "$INGESTION_ENV" CONNECTOR_RUNTIME_SECRET >/dev/null
ensure_secret "$INGESTION_ENV" SEARXNG_SECRET >/dev/null
sonic_password="$(ensure_secret "$INGESTION_ENV" SONIC_PASSWORD)"
sync_value SONIC_PASSWORD "$sonic_password" "$ROOT_ENV"
imports_service_key="$(ensure_secret "$INGESTION_ENV" IMPORTS_SERVICE_API_KEY)"
shipping_service_key="$(ensure_secret "$INGESTION_ENV" SHIPPING_SERVICE_API_KEY)"
quarry_service_key="$(ensure_secret "$INGESTION_ENV" QUARRY_SERVICE_API_KEY)"
integration_service_key="$(ensure_secret "$INGESTION_ENV" INTEGRATION_SERVICE_API_KEY)"
finspo_service_key="$(ensure_secret "$INGESTION_ENV" FINSPO_SERVICE_API_KEY)"

# Model Plane private infrastructure and its scoped Ingestion caller.
if [[ "$ROTATE_NATS" == "true" ]]; then
  upsert_env "$MODEL_ENV" MODEL_NATS_TOKEN "$(random_secret)"
fi
ensure_secret "$MODEL_ENV" MODEL_POSTGRES_PASSWORD >/dev/null
ensure_secret "$MODEL_ENV" MODEL_TEMPORAL_POSTGRES_PASSWORD >/dev/null
model_nats_token="$(ensure_secret "$MODEL_ENV" MODEL_NATS_TOKEN)"
ensure_secret "$MODEL_ENV" MODEL_DRAGONFLY_PASSWORD >/dev/null
model_minio_user="$(dotenv_get "$MODEL_ENV" MODEL_MINIO_ROOT_USER 2>/dev/null || true)"
if is_placeholder "$model_minio_user"; then
  upsert_env "$MODEL_ENV" MODEL_MINIO_ROOT_USER "verevon-model-$(openssl rand -hex 6)"
fi
ensure_secret "$MODEL_ENV" MODEL_MINIO_ROOT_PASSWORD >/dev/null
ensure_secret "$MODEL_ENV" AGENT_MEMORY_REDIS_PASSWORD >/dev/null
ensure_secret "$MODEL_ENV" AGENT_MEMORY_TOKEN >/dev/null
ensure_secret "$MODEL_ENV" MODEL_BRIDGE_JWT_SECRET >/dev/null
model_ingestion_key="$(ensure_secret "$MODEL_ENV" MODEL_EXECUTION_SERVICE_API_KEY)"
model_gateway_service_key="$(ensure_secret "$MODEL_ENV" MODEL_GATEWAY_SERVICE_API_KEY)"
execution_core_service_key="$(ensure_secret "$MODEL_ENV" EXECUTION_CORE_SERVICE_API_KEY)"
# inference-core reads Verevon's routing policy from session-core in the
# background; this credential lets it mint an aud=session-core service token.
inference_routing_key="$(ensure_secret "$MODEL_ENV" INFERENCE_ROUTING_SERVICE_API_KEY)"
# session-core mints aud=letta-bridge for durable memory and aud=inference-core
# for Dreaming's LLM extraction.
session_core_service_key="$(ensure_secret "$MODEL_ENV" SESSION_CORE_SERVICE_API_KEY)"
# capability-core reads session skills, invokes inference for distillation, and
# starts orchestration workflows.
capability_core_key="$(ensure_secret "$MODEL_ENV" CAPABILITY_CORE_SERVICE_API_KEY)"
# orchestrator-core's Temporal activities call four downstream audiences.
orchestrator_core_key="$(ensure_secret "$MODEL_ENV" ORCHESTRATOR_CORE_SERVICE_API_KEY)"
# Model Plane NATS principal passwords. nats.conf interpolates the SAME variable
# on both the server and the client side (deploy/nats.conf `password: $VAR`), so
# a generated value matches by construction. Idempotent: existing values are
# reused, so a live fleet is never rotated. Nothing else in the checked-in
# provisioning produced these, so a fresh machine's compose refused to start.
ensure_secret "$MODEL_ENV" MODEL_GATEWAY_NATS_PASSWORD >/dev/null
ensure_secret "$MODEL_ENV" MODEL_SESSION_CORE_NATS_PASSWORD >/dev/null
ensure_secret "$MODEL_ENV" MODEL_CAPABILITY_CORE_NATS_PASSWORD >/dev/null
ensure_secret "$MODEL_ENV" MODEL_ORCHESTRATOR_CORE_NATS_PASSWORD >/dev/null
ensure_secret "$MODEL_ENV" MODEL_TOOL_COMPLETION_NATS_PASSWORD >/dev/null
ensure_secret "$MODEL_ENV" MODEL_COST_CORE_NATS_PASSWORD >/dev/null
# The two Application->Model cross-plane NATS identities. Held in the Model env
# because that is where nats.conf authorizes them; synced to Application below.
model_convex_nats="$(ensure_secret "$MODEL_ENV" APPLICATION_CONVEX_MODEL_NATS_PASSWORD)"
model_insight_nats="$(ensure_secret "$MODEL_ENV" APPLICATION_INSIGHT_MODEL_NATS_PASSWORD)"
sync_value APPLICATION_CONVEX_MODEL_NATS_PASSWORD "$model_convex_nats" "$APPLICATION_ENV"
sync_value APPLICATION_INSIGHT_MODEL_NATS_PASSWORD "$model_insight_nats" "$APPLICATION_ENV"
# Opaque high-entropy secret for managed-run start identities (never a user token).
ensure_secret "$MODEL_ENV" MODEL_GATEWAY_MANAGED_START_KEY_SECRET >/dev/null
# Auth Core JWKS endpoint the Model Plane verifies signed audience tokens against.
# A route, not a secret — derived like AUTH_CORE_ISSUER already is.
ensure_value "$MODEL_ENV" AUTH_CORE_JWKS_URL "http://auth-core:3011/api/convex-auth/jwks" >/dev/null
# Data Plane callers of the Model Plane embedding hop. Compose maps three
# DIFFERENT plane-level variables onto the same container variable
# (MODEL_PLANE_INFERENCE_SERVICE_API_KEY) for retrieval-engine, embedding-engine
# and graph-index respectively — retrieval-engine reuses its control-policy key,
# so only these two need their own.
graph_index_key="$(ensure_secret "$DATA_ENV" MODEL_PLANE_INFERENCE_SERVICE_API_KEY)"
embedding_engine_key="$(ensure_secret "$DATA_ENV" MODEL_PLANE_EMBEDDING_INFERENCE_SERVICE_API_KEY)"

# Application and Frontend consumers.
ensure_secret "$APPLICATION_ENV" APPLICATION_NATS_TOKEN >/dev/null
ensure_secret "$APPLICATION_ENV" APPLICATION_PLANE_DB_PASSWORD >/dev/null
ensure_secret "$APPLICATION_ENV" APPLICATION_PLANE_DRAGONFLY_PASSWORD >/dev/null
ensure_secret "$APPLICATION_ENV" AFFINE_POSTGRES_PASSWORD >/dev/null
ensure_secret "$APPLICATION_ENV" CONVEX_INSTANCE_SECRET >/dev/null
ensure_secret "$APPLICATION_ENV" JWT_SECRET >/dev/null
ensure_secret "$APPLICATION_ENV" GRAFANA_ADMIN_PASSWORD >/dev/null
sync_value MODEL_PLANE_NATS_TOKEN "$model_nats_token" "$APPLICATION_ENV"

# Model Plane inference gRPC signed-audience contract (restored :9092 auth). The
# gateway/execution callers forward an aud=inference-core delegated bearer, so
# inference-core must verify against this exact audience string.
ensure_value "$MODEL_ENV" INFERENCE_CORE_AUTH_AUDIENCE "inference-core" >/dev/null

# Application Plane conversation/notification service tokens. conversation-core
# and notification-core VALIDATE these inbound; the Frontend gateway PRESENTS the
# gateway tokens and the Ingestion email worker PRESENTS the email-ingest token,
# so each value must match across the issuing and consuming planes.
conversation_gateway_token="$(ensure_secret "$APPLICATION_ENV" CONVERSATION_GATEWAY_SERVICE_TOKEN)"
sync_value CONVERSATION_GATEWAY_SERVICE_TOKEN "$conversation_gateway_token" "$FRONTEND_ENV"
notification_gateway_token="$(ensure_secret "$APPLICATION_ENV" NOTIFICATION_GATEWAY_SERVICE_TOKEN)"
sync_value NOTIFICATION_GATEWAY_SERVICE_TOKEN "$notification_gateway_token" "$FRONTEND_ENV"
conversation_email_ingest_token="$(ensure_secret "$APPLICATION_ENV" CONVERSATION_EMAIL_INGEST_SERVICE_TOKEN)"
sync_value CONVERSATION_EMAIL_INGEST_SERVICE_TOKEN "$conversation_email_ingest_token" "$INGESTION_ENV"
ensure_secret "$APPLICATION_ENV" CONVERSATION_CORE_INGEST_SERVICE_TOKEN >/dev/null
# conversation-core gets its OWN Auth Core credential. It used to be aliased to
# integration-corev2's key here, which meant one leaked secret granted BOTH
# identities — confirmed live by SHA-256 fingerprint before this was split. The
# credential is only ever exchanged at Auth Core's internal-token route
# (conversation-core-go internal/integration/client.go), never validated by
# integration-corev2 directly, so the two are independent by construction.
conversation_core_key="$(ensure_secret "$APPLICATION_ENV" CONVERSATION_CORE_SERVICE_API_KEY)"
sync_value CONVERSATION_INTEGRATION_SERVICE_API_KEY "$conversation_core_key" "$APPLICATION_ENV"
sync_value INTEGRATION_INTERNAL_API_KEY "$internal_api_key" "$APPLICATION_ENV"
# Application-plane secrets nothing else generated: the Ingestion->Application
# publisher NATS identity and the Convex control-projection key.
ensure_secret "$APPLICATION_ENV" APPLICATION_INGESTION_PUBLISHER_NATS_PASSWORD >/dev/null
sync_value APPLICATION_INGESTION_PUBLISHER_NATS_PASSWORD \
  "$(dotenv_get "$APPLICATION_ENV" APPLICATION_INGESTION_PUBLISHER_NATS_PASSWORD)" "$INGESTION_ENV"
ensure_secret "$APPLICATION_ENV" APPLICATION_CONVEX_CONTROL_PROJECTION_KEY >/dev/null

# Conversation provider-write attestation (Ed25519): conversation-core signs
# provider-write receipts; integration-corev2 verifies them at runtime.
ensure_value "$APPLICATION_ENV" CONVERSATION_PROVIDER_WRITE_ATTESTATION_KEY_ID "conversation-provider-write-v1" >/dev/null
ensure_ed25519_attestation_key

# Model-execution provider-write attestation (Ed25519): execution-core
# (Model Plane) signs the same attestation contract for human-approved agent
# tool actions; integration-corev2 verifies both issuers under one registry.
ensure_value "$MODEL_ENV" EXECUTION_CORE_PROVIDER_WRITE_ATTESTATION_KEY_ID "model-execution-provider-write-v1" >/dev/null
ensure_model_execution_attestation_key

# Combine whichever of the above keys exist into the array integration-corev2
# actually reads. Must run after both ensure_*_attestation_key calls above.
assemble_provider_write_attestation_keys_json

# Deployment-owned registry. Each identity has its own credential and bounded
# audience/scopes; allowAnyOrg permits internal workers to serve newly-created
# tenants without trusting a caller-supplied identity or widening scopes.
# Deployment-owned registry. Each identity has its own credential and bounded
# audience/scopes; allowAnyOrg permits internal workers to serve newly-created
# tenants without trusting a caller-supplied identity or widening scopes.
#
# Reconciled 2026-08-01 against the LIVE 14-principal registry. Auth Core's
# parseRegistry throws on the FIRST bad entry and runs PER REQUEST, so one
# malformed entry 503s every service-token mint fleet-wide while /health stays
# green. Two invariants it enforces, both easy to break by hand:
#   * the union of scopesByAudience MUST exactly equal the flat `scopes` array;
#   * retentionByAudience MUST have a key for every audience, valued only
#     'zdr' or 'persistent'.
# retention is issuer-determined and inference-core treats it as a FLOOR:
# 'persistent' permits provider-side retention, 'zdr' forbids it.
#
# All 14 live principals are seeded. graph-index and embedding-engine reach the
# Model Plane embedding hop with their own credentials from $DATA_ENV; the three
# Data Plane services share the container variable NAME
# (MODEL_PLANE_INFERENCE_SERVICE_API_KEY) but compose sources it from three
# different plane-level variables, so the values are independent.
principal_registry="$(jq -cn \
  --arg retrieval "$control_policy_key" \
  --arg imports "$imports_service_key" \
  --arg model "$model_ingestion_key" \
  --arg quarry "$quarry_service_key" \
  --arg model_gateway "$model_gateway_service_key" \
  --arg execution_core "$execution_core_service_key" \
  --arg inference_routing "$inference_routing_key" \
  --arg session_core "$session_core_service_key" \
  --arg capability_core "$capability_core_key" \
  --arg orchestrator_core "$orchestrator_core_key" \
  --arg conversation_core "$conversation_core_key" \
  --arg graph_index "$graph_index_key" \
  --arg embedding_engine "$embedding_engine_key" \
  --arg finspo "$finspo_service_key" \
  '{
    "retrieval-engine": {
      credential: $retrieval,
      audiences: ["control-policy", "inference-core"],
      orgIds: [],
      allowAnyOrg: true,
      scopes: ["data:authorization:decide", "inference:invoke"],
      scopesByAudience: {
        "control-policy": ["data:authorization:decide"],
        "inference-core": ["inference:invoke"]
      },
      retentionByAudience: { "control-policy": "zdr", "inference-core": "persistent" }
    },
    "imports-core": {
      credential: $imports,
      audiences: ["data-plane"],
      orgIds: [],
      allowAnyOrg: true,
      scopes: ["documents:write"],
      scopesByAudience: { "data-plane": ["documents:write"] },
      retentionByAudience: { "data-plane": "persistent" }
    },
    "model-execution": {
      credential: $model,
      audiences: ["ingestion"],
      orgIds: [],
      allowAnyOrg: true,
      scopes: ["integration:read", "shipping:read"],
      scopesByAudience: { "ingestion": ["integration:read", "shipping:read"] },
      retentionByAudience: { "ingestion": "zdr" }
    },
    "conversation-core": {
      credential: $conversation_core,
      audiences: ["ingestion"],
      orgIds: [],
      allowAnyOrg: true,
      scopes: ["integration:read", "integration:write"],
      scopesByAudience: { "ingestion": ["integration:read", "integration:write"] },
      retentionByAudience: { "ingestion": "zdr" }
    },
    "quarry-edge": {
      credential: $quarry,
      audiences: ["data-plane"],
      orgIds: [],
      allowAnyOrg: true,
      scopes: ["data:read", "documents:write", "org:data:read_all"],
      scopesByAudience: { "data-plane": ["data:read", "documents:write", "org:data:read_all"] },
      retentionByAudience: { "data-plane": "persistent" }
    },
    "model-gateway": {
      credential: $model_gateway,
      audiences: ["session-core", "quarry"],
      orgIds: [],
      allowAnyOrg: true,
      scopes: ["scrape:read", "search:read", "session:heartbeat", "session:terminalize"],
      scopesByAudience: {
        "session-core": ["session:heartbeat", "session:terminalize"],
        "quarry": ["scrape:read", "search:read"]
      },
      retentionByAudience: { "session-core": "zdr", "quarry": "zdr" }
    },
    "execution-core": {
      credential: $execution_core,
      audiences: ["capability-core", "session-core", "quarry"],
      orgIds: [],
      allowAnyOrg: true,
      scopes: ["browser:execute", "capability:health:global:write", "capability:read", "scrape:write", "session:heartbeat", "session:terminalize"],
      scopesByAudience: {
        "capability-core": ["capability:read", "capability:health:global:write"],
        "session-core": ["session:heartbeat", "session:terminalize"],
        "quarry": ["browser:execute", "scrape:write"]
      },
      retentionByAudience: { "capability-core": "persistent", "session-core": "zdr", "quarry": "zdr" }
    },
    "finspo-core": {
      credential: $finspo,
      audiences: ["data-plane"],
      orgIds: [],
      allowAnyOrg: true,
      scopes: ["documents:write"],
      scopesByAudience: { "data-plane": ["documents:write"] },
      retentionByAudience: { "data-plane": "persistent" }
    },
    "inference-core": {
      credential: $inference_routing,
      audiences: ["session-core"],
      orgIds: [],
      allowAnyOrg: true,
      scopes: ["routing:read"],
      scopesByAudience: { "session-core": ["routing:read"] },
      retentionByAudience: { "session-core": "zdr" }
    },
    "session-core": {
      credential: $session_core,
      audiences: ["letta-bridge", "inference-core"],
      orgIds: [],
      allowAnyOrg: true,
      scopes: ["inference:invoke", "memory:read", "memory:write"],
      scopesByAudience: {
        "letta-bridge": ["memory:read", "memory:write"],
        "inference-core": ["inference:invoke"]
      },
      retentionByAudience: { "letta-bridge": "persistent", "inference-core": "persistent" }
    },
    "capability-core": {
      credential: $capability_core,
      audiences: ["session-core", "inference-core", "orchestrator-core"],
      orgIds: [],
      allowAnyOrg: true,
      scopes: ["inference:invoke", "orchestration:workflow:start", "orchestration:workflow:start:global", "session:read", "session:skills:write"],
      scopesByAudience: {
        "session-core": ["session:read", "session:skills:write"],
        "inference-core": ["inference:invoke"],
        "orchestrator-core": ["orchestration:workflow:start", "orchestration:workflow:start:global"]
      },
      retentionByAudience: { "session-core": "persistent", "inference-core": "zdr", "orchestrator-core": "persistent" }
    },
    "orchestrator-core": {
      credential: $orchestrator_core,
      audiences: ["session-core", "inference-core", "capability-core", "letta-bridge"],
      orgIds: [],
      allowAnyOrg: true,
      scopes: ["capability:read", "capability:write", "inference:invoke", "memory:read", "memory:write", "session:runs:system-owner", "session:write"],
      scopesByAudience: {
        "session-core": ["session:write", "session:runs:system-owner"],
        "inference-core": ["inference:invoke"],
        "capability-core": ["capability:read", "capability:write"],
        "letta-bridge": ["memory:read", "memory:write"]
      },
      retentionByAudience: { "session-core": "persistent", "inference-core": "persistent", "capability-core": "persistent", "letta-bridge": "persistent" }
    },
    "graph-index": {
      credential: $graph_index,
      audiences: ["inference-core"],
      orgIds: [],
      allowAnyOrg: true,
      scopes: ["inference:invoke"],
      scopesByAudience: { "inference-core": ["inference:invoke"] },
      retentionByAudience: { "inference-core": "persistent" }
    },
    "embedding-engine": {
      credential: $embedding_engine,
      audiences: ["inference-core"],
      orgIds: [],
      allowAnyOrg: true,
      scopes: ["inference:invoke"],
      scopesByAudience: { "inference-core": ["inference:invoke"] },
      retentionByAudience: { "inference-core": "persistent" }
    }
  }')"
upsert_env "$CONTROL_ENV" PLANE_SERVICE_PRINCIPALS_JSON "$principal_registry"

for env_file in "${ENV_FILES[@]}"; do
  if [[ "$DRY_RUN" == "false" ]]; then
    chmod 600 "$env_file"
  fi
done

if [[ "$DRY_RUN" == "true" ]]; then
  log "dry-run complete; no files were modified"
else
  log "runtime credentials and event-signing keys are configured"
fi
