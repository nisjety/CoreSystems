#!/usr/bin/env bash
set -euo pipefail

# Values-free readiness check for the mail-to-task bindings (chat gap closure
# plan task J3). It reports, per binding, whether it is `present` or `absent`
# and NEVER prints a value, a hash, or a length: every comparison between two
# copies of a secret is made on a SHA-256 digest held in memory and reported as
# yes/no.
#
# Mail-to-task ships dark. Three kinds of binding exist, and the check treats
# them differently:
#
#   switch   set by the owner at switch-on, empty until then
#              MAIL_TASK_PROVIDER, MAIL_TASK_WEBHOOK_SECRET,
#              MAIL_TASK_RECEIVING_DOMAIN, MAIL_TASK_GATEWAY_SERVICE_TOKEN
#   minted   minted together by scripts/run-control-plane.sh, present from the
#            first launcher run after J3 whether or not the feature is on
#              USER_CORE_MAIL_TASK_WORKER_TOKEN, MAIL_TASK_BRIDGE_SERVICE_API_KEY,
#              MAIL_TASK_GATEWAY_BRIDGE_SERVICE_API_KEY
#   optional MAIL_TASK_IMPORTS_CORE_TOKEN (empty = text-only attachments),
#            MAIL_TASK_RATE_LIMIT_PER_HOUR (empty = 20 per hour)
#
# Result:
#   feature off  no switch is set and the minted bindings are all present or all
#                absent. Exit 0.
#   ready        every switch and every minted binding is present and valid.
#                Exit 0.
#   half-set     anything in between: a switch set while another is absent, the
#                minted bindings only partly present, a secret shorter than 32
#                characters, a provider other than postmark, a non-numeric rate
#                limit, or (with --live) two copies of one secret that differ.
#                Exit 1.
#   Exit 2 is a usage error. Exit 3 is "--live was asked for and docker could
#   not be reached", reported only when nothing else failed.
#
# Where a binding is read from, highest precedence first: the process
# environment, each --env-file (the last one wins), then the Control Plane
# secrets store (CONTROL_PLANE_SECRETS_FILE, else .env.generated-secrets next to
# this scripts directory). The store holds the minted bindings; the switches are
# owner-supplied and normally come from the environment.
#
# --live additionally inspects the RUNNING containers, read-only (docker inspect
# and docker exec of a fixed one-line probe that prints only presence and a
# digest comparison): which bindings each service really received, whether the
# two ends of each shared secret agree, and whether auth-service registered the
# two Auth Core principals with the credentials in the store.

script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
root=$(cd "$script_dir/.." && pwd)

usage() {
  cat <<'USAGE'
Usage: check-mail-task-bindings.sh [--store FILE | --no-store] [--env-file FILE]... [--live]

  --store FILE     Control Plane secrets store to read (default:
                   $CONTROL_PLANE_SECRETS_FILE or <Control Plane>/.env.generated-secrets)
  --no-store       do not read the secrets store
  --env-file FILE  extra dotenv file to read (repeatable, the last one wins)
  --live           also inspect the running containers (read-only)

Prints present/absent per binding, never a value. Exit 0: feature off or ready.
Exit 1: a binding is half-set or invalid. Exit 2: usage. Exit 3: --live could
not reach docker.
USAGE
}

usage_error() {
  printf 'check-mail-task-bindings: %s\n' "$1" >&2
  usage >&2
  exit 2
}

store_file="${CONTROL_PLANE_SECRETS_FILE:-$root/.env.generated-secrets}"
use_store=1
live=0
env_files=()
while (( $# > 0 )); do
  case "$1" in
    --store)
      (( $# >= 2 )) || usage_error '--store needs a file'
      store_file=$2
      use_store=1
      shift 2
      ;;
    --no-store)
      use_store=0
      shift
      ;;
    --env-file)
      (( $# >= 2 )) || usage_error '--env-file needs a file'
      env_files+=("$2")
      shift 2
      ;;
    --live)
      live=1
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      usage_error "unknown argument: $1"
      ;;
  esac
done

switch_names=(
  MAIL_TASK_PROVIDER MAIL_TASK_WEBHOOK_SECRET MAIL_TASK_RECEIVING_DOMAIN
  MAIL_TASK_GATEWAY_SERVICE_TOKEN
)
minted_names=(
  USER_CORE_MAIL_TASK_WORKER_TOKEN MAIL_TASK_BRIDGE_SERVICE_API_KEY
  MAIL_TASK_GATEWAY_BRIDGE_SERVICE_API_KEY
)
optional_names=(MAIL_TASK_IMPORTS_CORE_TOKEN MAIL_TASK_RATE_LIMIT_PER_HOUR)
min_secret_length=32

consumers_of() {
  case "$1" in
    MAIL_TASK_PROVIDER|MAIL_TASK_WEBHOOK_SECRET) printf 'integration-api' ;;
    MAIL_TASK_RECEIVING_DOMAIN) printf 'integration-api, user-core' ;;
    MAIL_TASK_GATEWAY_SERVICE_TOKEN)
      printf 'integration-api, gateway BFF as MAIL_TASK_INGEST_SERVICE_TOKEN' ;;
    USER_CORE_MAIL_TASK_WORKER_TOKEN)
      printf 'user-core registry, integration-api as MAIL_TASK_USER_CORE_SERVICE_TOKEN' ;;
    MAIL_TASK_BRIDGE_SERVICE_API_KEY)
      printf 'Auth Core principal model-gateway-mail-task-bridge, model-gateway' ;;
    MAIL_TASK_GATEWAY_BRIDGE_SERVICE_API_KEY)
      printf 'Auth Core principal verevon-gateway, gateway BFF' ;;
    MAIL_TASK_IMPORTS_CORE_TOKEN) printf 'integration-api, empty means text-only' ;;
    MAIL_TASK_RATE_LIMIT_PER_HOUR) printf 'gateway BFF, empty means 20 per hour' ;;
    *) printf '' ;;
  esac
}

sha256_of() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum | cut -d' ' -f1
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 | cut -d' ' -f1
  else
    openssl dgst -sha256 | sed 's/^.*= //'
  fi
}

# KEY=VALUE reader, same rule as the launcher's lookup_value: the last matching
# line wins, CR is dropped, one pair of surrounding quotes is removed. Prints the
# value; returns 1 when the file or a non-empty value is missing.
file_lookup() {
  local file=$1 name=$2 line value
  [[ -f "$file" ]] || return 1
  line=$(grep -E "^${name}=" "$file" 2>/dev/null | tail -n 1 | tr -d '\r' || true)
  [[ -n "$line" ]] || return 1
  value=${line#*=}
  case "$value" in
    \"*\") value=${value#\"}; value=${value%\"} ;;
    \'*\') value=${value#\'}; value=${value%\'} ;;
  esac
  [[ -n "$value" ]] || return 1
  printf '%s' "$value"
}

# Prints the value of a binding and returns 0, or returns 1 when it is absent.
# Callers keep the value in a shell variable only to measure or digest it.
binding_value() {
  local name=$1 index
  if [[ -n "${!name-}" ]]; then
    printf '%s' "${!name}"
    return 0
  fi
  for (( index = ${#env_files[@]} - 1; index >= 0; index-- )); do
    if file_lookup "${env_files[$index]}" "$name"; then
      return 0
    fi
  done
  if (( use_store )) && file_lookup "$store_file" "$name"; then
    return 0
  fi
  return 1
}

# Where a present binding came from: env, file or store. Never the value.
binding_source() {
  local name=$1 index
  if [[ -n "${!name-}" ]]; then
    printf 'environment'
    return 0
  fi
  for (( index = ${#env_files[@]} - 1; index >= 0; index-- )); do
    if file_lookup "${env_files[$index]}" "$name" >/dev/null; then
      printf 'env-file'
      return 0
    fi
  done
  if (( use_store )) && file_lookup "$store_file" "$name" >/dev/null; then
    printf 'secrets store'
    return 0
  fi
  return 1
}

problems=()
notes=()
add_problem() { problems+=("$1"); }
add_note() { notes+=("$1"); }

present_switches=0
present_minted=0
absent_switches=()
absent_minted=()

report_binding() {
  local role=$1 name=$2 source
  if source=$(binding_source "$name"); then
    printf '  %-8s %-42s present (%s)\n' "$role" "$name" "$source"
    return 0
  fi
  printf '  %-8s %-42s absent\n' "$role" "$name"
  return 1
}

printf 'mail-to-task bindings (values are never printed)\n'
if (( use_store )); then
  if [[ -f "$store_file" ]]; then
    printf 'secrets store: read\n'
  else
    printf 'secrets store: not found\n'
  fi
else
  printf 'secrets store: not read\n'
fi

for name in "${switch_names[@]}"; do
  if report_binding switch "$name"; then
    present_switches=$((present_switches + 1))
  else
    absent_switches+=("$name")
  fi
done
for name in "${minted_names[@]}"; do
  if report_binding minted "$name"; then
    present_minted=$((present_minted + 1))
  else
    absent_minted+=("$name")
  fi
done
for name in "${optional_names[@]}"; do
  report_binding optional "$name" || true
done

# Value-free validity: lengths and shapes, reported as problems by name only.
secret_too_short() {
  local name=$1 value
  value=$(binding_value "$name" || true)
  [[ -n "$value" ]] && (( ${#value} < min_secret_length ))
}

if (( present_switches > 0 )); then
  for name in "${absent_switches[@]+"${absent_switches[@]}"}"; do
    add_problem "switch is on but $name is absent"
  done
  if (( present_minted == 0 )); then
    for name in "${minted_names[@]}"; do
      add_problem "switch is on but $name is absent (run scripts/run-control-plane.sh to mint it)"
    done
  fi
fi
if (( present_minted > 0 && present_minted < ${#minted_names[@]} )); then
  for name in "${absent_minted[@]}"; do
    add_problem "minted bindings are only partly present: $name is absent (re-run scripts/run-control-plane.sh, which mints them together)"
  done
fi
for name in MAIL_TASK_GATEWAY_SERVICE_TOKEN "${minted_names[@]}"; do
  if secret_too_short "$name"; then
    add_problem "$name is shorter than $min_secret_length characters"
  fi
done
if binding_value MAIL_TASK_PROVIDER >/dev/null; then
  provider=$(binding_value MAIL_TASK_PROVIDER | tr '[:upper:]' '[:lower:]')
  if [[ "$provider" != "postmark" ]]; then
    add_problem "MAIL_TASK_PROVIDER is set but is not postmark, so the receiver treats the feature as off"
  fi
fi
if binding_value MAIL_TASK_RATE_LIMIT_PER_HOUR >/dev/null; then
  rate=$(binding_value MAIL_TASK_RATE_LIMIT_PER_HOUR)
  if ! [[ "$rate" =~ ^[0-9]+([.][0-9]+)?$ ]] || [[ "$rate" =~ ^0+([.]0+)?$ ]]; then
    add_problem "MAIL_TASK_RATE_LIMIT_PER_HOUR is set but is not a positive number, so the gateway falls back to 20"
  fi
fi

# --- live containers -------------------------------------------------------

live_unreachable=0

# One probe, run INSIDE the container: prints `absent`, or the SHA-256 digest of
# the variable's value. The digest is compared locally and never printed.
probe_env() {
  local container=$1 variable=$2
  MSYS_NO_PATHCONV=1 docker exec "$container" sh -c '
    v=$(printenv "$1" 2>/dev/null || true)
    if [ -z "$v" ]; then echo absent; exit 0; fi
    printf %s "$v" | sha256sum | cut -d" " -f1
  ' sh "$variable" 2>/dev/null
}

# Compares one live binding with the offline copy, by digest. Sets live_state to
# absent|present|unreadable and live_match to yes|no|n/a.
live_state=''
live_match=''
live_check() {
  local container=$1 variable=$2 offline_name=$3 answer offline offline_digest
  answer=$(probe_env "$container" "$variable" || true)
  live_state=absent
  live_match='n/a'
  if [[ -z "$answer" ]]; then
    live_state=unreadable
    return 0
  fi
  if [[ "$answer" == absent ]]; then
    return 0
  fi
  live_state=present
  if [[ -n "$offline_name" ]] && offline=$(binding_value "$offline_name"); then
    offline_digest=$(printf '%s' "$offline" | sha256_of)
    if [[ "$answer" == "$offline_digest" ]]; then
      live_match=yes
    else
      live_match=no
    fi
  fi
}

# A live binding's digest for a cross-container comparison, never printed.
# Empty when the binding is absent or unreadable.
live_digest() {
  local container=$1 variable=$2 answer
  answer=$(probe_env "$container" "$variable" || true)
  if [[ "$answer" == absent ]]; then
    answer=''
  fi
  printf '%s' "$answer"
}

container_running() {
  [[ "$(MSYS_NO_PATHCONV=1 docker inspect -f '{{.State.Running}}' "$1" 2>/dev/null || true)" == true ]]
}

live_section() {
  if ! command -v docker >/dev/null 2>&1 || ! docker version >/dev/null 2>&1; then
    printf '\nlive: docker is not reachable, containers were not inspected\n'
    live_unreachable=1
    return 0
  fi
  local integration=${MAIL_TASK_CHECK_INTEGRATION_CONTAINER:-integration-api}
  local gateway=${MAIL_TASK_CHECK_GATEWAY_CONTAINER:-verevon-gateway-rs}
  local model_gateway=${MAIL_TASK_CHECK_MODEL_GATEWAY_CONTAINER:-model-plane-model-gateway-1}
  local user_core=${MAIL_TASK_CHECK_USER_CORE_CONTAINER:-user-service}
  local auth=${MAIL_TASK_CHECK_AUTH_CONTAINER:-auth-service}
  local live_switch_set=0 row container variable offline_name label

  printf '\nlive containers (read-only; presence and agreement with the offline copy only)\n'
  # container|variable inside it|offline binding it must agree with|kind
  local rows=(
    "$integration|MAIL_TASK_PROVIDER|MAIL_TASK_PROVIDER|switch"
    "$integration|MAIL_TASK_WEBHOOK_SECRET|MAIL_TASK_WEBHOOK_SECRET|switch"
    "$integration|MAIL_TASK_RECEIVING_DOMAIN|MAIL_TASK_RECEIVING_DOMAIN|switch"
    "$integration|MAIL_TASK_GATEWAY_SERVICE_TOKEN|MAIL_TASK_GATEWAY_SERVICE_TOKEN|switch"
    "$integration|MAIL_TASK_USER_CORE_SERVICE_TOKEN|USER_CORE_MAIL_TASK_WORKER_TOKEN|minted"
    "$gateway|MAIL_TASK_INGEST_SERVICE_TOKEN|MAIL_TASK_GATEWAY_SERVICE_TOKEN|switch"
    "$gateway|MAIL_TASK_GATEWAY_BRIDGE_SERVICE_API_KEY|MAIL_TASK_GATEWAY_BRIDGE_SERVICE_API_KEY|minted"
    "$model_gateway|MAIL_TASK_BRIDGE_SERVICE_API_KEY|MAIL_TASK_BRIDGE_SERVICE_API_KEY|minted"
    "$user_core|MAIL_TASK_RECEIVING_DOMAIN|MAIL_TASK_RECEIVING_DOMAIN|switch"
  )
  local last_container=''
  for row in "${rows[@]}"; do
    IFS='|' read -r container variable offline_name label <<<"$row"
    if [[ "$container" != "$last_container" ]]; then
      last_container=$container
      if container_running "$container"; then
        printf '  %s: running\n' "$container"
      else
        printf '  %s: not running\n' "$container"
      fi
    fi
    if ! container_running "$container"; then
      if (( present_switches > 0 )); then
        add_problem "$container is not running, so its $variable cannot be confirmed"
      fi
      continue
    fi
    live_check "$container" "$variable" "$offline_name"
    printf '    %-8s %-42s live %-10s agrees with offline copy: %s\n' \
      "$label" "$variable" "$live_state" "$live_match"
    if [[ "$live_state" == present && "$label" == switch ]]; then
      live_switch_set=1
    fi
    if [[ "$live_match" == no ]]; then
      add_problem "$container $variable differs from the $offline_name this check read (a stale container, or two copies that were minted separately)"
    fi
    if [[ "$live_state" == present && -n "$offline_name" ]] && ! binding_value "$offline_name" >/dev/null; then
      add_problem "$container has $variable set but $offline_name is absent here (a different environment than the one that started the container)"
    fi
    if (( present_switches > 0 )) && [[ "$live_state" != present ]] && [[ -n "$offline_name" ]] && binding_value "$offline_name" >/dev/null; then
      add_problem "$offline_name is set here but $container does not have $variable (not applied: recreate $container)"
    fi
  done

  # The two ends of the ingest HMAC key must be the same secret.
  local ingest_a ingest_b
  if container_running "$integration" && container_running "$gateway"; then
    ingest_a=$(live_digest "$integration" MAIL_TASK_GATEWAY_SERVICE_TOKEN)
    ingest_b=$(live_digest "$gateway" MAIL_TASK_INGEST_SERVICE_TOKEN)
    if [[ -n "$ingest_a" && -n "$ingest_b" ]]; then
      if [[ "$ingest_a" == "$ingest_b" ]]; then
        printf '    ingest key: integration-api and the gateway BFF hold the same secret: yes\n'
      else
        printf '    ingest key: integration-api and the gateway BFF hold the same secret: no\n'
        add_problem 'the ingest HMAC key differs between integration-api and the gateway BFF'
      fi
    elif [[ -n "$ingest_a" || -n "$ingest_b" ]]; then
      printf '    ingest key: integration-api and the gateway BFF hold the same secret: half-set\n'
      add_problem 'the ingest HMAC key is set on only one of integration-api and the gateway BFF'
    else
      printf '    ingest key: integration-api and the gateway BFF hold the same secret: neither set\n'
    fi
  fi

  # The two Auth Core principals, as the running auth-service loaded them.
  if container_running "$auth"; then
    local bridge_digest='' gateway_digest='' answer name registered match
    if offline=$(binding_value MAIL_TASK_BRIDGE_SERVICE_API_KEY); then
      bridge_digest=$(printf '%s' "$offline" | sha256_of)
    fi
    if offline=$(binding_value MAIL_TASK_GATEWAY_BRIDGE_SERVICE_API_KEY); then
      gateway_digest=$(printf '%s' "$offline" | sha256_of)
    fi
    answer=$(MSYS_NO_PATHCONV=1 docker exec "$auth" node -e '
      const crypto = require("crypto");
      let registry = {};
      try { registry = JSON.parse(process.env.PLANE_SERVICE_PRINCIPALS_JSON || "{}"); } catch (error) { registry = {}; }
      const digestOrEmpty = (value) => (value === "none" ? "" : value || "");
      const wanted = [
        ["model-gateway-mail-task-bridge", digestOrEmpty(process.argv[1])],
        ["verevon-gateway", digestOrEmpty(process.argv[2])],
      ];
      for (const [name, expected] of wanted) {
        const entry = registry[name];
        const registered = entry !== undefined;
        let match = "n/a";
        if (registered && expected) {
          const actual = crypto.createHash("sha256").update(String(entry.credential || "")).digest("hex");
          match = actual === expected ? "yes" : "no";
        }
        console.log(name + " " + (registered ? "registered" : "missing") + " " + match);
      }
    ' "${bridge_digest:-none}" "${gateway_digest:-none}" 2>/dev/null || true)
    if [[ -z "$answer" ]]; then
      printf '  %s: could not read the principal registry\n' "$auth"
      add_problem "could not read the principal registry inside $auth"
    else
      while read -r name registered match; do
        [[ -n "$name" ]] || continue
        printf '    auth principal %-34s %-10s credential matches the store: %s\n' \
          "$name" "$registered" "$match"
        if [[ "$registered" == missing ]]; then
          if (( present_switches > 0 )); then
            add_problem "auth-service has not registered the principal $name (run scripts/run-control-plane.sh, then recreate $auth)"
          else
            add_note "auth-service has not registered the principal $name yet (expected until the next launcher run and auth-service recreate)"
          fi
        elif [[ "$match" == no ]]; then
          add_problem "the credential auth-service holds for $name differs from the one in the store"
        fi
      done <<<"$answer"
    fi
  else
    printf '  %s: not running\n' "$auth"
    if (( present_switches > 0 )); then
      add_problem "$auth is not running, so the principal registry cannot be confirmed"
    fi
  fi

  # A switch that is live in a container while this check sees none is drift,
  # reported once more in plain words when nothing else already said so.
  if (( live_switch_set == 1 && present_switches == 0 )); then
    add_note 'a switch binding is set inside a running container although none is set here'
  fi
}

if (( live )); then
  live_section
fi

# --- verdict ---------------------------------------------------------------

printf '\n'
for note in "${notes[@]+"${notes[@]}"}"; do
  printf 'note: %s\n' "$note"
done
if (( ${#problems[@]} > 0 )); then
  for problem in "${problems[@]}"; do
    printf 'problem: %s\n' "$problem"
  done
  printf 'result: half-set or invalid (%d problem(s)); exit 1\n' "${#problems[@]}"
  exit 1
fi
if (( present_switches == 0 )); then
  if (( present_minted == ${#minted_names[@]} )); then
    printf 'result: feature off (no switch is set; the minted credentials are present and unused)\n'
  else
    printf 'result: feature off (no switch is set; the minted credentials are not minted yet)\n'
  fi
else
  printf 'result: ready (every switch and every minted binding is present and valid)\n'
fi
if (( live_unreachable )); then
  printf 'live check could not run; exit 3\n'
  exit 3
fi
exit 0
