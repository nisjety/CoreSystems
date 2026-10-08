#!/bin/sh
# export-provider-posture.sh -- print the provider posture of a running stack
# WITHOUT printing any secret.
#
# What it prints, per source (a container or an env file):
#   * the NAME of every provider-related variable and whether it is set or empty;
#   * the VALUE only for an explicit allow-list (see below), and only when the
#     value has the shape that variable is supposed to have;
#   * for endpoint-style variables, the HOSTNAME only (scheme, credentials, port,
#     path and query are dropped).
# It never prints an API key, token, password, connection string or full URL.
#
# Value allow-list (engineering evidence, not legal advice):
#   Core (from the J10 spec):
#     *_REGION  *_DEPLOYMENT_TYPE  *_RESIDENCY  MODEL_PLANE_ALLOW_*
#   Host only:
#     *_ENDPOINT  *_API_BASE  *_BASE_URL  COHERE_URL
#   EXTENSION (not in the spec; added because they decide which provider serves
#   which data, and they hold no secret): the exact names in SELECTOR_NAMES below.
#   Each is shape-checked, so a pasted secret in one of them is withheld.
#
# Sources:
#   --env-file FILE     KEY=VALUE lines (a .env, or a saved `docker inspect` env).
#                       Repeatable. Read, filtered, never echoed.
#   --container NAME    a running container; its environment is read with
#                       `docker inspect` (read-only, no exec). Repeatable.
#   --known-containers  the containers of this stack that carry provider
#                       settings (list in KNOWN_CONTAINERS below).
#
# Exit status: 0 when at least one source was read, 2 on a usage error, 1 when
# every source failed.

set -eu

KNOWN_CONTAINERS="model-plane-inference-core-1 model-plane-model-gateway-1 model-plane-execution-core-1 model-plane-session-core-1 model-plane-agent-memory-server-1 model-plane-letta-bridge-1 integration-api data-plane-v2-embedding-engine-1 data-plane-v2-retrieval-engine-1 data-plane-v2-graph-index-1 quarry-edge notification-core billing-core-service auth-service verevon-gateway-rs"

usage() {
  cat <<'USAGE' >&2
usage: export-provider-posture.sh [--env-file FILE]... [--container NAME]... [--known-containers]

Prints provider variable NAMES, set/empty state, and allow-listed values only
(regions, deployment types, residency, MODEL_PLANE_ALLOW_* flags, a few routing
selectors) plus hostnames for endpoint variables. Never prints a key, token or
URL with credentials.
USAGE
}

AWK_FILE=$(mktemp "${TMPDIR:-/tmp}/provider-posture.XXXXXX")
trap 'rm -f "$AWK_FILE"' EXIT HUP INT TERM

cat > "$AWK_FILE" <<'AWK'
function trim(s) {
  sub(/^[ \t]+/, "", s)
  sub(/[ \t]+$/, "", s)
  return s
}

function unquote(s,   first, last) {
  s = trim(s)
  if (length(s) >= 2) {
    first = substr(s, 1, 1)
    last = substr(s, length(s), 1)
    if ((first == "\"" && last == "\"") || (first == "'" && last == "'")) {
      s = substr(s, 2, length(s) - 2)
    }
  }
  return s
}

function starts_with(s, prefix) {
  return index(s, prefix) == 1
}

function ends_with(s, suffix,   n, m) {
  n = length(s)
  m = length(suffix)
  return n >= m && substr(s, n - m + 1) == suffix
}

# Is this variable provider-related at all? Only these names are listed.
function is_provider_name(name,   i) {
  for (i = 1; i <= n_prefixes; i++) {
    if (starts_with(name, prefixes[i])) return 1
  }
  return (name in exact_names)
}

# Shape checks. Intervals are avoided on purpose: older awks lack them.
# They are a second line of defence for the day someone pastes a credential into a
# variable that is allow-listed by NAME: a value must look like what the variable
# is supposed to hold, or it is withheld.
function count_digits(v,   i, n) {
  n = 0
  for (i = 1; i <= length(v); i++) {
    if (substr(v, i, 1) ~ /[0-9]/) n++
  }
  return n
}

function longest_alnum_run(v,   i, run, best) {
  run = 0
  best = 0
  for (i = 1; i <= length(v); i++) {
    if (substr(v, i, 1) ~ /[A-Za-z0-9]/) {
      run++
      if (run > best) best = run
    } else {
      run = 0
    }
  }
  return best
}

# A region, deployment type or residency label: swedencentral, Sweden Central,
# eastus2, DataZoneStandard, eu, norway. Letters first, few digits, short.
function region_token_ok(v) {
  return length(v) > 0 && length(v) <= 24 && v ~ "^[A-Za-z][A-Za-z0-9 -]*$" && count_digits(v) <= 3
}

# A routing selector: a provider list, a model or deployment name, a mode.
# No long unbroken alphanumeric run and few digits, which excludes a typical key.
function selector_ok(v) {
  return length(v) > 0 && length(v) <= 96 && v ~ "^[A-Za-z0-9._,/-]+$" && longest_alnum_run(v) <= 23 && count_digits(v) <= 8
}

function bool_ok(v) {
  v = tolower(v)
  return v == "true" || v == "false" || v == "0" || v == "1" || v == "yes" || v == "no" || v == "on" || v == "off"
}

# Hostname of a URL-ish value, or "" when it cannot be derived safely.
# Needs an http(s)/ws(s) scheme: a bare token in an *_ENDPOINT variable is
# never treated as a hostname, because it could be a pasted secret.
function host_of(v,   s, cut, p, ch, auth, at, c, i, chars) {
  s = tolower(v)
  if (!match(s, /^(https|http|wss|ws):\/\//)) return ""
  s = substr(s, RLENGTH + 1)
  cut = length(s) + 1
  chars = "/?#"
  for (i = 1; i <= 3; i++) {
    ch = substr(chars, i, 1)
    p = index(s, ch)
    if (p > 0 && p < cut) cut = p
  }
  auth = substr(s, 1, cut - 1)
  while ((at = index(auth, "@")) > 0) auth = substr(auth, at + 1)
  c = index(auth, ":")
  if (c > 0) auth = substr(auth, 1, c - 1)
  auth = tolower(auth)
  if (auth != "" && length(auth) <= 253 && auth ~ /^[a-z0-9.-]+$/) return auth
  return ""
}

function is_host_class(name) {
  return ends_with(name, "_ENDPOINT") || ends_with(name, "_API_BASE") || ends_with(name, "_BASE_URL") || name == "COHERE_URL"
}

# What to show for a SET variable. Everything not matched here shows "-".
function shown_value(name, value) {
  if (name in selector_names) {
    return selector_ok(value) ? value : "[withheld: unexpected shape]"
  }
  # MODEL_PLANE_ALLOW_* first: MODEL_PLANE_ALLOW_GLOBAL_RESIDENCY also ends in
  # _RESIDENCY, and a flag must be held to the boolean shape, not the token shape.
  if (starts_with(name, "MODEL_PLANE_ALLOW_")) {
    return bool_ok(value) ? tolower(value) : "[withheld: unexpected shape]"
  }
  if (ends_with(name, "_REGION") || ends_with(name, "_DEPLOYMENT_TYPE") || ends_with(name, "_RESIDENCY")) {
    return region_token_ok(value) ? value : "[withheld: unexpected shape]"
  }
  if (is_host_class(name)) {
    h = host_of(value)
    return h != "" ? h : "[withheld: no http(s) host]"
  }
  return "-"
}

BEGIN {
  n_prefixes = split("AZURE_ OPENAI_ ANTHROPIC_ COHERE_ CODEX_ AGENTIC_ BRAVE_ SERPER_ SEARXNG_ BROWSERBASE_ POSTMARK_ MAIL_TASK_ MAILTASK_ NOVU_ RESEND_ TWILIO_ STRIPE_ NEXI_ LAGO_ GOOGLE_ MICROSOFT_ META_ FACEBOOK_ INSTAGRAM_ NANGO_ NOTION_ SLACK_ GITHUB_ DISCORD_ SHOPIFY_ HUBSPOT_ LETTA_ AGENT_MEMORY_ MODEL_PLANE_ INFERENCE_ EMBEDDING_ RERANK_ RERANKER_ DREAMING_ MEMORY_GROUNDING_ QUARRY_ COLQWEN_ CONNECTOR_RUNTIME_ TIKA_", prefixes, " ")
  split("GENERATION_MODEL FAST_MODEL COHERE_URL NOTIFICATION_DELIVERY_MODE VEREVON_INTENT_ENABLED DEFAULT_PROVIDER", ex, " ")
  for (i in ex) exact_names[ex[i]] = 1
  # EXTENSION allow-list: routing selectors that hold no secret.
  split("INFERENCE_PROVIDER_ORDER EMBEDDING_PROVIDER MODEL_PLANE_EMBEDDING_PROVIDER GENERATION_MODEL FAST_MODEL EMBEDDING_MODEL RERANKER_MODEL AGENT_MEMORY_GENERATION_MODEL AGENT_MEMORY_EMBEDDING_MODEL DREAMING_LLM_EXTRACTION DREAMING_LLM_MODEL MEMORY_GROUNDING_ENABLED CODEX_SUBSCRIPTION_TRANSPORT CODEX_APP_SERVER_HARDENED AGENTIC_SUBSCRIPTION_ENABLED AGENTIC_SUBSCRIPTION_ALLOW_EFFECTS QUARRY_EDGE__ZERO_SAAS_SEARCH NOTIFICATION_DELIVERY_MODE VEREVON_INTENT_ENABLED DEFAULT_PROVIDER", sel, " ")
  for (i in sel) selector_names[sel[i]] = 1
  count = 0
}

{
  line = $0
  sub(/\r$/, "", line)
  if (line ~ /^[ \t]*#/ || line ~ /^[ \t]*$/) next
  sub(/^[ \t]*export[ \t]+/, "", line)
  eq = index(line, "=")
  if (eq < 2) next
  name = substr(line, 1, eq - 1)
  if (name !~ /^[A-Z][A-Z0-9_]*$/) next
  value = unquote(substr(line, eq + 1))
  if (!(name in seen)) {
    seen[name] = 1
    names[++count] = name
  }
  values[name] = value
}

END {
  # Insertion sort, so the output is stable and diffable.
  for (i = 2; i <= count; i++) {
    key = names[i]
    j = i - 1
    while (j >= 1 && names[j] > key) {
      names[j + 1] = names[j]
      j--
    }
    names[j + 1] = key
  }
  listed = 0
  for (i = 1; i <= count; i++) {
    name = names[i]
    if (!is_provider_name(name)) continue
    value = values[name]
    if (value == "") {
      printf "%s\tempty\t-\n", name
    } else {
      printf "%s\tset\t%s\n", name, shown_value(name, value)
    }
    listed++
  }
  printf "# %d provider variable(s) listed from %d variable(s) read\n", listed, count
}
AWK

emit_header() {
  printf '# export-provider-posture: variable NAMES, set/empty, allow-listed values and endpoint hostnames only\n'
  printf '# values for every other variable are never printed; read at %s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  printf '# columns: NAME<TAB>state<TAB>shown (shown is "-" unless allow-listed)\n'
}

read_ok=0
read_failed=0

filter_stream() {
  awk -f "$AWK_FILE"
}

source_env_file() {
  file="$1"
  printf '## env-file %s\n' "$(basename "$file")"
  if [ ! -r "$file" ]; then
    printf '# [unreadable]\n'
    read_failed=$((read_failed + 1))
    return 0
  fi
  filter_stream < "$file"
  read_ok=$((read_ok + 1))
}

source_container() {
  name="$1"
  printf '## container %s\n' "$name"
  if ! command -v docker >/dev/null 2>&1; then
    printf '# [docker not available]\n'
    read_failed=$((read_failed + 1))
    return 0
  fi
  if env_dump=$(docker inspect --format '{{range .Config.Env}}{{println .}}{{end}}' "$name" 2>/dev/null); then
    printf '%s\n' "$env_dump" | filter_stream
    read_ok=$((read_ok + 1))
  else
    printf '# [not inspectable: not running or no such container]\n'
    read_failed=$((read_failed + 1))
  fi
  env_dump=""
}

if [ "$#" -eq 0 ]; then
  usage
  exit 2
fi

emit_header

while [ "$#" -gt 0 ]; do
  case "$1" in
    --env-file)
      [ "$#" -ge 2 ] || { usage; exit 2; }
      source_env_file "$2"
      shift 2
      ;;
    --container)
      [ "$#" -ge 2 ] || { usage; exit 2; }
      source_container "$2"
      shift 2
      ;;
    --known-containers)
      for known in $KNOWN_CONTAINERS; do
        source_container "$known"
      done
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      usage
      exit 2
      ;;
  esac
done

if [ "$read_ok" -eq 0 ]; then
  exit 1
fi
exit 0
