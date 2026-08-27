#!/usr/bin/env bash
set -euo pipefail

# Local-development runner for the Control Plane. Compose interpolation happens
# before service env_file injection, so the deleted root .env cannot be relied
# upon. This runner merges the service-local files and supplies development
# values for missing interpolation variables. Generated credentials are written
# once to a local, gitignored, 0600 secrets store ($root/.env.generated-secrets)
# and reused on every run, so they stay stable across bring-ups and per-service
# recreates. It never writes credentials to a tracked repository file and must
# not be used with the production overlay.

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$root"

if [[ " ${*:-} " == *"docker-compose.production.yml"* ]]; then
  printf 'refusing production overlay: use the external secret-manager rollout\n' >&2
  exit 2
fi

env_files=(
  "$root/auth-core/.env"
  "$root/auth-core/.env.docker"
  "$root/user-core/.env"
  "$root/user-core/.env.docker"
  "$root/org-core/.env"
  "$root/org-core/.env.docker"
  "$root/audit-core/.env"
  "$root/billing-core/.env"
  "$root/billing-core/.env.docker"
  "$root/session-core/.env"
  "$root/session-core/.env.docker"
)
for file in "${env_files[@]}"; do
  [[ -f "$file" ]] || {
    printf 'missing service environment file: %s\n' "$file" >&2
    exit 1
  }
done

tmp_env=$(mktemp "${TMPDIR:-/tmp}/control-plane-compose.XXXXXX")
cleanup() {
  rm -f "$tmp_env"
}
trap cleanup EXIT INT TERM
chmod 600 "$tmp_env"

# Persisted local-development secret store. Historically every generated
# credential lived only in the ephemeral $tmp_env and was discarded on exit, so
# each bring-up minted a fresh set. That silently rotated BETTER_AUTH_SECRET and
# TOKEN_ENCRYPTION_KEY (invalidating sessions and making stored OAuth tokens
# undecryptable) and guaranteed a single-service recreate would mismatch the
# rest of the running stack. Persisting generated values to this 0600 file keeps
# them STABLE across runs and safe for per-service recreates. The file is
# .gitignore-covered (.env.*), so this honours "never write credentials to the
# repository": it is a local, untracked, development-only store — never staged,
# never committed, never used with the production overlay.
secrets_file="${CONTROL_PLANE_SECRETS_FILE:-$root/.env.generated-secrets}"
if [[ ! -f "$secrets_file" ]]; then
  umask 077
  : >"$secrets_file"
fi
chmod 600 "$secrets_file"
# Read back last: later --env-file wins in compose interpolation, and appearing
# in env_files lets lookup_value/persist_if_missing find already-persisted keys.
env_files+=("$secrets_file")

lookup_value() {
  local key=$1 file line value
  for file in "${env_files[@]}"; do
    line=$(grep -E "^${key}=" "$file" | tail -n 1 || true)
    [[ -n "$line" ]] || continue
    value=${line#*=}
    if [[ -n "$value" ]]; then
      printf '%s' "$value"
      return 0
    fi
  done
  return 1
}

database_value_from_urls() {
  local component=$1 file line url value
  for file in "${env_files[@]}"; do
    line=$(grep -E '^DATABASE_URL=' "$file" | tail -n 1 || true)
    [[ -n "$line" ]] || continue
    url=${line#*=}
    [[ "$url" =~ ^[A-Za-z][A-Za-z0-9+.-]*://[^:]+:([^@]*)@ ]] || continue
    if [[ "$component" == user ]]; then
      value=${url#*://}
      value=${value%%:*}
    else
      value=${BASH_REMATCH[1]}
    fi
    if [[ -n "$value" ]]; then
      printf '%s' "$value"
      return 0
    fi
  done
  return 1
}

set_if_missing() {
  local key=$1 value=$2
  if ! lookup_value "$key" >/dev/null 2>&1; then
    printf '%s=%s\n' "$key" "$value" >>"$tmp_env"
  fi
}

# Like set_if_missing, but PERSISTS the generated value to the secrets file so
# it survives across bring-ups (and is reused by lookup_value next run). Because
# $secrets_file is part of env_files, the value is also visible to this run's
# compose interpolation. Use for credentials that must be stable; use
# set_if_missing for disposable per-run metadata (build stamps, cosmetic config).
persist_if_missing() {
  local key=$1 value=$2
  if ! lookup_value "$key" >/dev/null 2>&1; then
    printf '%s=%s\n' "$key" "$value" >>"$secrets_file"
  fi
}

random_value() {
  openssl rand -hex 32
}

# base64url WITHOUT padding — the encoding every Space-decision peer uses.
# Verified against both verifier implementations: Go reads the key with
# `base64.RawURLEncoding` (documents-api-go/internal/handler/space_import.go,
# user-core/internal/spaces/decision.go) and Rust with `URL_SAFE_NO_PAD`
# (execution-core/src/scheduled_step_decision.rs). Standard base64 would decode
# to the wrong bytes (or fail outright) on any key containing - or _.
base64url_no_pad() {
  openssl base64 -A | tr '+/' '-_' | tr -d '='
}

# Convex-Auth JWT/JWKS signing key (RS256). docker-compose.yml volume-mounts
# ./auth-core/keys read-only into the container at /app/keys; without a real
# keypair there, convex-token.service.ts's readKeyFile() throws "Configured
# signing key file is not readable" — the compose default (${..:-/app/keys/...})
# is a non-empty path, so the code's own graceful ephemeral-key fallback (which
# only triggers when the env var is entirely unset) never runs, and auth-core
# crash-loops on every start. Generated once and reused, matching
# bootstrap_runtime_environment.sh's ensure_event_keypair pattern for this
# plane's other signing keys.
convex_auth_keys_dir="$root/auth-core/keys"
convex_auth_private_key="$convex_auth_keys_dir/convex-auth.key"
convex_auth_public_key="$convex_auth_keys_dir/convex-auth.pub"
if [[ ! -s "$convex_auth_private_key" || ! -s "$convex_auth_public_key" ]]; then
  mkdir -p "$convex_auth_keys_dir"
  # A bind-mount attempted before this ever ran can leave Docker's
  # auto-vivified directory placeholder here instead of a file; clear it.
  [[ -d "$convex_auth_private_key" ]] && rmdir "$convex_auth_private_key" 2>/dev/null
  [[ -d "$convex_auth_public_key" ]] && rmdir "$convex_auth_public_key" 2>/dev/null
  openssl genpkey -algorithm RSA -pkeyopt rsa_keygen_bits:2048 -out "$convex_auth_private_key" >/dev/null 2>&1
  openssl pkey -in "$convex_auth_private_key" -pubout -out "$convex_auth_public_key" >/dev/null 2>&1
  chmod 600 "$convex_auth_private_key"
  chmod 644 "$convex_auth_public_key"
fi

# Space-decision signing keypair (Ed25519). Control (user-core) SIGNS Space
# decisions with the private half; Model, Data and Ingestion plane services
# VERIFY with the public half, so the two must be generated together and stay
# in lockstep — a mismatched pair fails every Space authority check with a
# signature error rather than a config error, which is far harder to read.
#
# These arrived as new `${..:?}` requirements with the Spaces authority work and
# nothing provisioned them, so every Control Plane `docker compose` invocation
# (including a single-service build) failed interpolation before reaching any
# service. Generated once here and reused, matching the Convex-Auth keypair
# above and bootstrap_runtime_environment.sh's ensure_event_keypair pattern.
#
# Ed25519 PKCS#8 DER is a fixed 48-byte structure whose trailing 32 bytes are
# the seed, and the SubjectPublicKeyInfo DER's trailing 32 bytes are the public
# key — hence `tail -c 32`. user-core accepts either a 32-byte seed or a
# 64-byte expanded key (decision.go's ed25519.SeedSize / PrivateKeySize switch);
# the seed is the smaller, canonical form.
if ! lookup_value CONTROL_SPACE_DECISION_PRIVATE_KEY_BASE64 >/dev/null 2>&1 \
  || ! lookup_value CONTROL_SPACE_DECISION_PUBLIC_KEY_BASE64 >/dev/null 2>&1; then
  space_decision_der=$(mktemp)
  openssl genpkey -algorithm ed25519 -outform DER -out "$space_decision_der" >/dev/null 2>&1
  space_decision_private=$(tail -c 32 "$space_decision_der" | base64url_no_pad)
  space_decision_public=$(openssl pkey -inform DER -in "$space_decision_der" -pubout -outform DER 2>/dev/null \
    | tail -c 32 | base64url_no_pad)
  rm -f "$space_decision_der"
  if [[ -n "$space_decision_private" && -n "$space_decision_public" ]]; then
    # Regenerate BOTH halves whenever either is missing, so a half-provisioned
    # store can never leave a public key that does not match the private one.
    persist_if_missing CONTROL_SPACE_DECISION_KEY_ID "control-space-decision-$(openssl rand -hex 8)"
    persist_if_missing CONTROL_SPACE_DECISION_PRIVATE_KEY_BASE64 "$space_decision_private"
    persist_if_missing CONTROL_SPACE_DECISION_PUBLIC_KEY_BASE64 "$space_decision_public"
  else
    printf 'WARNING: could not generate the Space-decision Ed25519 keypair; Space authority checks will fail.\n' >&2
  fi
fi

# AES-256-GCM key for Model Plane session-core's approval-continuation
# descriptors. NOT the base64url form used above: continuation_crypto.rs decodes
# with base64::STANDARD and then hard-requires exactly 32 bytes, so this is
# padded standard base64 of 32 random bytes. Using base64url_no_pad here would
# decode to the wrong bytes on any key containing - or _, and silently only for
# some keys.
#
# Without it the HITL boundary does not merely fail to resume -- session-core
# treats a missing key as "descriptor boundary unavailable", so the PAUSE
# itself returns unavailable and an approval can never even be requested.
persist_if_missing SESSION_CORE_CONTINUATION_DESCRIPTOR_KEY "$(openssl rand -base64 32)"

# Keep local database URLs internally consistent with the generated database
# password. Existing non-empty service-local values always win.
db_user=$(lookup_value DB_USER || database_value_from_urls user || printf 'coresystem')
db_password=$(lookup_value DB_PASSWORD || database_value_from_urls password || random_value)
dragonfly_password=$(lookup_value DRAGONFLY_PASSWORD || random_value)
persist_if_missing DB_USER "$db_user"
persist_if_missing DB_PASSWORD "$db_password"
persist_if_missing DRAGONFLY_PASSWORD "$dragonfly_password"
set_if_missing SOURCE_REVISION local
set_if_missing BUILD_DATE "$(date -u +%Y-%m-%dT%H:%M:%SZ)"
set_if_missing GRAFANA_ADMIN_USER admin
persist_if_missing GRAFANA_ADMIN_PASSWORD "$(random_value)"
# PERSISTED, not set_if_missing: auth-core's docker-compose.yml requires this
# (${..:?}), and build-verevon-services.sh interpolates Compose directly rather
# than through this script's $tmp_env — an ephemeral default here is invisible
# to that entry point and its compose config validation fails.
# Seeded EMPTY on purpose: the principal registry is deployment policy, not a
# generated secret, so it is maintained in the local store rather than minted
# here. That does mean a fresh machine starts with no principals at all and
# every cross-plane lane is closed until they are added back.
#
# The governed ticket-action lane (Phase 0) needs the `execution-core`
# principal to carry, for audience session-core:
#     scopesByAudience.session-core   includes  approval:deliver
#     retentionByAudience.session-core = persistent
#
# approval:deliver is what session-core's authorize_operation() checks before
# it will hand out an approval-delivery claim. The retention marker is a real
# DATA-RETENTION DECISION, not a toggle: session-core refuses durable writes
# from a credential minted as `zdr`, and approval receipts are durable by
# nature. It was set to persistent deliberately for this audience only --
# quarry stays `zdr`. Revisit it with whoever owns retention policy before
# copying this shape to another audience.
persist_if_missing PLANE_SERVICE_PRINCIPALS_JSON '{}'

# ── Plane service principal registry ────────────────────────────────────────
#
# Assembled from config/plane-service-principals.json, which is COMMITTED and
# holds the policy half of each principal: audiences, scopes, per-audience
# scopes, per-audience retention, org binding. The secret half -- one credential
# per principal -- is minted here and only ever written to this machine's
# gitignored store.
#
# Why this exists: the registry used to be hand-maintained inside the store, so
# a fresh checkout produced NO principals at all and every cross-plane lane was
# closed with nothing in the repo explaining what was missing. Policy belongs in
# git; secrets do not; this splits them.
#
# Credentials are never rotated by this function. An existing value is reused
# so a re-run cannot invalidate a credential the running fleet is already
# presenting -- the same reasoning as the persisted secrets store itself.
# Path a NATIVE tool can open. jq/openssl on Windows are win32 binaries and
# cannot resolve the /c/Users/... form bash hands them; `[[ -f ]]` succeeds on
# the POSIX path while the tool fails on the very same string, so the mismatch
# looks like a missing file rather than a path-format problem.
native_path() {
  if command -v cygpath >/dev/null 2>&1; then cygpath -m "$1"; else printf '%s' "$1"; fi
}

# jq on Windows writes CRLF. Command substitution strips a trailing newline but
# NOT the , and `read -r` keeps it too -- so an unstripped key becomes
# "capability-core", every registry lookup for it misses, and the generator
# quietly mints a replacement credential for a principal that already had one.
# Every jq call in this script goes through here.
jqr() { jq "$@" | tr -d '\015'; }

plane_principals_registry() {
  local policy_posix="$root/config/plane-service-principals.json" policy
  policy=$(native_path "$policy_posix")
  if [[ ! -f "$policy_posix" ]]; then
    printf 'WARNING: %s is missing; leaving PLANE_SERVICE_PRINCIPALS_JSON untouched.\n' "$policy" >&2
    return 0
  fi
  if ! command -v jq >/dev/null 2>&1; then
    # Fail loudly. A silently skipped step here would leave every cross-plane
    # lane closed while the bring-up still looked successful -- the exact
    # failure mode a missing `rg` produced in the cross-plane preflight.
    printf 'ERROR: jq is required to assemble PLANE_SERVICE_PRINCIPALS_JSON from %s\n' "$policy" >&2
    printf '       Install jq, or set PLANE_SERVICE_PRINCIPALS_JSON yourself before running.\n' >&2
    return 1
  fi

  local existing creds='{}' name cred consumer consumer_value
  existing=$(lookup_value PLANE_SERVICE_PRINCIPALS_JSON 2>/dev/null || printf '{}')
  printf '%s' "$existing" | jq empty >/dev/null 2>&1 || existing='{}'

  while IFS= read -r name; do
    [[ -n "$name" ]] || continue
    cred=$(printf '%s' "$existing" | jqr -r --arg n "$name" '.[$n].credential // empty')
    consumer=$(jqr -r --arg n "$name" '.[$n].consumerEnv // empty' "$policy")

    if [[ -n "$consumer" ]]; then
      consumer_value=$(lookup_value "$consumer" 2>/dev/null || true)
    else
      consumer_value=""
    fi

    if [[ -z "$cred" ]]; then
      # No registered credential yet: adopt the consumer's existing value if it
      # has one (that is the token already being presented), else mint.
      cred="${consumer_value:-$(random_value)}"
    elif [[ -n "$consumer_value" && "$consumer_value" != "$cred" ]]; then
      # Registry and consumer disagree. Report rather than silently pick a side:
      # whichever is wrong, one end is presenting a credential the other will
      # reject, and that is a drift bug to fix at its source -- not something to
      # paper over on every bring-up.
      printf 'WARNING: principal %s: %s does not match the registered credential; the presenting service will be rejected.\n' \
        "$name" "$consumer" >&2
    fi

    # Publish to the consumer variable so both ends resolve ONE value. Existing
    # values are left alone (persist_if_missing), so this never rotates a peer.
    [[ -n "$consumer" ]] && persist_if_missing "$consumer" "$cred"

    creds=$(printf '%s' "$creds" | jqr --arg n "$name" --arg c "$cred" '.[$n] = $c')
  done < <(jqr -r 'keys[]' "$policy")

  local registry
  registry=$(jqr -c -n --slurpfile pol "$policy" --argjson creds "$creds" '
    reduce ($pol[0] | keys[]) as $n ({};
      .[$n] = {
        credential: $creds[$n],
        audiences: $pol[0][$n].audiences,
        orgIds: $pol[0][$n].orgIds,
        allowAnyOrg: $pol[0][$n].allowAnyOrg,
        scopes: $pol[0][$n].scopes,
        scopesByAudience: $pol[0][$n].scopesByAudience,
        retentionByAudience: $pol[0][$n].retentionByAudience
      })
  ') || return 1

  if [[ -n "$(lookup_value PLANE_SERVICE_PRINCIPALS_JSON 2>/dev/null || true)" ]]; then
    # Replace in place: the assembled value is authoritative once policy exists.
    local tmp; tmp=$(mktemp)
    grep -v '^PLANE_SERVICE_PRINCIPALS_JSON=' "$secrets_file" > "$tmp" 2>/dev/null || true
    printf 'PLANE_SERVICE_PRINCIPALS_JSON=%s\n' "$registry" >> "$tmp"
    cat "$tmp" > "$secrets_file"
    rm -f "$tmp"
  else
    printf 'PLANE_SERVICE_PRINCIPALS_JSON=%s\n' "$registry" >> "$secrets_file"
  fi
  printf 'plane service principals: %s assembled from config/plane-service-principals.json\n' \
    "$(printf '%s' "$registry" | jqr -r 'keys | length')" >&2
}
plane_principals_registry


# Session-signing and token-at-rest keys MUST be stable: rotating them logs
# every user out and renders stored encrypted OAuth tokens undecryptable.
persist_if_missing BETTER_AUTH_SECRET "$(random_value)"
persist_if_missing TOKEN_ENCRYPTION_KEY "$(random_value)"
persist_if_missing RESEND_API_KEY "$(random_value)"
set_if_missing RESEND_FROM_EMAIL dev@example.test
set_if_missing VEREVON_PUBLIC_ORIGIN http://localhost:5173
# Plane-token issuer. Some service definitions require it (${..:?}) while others
# document the same default (${..:-...}); supply that default so a bring-up or a
# single-service recreate resolves without an operator-exported value.
# PERSISTED (see PLANE_SERVICE_PRINCIPALS_JSON above): required with ${..:?} by
# name in docker-compose.yml, so an ephemeral-only default is invisible to
# build-verevon-services.sh's direct `docker compose config` interpolation.
persist_if_missing AUTH_CORE_ISSUER http://localhost:3011/api/convex-auth
set_if_missing BETTER_AUTH_TRUSTED_ORIGINS 'http://localhost:3185,http://127.0.0.1:3185,http://localhost:5173,http://127.0.0.1:5173'

# Compose's development overlay requires pairwise-distinct values for these
# scoped NATS and HTTP/gRPC principals. They are generated once into the 0600
# persisted secrets store and reused on every subsequent run, so cross-plane
# credentials stay consistent and single-service recreates do not mismatch the
# running stack.
required_credentials=(
  APPLICATION_CONVEX_CONTROL_NATS_PASSWORD APPLICATION_CONVEX_CONTROL_PROJECTION_KEY
  APPLICATION_CONVEX_MODEL_NATS_PASSWORD APPLICATION_INSIGHT_MODEL_NATS_PASSWORD
  APPLICATION_NATS_PROVISIONER_PASSWORD
  APPLICATION_RECONCILER_AUTH_TOKEN APPLICATION_SPACE_LIFECYCLE_TOKEN
  APPLICATION_ORG_CORE_SERVICE_TOKEN
  AUDIT_APPLICATION_NATS_PASSWORD
  AUDIT_CONTROL_NATS_PASSWORD AUDIT_MODEL_NATS_PASSWORD AUTH_BILLING_CORE_SERVICE_TOKEN
  AUTH_NATS_PASSWORD AUTH_ORG_CORE_SERVICE_TOKEN AUTH_SHARED_NATS_PASSWORD
  AUTH_USER_CORE_GRPC_SERVICE_TOKEN BILLING_NATS_PASSWORD BILLING_ORG_CORE_SERVICE_TOKEN
  BILLING_SHARED_NATS_PASSWORD CONTROL_NATS_PROVISIONER_PASSWORD CONTROL_SHARED_BRIDGE_PASSWORD
  CONTROL_SHARED_NATS_PROVISIONER_PASSWORD CONTROL_SPACE_POLICY_TOKEN
  DOCUMENTS_GDPR_NATS_PASSWORD
  CONVERSATION_CORE_GDPR_NATS_PASSWORD COST_CORE_GDPR_NATS_PASSWORD
  DATA_ORCHESTRATOR_GDPR_NATS_PASSWORD DATA_QUALITY_GDPR_NATS_PASSWORD
  EMBEDDING_ENGINE_GDPR_NATS_PASSWORD EXECUTION_ORG_CORE_SERVICE_TOKEN
  EXECUTION_CORE_USER_CORE_GRPC_TOKEN
  # The governed agent ticket-action lane: execution-core -> user-core
  # run-action decision -> conversation-core owner effect. Minted HERE, once,
  # because every one of these is read by BOTH ends. user-core validates the
  # two Control tokens against USER_CORE_SERVICE_CREDENTIALS while
  # execution-core presents them, and conversation-core validates
  # CONVERSATION_EXECUTION_CORE_SERVICE_TOKEN as its execution-core delegation
  # key while execution-core presents that one too. Minting either end
  # independently is precisely what produced the credential mismatches this
  # fleet has been carrying.
  EXECUTION_CORE_CONTROL_RUN_ACTION_TOKEN
  EXECUTION_CORE_CONTROL_MODEL_ACTION_VIEW_TOKEN
  CONVERSATION_EXECUTION_CORE_SERVICE_TOKEN
  # Presented BY conversation-core TO user-core for the two owner-effect
  # checks that fence a governed ticket action (current authority, then the
  # reservation/commit pair).
  CONVERSATION_CONTROL_RUN_ACTION_AUTHORITY_TOKEN
  CONVERSATION_CONTROL_OWNER_EFFECT_RESERVATION_TOKEN
  GRAPH_INDEX_GDPR_NATS_PASSWORD INDEX_ENGINE_GDPR_NATS_PASSWORD
  NOTIFICATION_CORE_GDPR_NATS_PASSWORD QUARRY_CONTROL_GDPR_NATS_PASSWORD
  QUICKWIT_ADAPTER_GDPR_NATS_PASSWORD RETRIEVAL_ENGINE_GDPR_NATS_PASSWORD
  SESSION_CORE_GDPR_NATS_PASSWORD WIKI_STORE_GDPR_NATS_PASSWORD
  VEREVON_GATEWAY_GDPR_NATS_PASSWORD
  GATEWAY_AUDIT_CORE_SERVICE_TOKEN GATEWAY_AUTH_GRPC_SERVICE_TOKEN
  GATEWAY_BILLING_CORE_SERVICE_TOKEN GATEWAY_ORG_CORE_SERVICE_TOKEN
  INTEGRATION_AUDIT_CORE_SERVICE_TOKEN INTEGRATION_BILLING_CORE_SERVICE_TOKEN
  INTEGRATION_ORG_CORE_SERVICE_TOKEN MODEL_GATEWAY_ORG_CORE_SERVICE_TOKEN
  MODEL_NATS_PROVISIONER_PASSWORD
  ORG_GROUP_GRANT_SERVICE_TOKEN
  ORG_NATS_PASSWORD ORG_SHARED_NATS_PASSWORD QUARRY_AUTH_INTERNAL_SERVICE_TOKEN
  RETRIEVAL_AUTH_GRPC_SERVICE_TOKEN SESSION_BILLING_CORE_SERVICE_TOKEN
  SESSION_CORE_SERVICE_TOKEN SESSION_NATS_PASSWORD SESSION_ORG_CORE_SERVICE_TOKEN
  SESSION_SHARED_NATS_PASSWORD USER_AUTH_INTERNAL_SERVICE_TOKEN USER_CORE_AUTH_TOKEN
  USER_CORE_DOCUMENTS_TOKEN USER_CORE_GATEWAY_TOKEN USER_CORE_GRPC_CLIENT_CREDENTIAL_FILE
  USER_CORE_GRPC_SERVICE_CREDENTIALS_FILE USER_CORE_GRPC_TLS_CA_FILE
  USER_CORE_GRPC_TLS_CERT_FILE USER_CORE_GRPC_TLS_KEY_FILE USER_CORE_MEMBERSHIP_SERVICE_TOKEN
  USER_CORE_ORG_TOKEN USER_CORE_RETRIEVAL_TOKEN USER_CORE_SESSION_TOKEN USER_NATS_PASSWORD
  USER_SHARED_NATS_PASSWORD
)
for key in "${required_credentials[@]}"; do
  case "$key" in
    *_FILE)
      # File-backed credentials belong to the production overlay and are not
      # needed by the development Compose environment; keep a safe path value
      # only if a local contract references the name.
      set_if_missing "$key" "/tmp/control-plane/$key"
      ;;
    *)
      persist_if_missing "$key" "$(random_value)"
      ;;
  esac
done

set_if_missing DB_NAME controlplane

compose_env_args=(--env-file "$tmp_env")
for file in "${env_files[@]}"; do
  compose_env_args+=(--env-file "$file")
done

case " ${*:-} " in
  *" up "*|*" start "*)
    if ! docker network inspect inter-plane-bus >/dev/null 2>&1; then
      docker network create --driver bridge \
        --label com.docker.compose.network=inter-plane-bus \
        --label com.docker.compose.project=triodelab \
        inter-plane-bus >/dev/null
    fi
    ;;
esac

exec docker compose "${compose_env_args[@]}" -f docker-compose.yml "$@"
