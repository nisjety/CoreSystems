#!/usr/bin/env bash
# Parity of the three representations of the Auth Core service principal registry
# (plan W0-3):
#
#   1. the committed policy        apps/Control Plane/config/plane-service-principals.json
#   2. the launcher's registry     apps/Control Plane/scripts/run-control-plane.sh
#                                  (plane_principals_registry -> PLANE_SERVICE_PRINCIPALS_JSON)
#   3. the bootstrap's registry    scripts/bootstrap_runtime_environment.sh
#                                  (it used to write a third, INLINE registry that had
#                                  drifted: 14 principals, older scopes, no tokenExchange,
#                                  and the launcher then adopted its stale credentials)
#
# Nothing here reads a live env file or prints a credential. The REAL launcher and the
# REAL bootstrap run in hermetic sandboxes (a stub `docker`, throwaway env files), and the
# registries they write are compared with the committed policy:
#
#   * the same principals, and the same policy (audiences, scopes per audience, retention,
#     organization binding, tokenExchange), in every representation and both profiles;
#   * no explicit null anywhere (Auth Core parses the whole registry per request and
#     rejects a null, so one null 503s every mint while /health stays green);
#   * every principal has its own credential, equal to the variable its presenting end
#     reads (`consumerEnv`), so the registry accepts exactly what the fleet presents;
#   * the bootstrap never rotates a credential it can already see, converges a stale
#     inline registry to the committed policy, and is idempotent;
#   * a principal added to the policy is picked up with no change to either script.
#
# A missing tool or file is a failure, never a skip. On Windows (MSYS) this takes a few
# minutes because the bootstrap forks hundreds of processes; on Linux or macOS it takes
# seconds.
#
#   scripts/tests/principal-registry-parity.test.sh
set -Eeuo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# jq on Windows is a native binary: it cannot open /c/Users/... style paths. Every path
# this script uses is therefore the form both bash and jq can open (C:/... on Windows,
# unchanged elsewhere).
native() { if command -v cygpath >/dev/null 2>&1; then cygpath -m "$1"; else printf '%s' "$1"; fi; }

ROOT="$(native "${PRINCIPAL_PARITY_ROOT:-$(cd "$HERE/../.." && pwd)}")"
CONTROL="$ROOT/apps/Control Plane"
POLICY="$CONTROL/config/plane-service-principals.json"
LAUNCHER="$CONTROL/scripts/run-control-plane.sh"
FILTER="$CONTROL/scripts/plane-principal-registry.jq"
BOOTSTRAP="$ROOT/scripts/bootstrap_runtime_environment.sh"

passes=0
failures=0
pass() { printf 'ok - %s\n' "$1"; passes=$((passes + 1)); }
fail() { printf 'not ok - %s\n' "$1" >&2; failures=$((failures + 1)); }

for tool in jq openssl awk mktemp sha256sum; do
  command -v "$tool" >/dev/null 2>&1 || { printf 'not ok - required tool is missing: %s\n' "$tool" >&2; exit 1; }
done
for file in "$POLICY" "$LAUNCHER" "$BOOTSTRAP"; do
  [[ -f "$file" ]] || { printf 'not ok - required file is missing: %s\n' "$file" >&2; exit 1; }
done

jqr() { jq "$@" | tr -d '\015'; }
# PATH entries are colon separated, so a stub directory must be given in the form bash
# itself uses (/c/Users/... on Windows), not the C:/... form handed to native tools.
posix() { if command -v cygpath >/dev/null 2>&1; then cygpath -u "$1"; else printf '%s' "$1"; fi; }

WORK="$(native "$(mktemp -d)")"
trap 'rm -rf "$WORK"' EXIT

# ---- helpers over registries and the policy ---------------------------------

# The policy-relevant view of a registry or of the policy file: credentials, consumerEnv
# and any other bookkeeping dropped, absent keys absent (a null would not survive a
# comparison with a row that simply omits the key).
POLICY_VIEW='
  with_entries(.value |= (
    {audiences, orgIds, scopes, scopesByAudience, retentionByAudience, tokenExchange, allowAnyOrg, tenancy}
    | with_entries(select(.value != null))))'

# What the development profile must make of the policy: `tenancy: "directory"` rows keep
# running on the legacy allowAnyOrg the stack has always used.
DEVELOPMENT_VIEW="$POLICY_VIEW | with_entries(.value |= (if .tenancy == \"directory\" then (del(.tenancy) | .allowAnyOrg = true) else . end))"

view_of() { jqr -S -c "$1" "$2"; }

registry_of_dotenv() {
  local line
  line="$(grep -E '^PLANE_SERVICE_PRINCIPALS_JSON=' "$1" | tail -n 1 || true)"
  if [[ -z "$line" ]]; then printf '{}'; else printf '%s' "${line#PLANE_SERVICE_PRINCIPALS_JSON=}"; fi
}

# value of VAR across the given dotenv files (first definition wins); fails if none has it.
dotenv_value() {
  local var="$1" file line
  shift
  for file in "$@"; do
    [[ -f "$file" ]] || continue
    line="$(grep -E "^${var}=" "$file" | tail -n 1 || true)"
    if [[ -n "$line" && -n "${line#*=}" ]]; then printf '%s' "${line#*=}"; return 0; fi
  done
  return 1
}

assert_registry() { # label registry_file expected_view ledger_file...
  local label="$1" registry_file="$2" expected_view="$3"
  shift 3
  local policy_names registry_names count distinct name consumer expected presented mismatched=""

  policy_names="$(jqr -r 'keys[]' "$POLICY" | sort | tr '\n' ' ')"
  registry_names="$(jqr -r 'keys[]' "$registry_file" | sort | tr '\n' ' ')"
  if [[ "$policy_names" == "$registry_names" ]]; then
    pass "$label: the same principals as the committed policy"
  else
    fail "$label: principals differ. policy: $policy_names / registry: $registry_names"
  fi

  if [[ "$(view_of "$POLICY_VIEW" "$registry_file")" == "$expected_view" ]]; then
    pass "$label: audiences, scopes, retention, organization binding and tokenExchange equal the policy"
  else
    fail "$label: policy content differs from the committed policy"
    diff <(printf '%s\n' "$expected_view" | jq -S .) <(view_of "$POLICY_VIEW" "$registry_file" | jq -S .) | head -40 >&2 || true
  fi

  if [[ "$(jqr '[.. | nulls] | length' "$registry_file")" == "0" ]]; then
    pass "$label: no explicit null anywhere"
  else
    fail "$label: the registry carries an explicit null (Auth Core rejects the whole registry)"
  fi

  count="$(jqr 'length' "$registry_file")"
  distinct="$(jqr '[.[].credential] | unique | length' "$registry_file")"
  if [[ "$count" == "$distinct" ]] && jq -e 'all(.[]; (.credential | type) == "string" and (.credential | length) >= 16)' "$registry_file" >/dev/null; then
    pass "$label: $count principals, each with its own credential of 16+ characters"
  else
    fail "$label: credentials are missing, short or shared ($count principals, $distinct distinct)"
  fi

  while IFS= read -r name; do
    consumer="$(jqr -r --arg n "$name" '.[$n].consumerEnv // empty' "$POLICY")"
    expected="$(jqr -r --arg n "$name" '.[$n].credential // empty' "$registry_file")"
    presented="$(dotenv_value "$consumer" "$@" || true)"
    if [[ -z "$consumer" || -z "$presented" || "$presented" != "$expected" ]]; then
      mismatched+=" $name"
    fi
  done < <(jqr -r 'keys[]' "$POLICY")
  if [[ -z "$mismatched" ]]; then
    pass "$label: every credential equals the variable its presenting end reads (consumerEnv)"
  else
    fail "$label: credential differs from, or is missing at, the presenting end for:$mismatched"
  fi
}

# ---- 0. static: no second definition of the policy --------------------------

if [[ -f "$FILTER" ]]; then
  pass "the shared assembler exists: scripts/plane-principal-registry.jq"
else
  fail "the shared assembler is missing: $FILTER"
fi
if grep -q 'plane-principal-registry\.jq' "$LAUNCHER"; then
  pass "the launcher assembles through the shared assembler"
else
  fail "the launcher does not use the shared assembler"
fi
if grep -q 'plane-principal-registry\.jq' "$BOOTSTRAP" && grep -q 'plane-service-principals\.json' "$BOOTSTRAP"; then
  pass "the bootstrap reads the committed policy and assembles through the shared assembler"
else
  fail "the bootstrap does not read the committed policy through the shared assembler"
fi
if grep -Eq '^[[:space:]]*"(retrieval-engine|embedding-engine|graph-index|quarry-edge|capability-core)"[[:space:]]*:[[:space:]]*\{' "$BOOTSTRAP" \
  || grep -Eq 'scopesByAudience:|retentionByAudience:' "$BOOTSTRAP"; then
  fail "the bootstrap still carries an inline principal registry"
else
  pass "the bootstrap carries no inline principal registry"
fi

# ---- 1. the shared assembler on its own -------------------------------------

if [[ -f "$FILTER" ]]; then
  fake_creds="$(jqr -c 'with_entries(.value = "parity-credential-0123456789abcdef-" + .key)' "$POLICY")"
  assemble() { # profile policy
    jqr -c -n --slurpfile pol "$2" --argjson creds "$fake_creds" --arg profile "$1" -f "$FILTER"
  }
  if assemble production "$POLICY" >"$WORK/assembled-production.json" 2>"$WORK/assembled-production.err" \
    && [[ "$(view_of "$POLICY_VIEW" "$WORK/assembled-production.json")" == "$(view_of "$POLICY_VIEW" "$POLICY")" ]]; then
    pass "assembler, production profile: emits the policy as stated"
  else
    fail "assembler, production profile: output differs from the policy ($(head -c 200 "$WORK/assembled-production.err"))"
  fi
  if [[ "$(jqr '[.[] | select(.allowAnyOrg == true)] | length' "$WORK/assembled-production.json" 2>/dev/null || echo x)" == "0" ]]; then
    pass "assembler, production profile: no principal carries allowAnyOrg: true"
  else
    fail "assembler, production profile: a principal still carries allowAnyOrg: true"
  fi
  if assemble development "$POLICY" >"$WORK/assembled-development.json" 2>/dev/null \
    && [[ "$(view_of "$POLICY_VIEW" "$WORK/assembled-development.json")" == "$(view_of "$DEVELOPMENT_VIEW" "$POLICY")" ]]; then
    pass "assembler, development profile: directory rows keep running on allowAnyOrg, everything else equal"
  else
    fail "assembler, development profile: output differs from the expected development view"
  fi

  jqr '.["quarry-edge"] |= (del(.tenancy) | .allowAnyOrg = true)' "$POLICY" >"$WORK/policy-unchecked.json"
  if assemble production "$WORK/policy-unchecked.json" >/dev/null 2>&1; then
    fail "assembler, production profile: accepted a row carrying allowAnyOrg: true"
  else
    pass "assembler, production profile: refuses a row carrying allowAnyOrg: true"
  fi
  jqr '.["quarry-edge"] |= (del(.tenancy) | .orgIds = [])' "$POLICY" >"$WORK/policy-unbound.json"
  if assemble development "$WORK/policy-unbound.json" >/dev/null 2>&1; then
    fail "assembler: accepted a principal that names no organization source"
  else
    pass "assembler: refuses a principal that names no organization source"
  fi
  if jqr -c -n --slurpfile pol "$POLICY" --argjson creds '{}' --arg profile development -f "$FILTER" >/dev/null 2>&1; then
    fail "assembler: emitted a registry with no credentials"
  else
    pass "assembler: refuses to emit a principal without a credential"
  fi
fi

# ---- 2. the launcher, run for real in a sandbox -----------------------------

run_launcher() { # profile dir
  local profile="$1" dir="$2" sandbox="$2/sandbox" svc file
  mkdir -p "$sandbox/apps/Control Plane/scripts" "$sandbox/apps/Control Plane/config" "$dir/bin"
  cp "$LAUNCHER" "$sandbox/apps/Control Plane/scripts/"
  if [[ -f "$FILTER" ]]; then cp "$FILTER" "$sandbox/apps/Control Plane/scripts/"; fi
  cp "$POLICY" "$sandbox/apps/Control Plane/config/"
  for svc in auth-core user-core org-core audit-core billing-core session-core; do
    mkdir -p "$sandbox/apps/Control Plane/$svc"
  done
  for file in auth-core/.env auth-core/.env.docker user-core/.env user-core/.env.docker org-core/.env \
    org-core/.env.docker audit-core/.env billing-core/.env billing-core/.env.docker session-core/.env \
    session-core/.env.docker; do
    : >"$sandbox/apps/Control Plane/$file"
  done
  printf '#!/usr/bin/env bash\nexit 0\n' >"$dir/bin/docker"
  chmod +x "$dir/bin/docker"
  PATH="$(posix "$dir/bin"):$PATH" CONTROL_PLANE_SECRETS_FILE="$dir/secrets" \
    PLANE_SERVICE_PRINCIPALS_PROFILE="$profile" \
    bash "$sandbox/apps/Control Plane/scripts/run-control-plane.sh" config >"$dir/launcher.out" 2>&1
}

mkdir -p "$WORK/launcher-development" "$WORK/launcher-production"
if run_launcher development "$WORK/launcher-development"; then
  registry_of_dotenv "$WORK/launcher-development/secrets" >"$WORK/launcher-development.json"
  assert_registry "launcher (development)" "$WORK/launcher-development.json" "$(view_of "$DEVELOPMENT_VIEW" "$POLICY")" "$WORK/launcher-development/secrets"
  # Re-running must not rotate a credential the running fleet presents.
  first="$(jqr -S -c . "$WORK/launcher-development.json")"
  PATH="$(posix "$WORK/launcher-development/bin"):$PATH" CONTROL_PLANE_SECRETS_FILE="$WORK/launcher-development/secrets" \
    bash "$WORK/launcher-development/sandbox/apps/Control Plane/scripts/run-control-plane.sh" config >/dev/null 2>&1 || true
  registry_of_dotenv "$WORK/launcher-development/secrets" >"$WORK/launcher-development-again.json"
  second="$(jqr -S -c . "$WORK/launcher-development-again.json")"
  if [[ "$first" == "$second" ]]; then pass "launcher (development): a second run changes nothing"; else fail "launcher (development): a second run changed the registry"; fi
else
  fail "launcher (development): the real launcher failed in its sandbox: $(tail -n 5 "$WORK/launcher-development/launcher.out" | tr '\n' ' ' | cut -c1-300)"
fi
if run_launcher production "$WORK/launcher-production"; then
  registry_of_dotenv "$WORK/launcher-production/secrets" >"$WORK/launcher-production.json"
  assert_registry "launcher (production profile)" "$WORK/launcher-production.json" "$(view_of "$POLICY_VIEW" "$POLICY")" "$WORK/launcher-production/secrets"
else
  fail "launcher (production profile): the real launcher failed in its sandbox: $(tail -n 5 "$WORK/launcher-production/launcher.out" | tr '\n' ' ' | cut -c1-300)"
fi

# The live regression the launcher fix closes. lookup_value takes the FIRST non-empty hit
# across every env file, and the plane's own .env comes before the launcher's store, so a
# stale registry the central bootstrap once wrote into that .env shadowed the store: its
# credentials were adopted and written back as the live registry. The registry an earlier
# run assembled must be read from the store alone. Nothing above exercises this (each of
# those runs starts from an empty store and an empty .env), so reverting the fix passed.
shadow="$WORK/launcher-shadow"
mkdir -p "$shadow"
if run_launcher development "$shadow"; then
  registry_of_dotenv "$shadow/secrets" >"$shadow/before.json"
  jqr -c 'with_entries(.value.credential = "stale-shadow-registry-credential-0123456789-" + .key | del(.value.tokenExchange))' \
    "$shadow/before.json" >"$shadow/stale.json"
  printf 'PLANE_SERVICE_PRINCIPALS_JSON=%s\n' "$(cat "$shadow/stale.json")" >"$shadow/sandbox/apps/Control Plane/.env"
  PATH="$(posix "$shadow/bin"):$PATH" CONTROL_PLANE_SECRETS_FILE="$shadow/secrets" \
    PLANE_SERVICE_PRINCIPALS_PROFILE=development \
    bash "$shadow/sandbox/apps/Control Plane/scripts/run-control-plane.sh" config >"$shadow/launcher-again.out" 2>&1 || true
  registry_of_dotenv "$shadow/secrets" >"$shadow/after.json"
  if [[ "$(jqr -S -c . "$shadow/before.json")" == "$(jqr -S -c . "$shadow/after.json")" ]] \
    && ! grep -q 'stale-shadow-registry-credential' "$shadow/secrets"; then
    pass "launcher: a stale registry in the plane's own .env neither shadows the store nor leaks its credentials into it"
  else
    fail "launcher: a stale registry in the plane's own .env changed the registry the store holds"
  fi
else
  fail "launcher (shadowing): the real launcher failed in its sandbox: $(tail -n 5 "$shadow/launcher.out" | tr '\n' ' ' | cut -c1-300)"
fi

# ---- 3. the bootstrap, run for real in three ledgers ------------------------

LEDGER_RELATIVE=(
  '.env'
  'apps/Data Plane v2/.env'
  'apps/Control Plane/.env'
  'apps/Ingestion Plane/.env'
  'apps/Model Plane/deploy/.env'
  'apps/Application Plane/.env'
  'apps/Frontend Plane/verevonv3/.env'
)

make_ledger() { # dir
  local rel
  for rel in "${LEDGER_RELATIVE[@]}"; do
    mkdir -p "$1/$(dirname "$rel")"
    printf '# fixture\n' >"$1/$rel"
  done
}

# Prints the ledger's files as NUL-free lines for the caller to read into an array.
ledger_paths() { # dir
  local rel
  for rel in "${LEDGER_RELATIVE[@]}"; do printf '%s\n' "$1/$rel"; done
}

ledger_snapshot() { # dir -> a digest of every value in the ledger
  (cd "$1" && find . -type f -name '.env' | sort | while IFS= read -r f; do printf '%s\n' "$f"; cat "$f"; done | sha256sum | cut -d' ' -f1)
}

# A: a fresh ledger.
make_ledger "$WORK/ledger-fresh"
# B: a ledger that already holds a STALE inline registry and some consumer variables, as a
# live machine does. The stale rows carry an older policy and credentials that must survive.
make_ledger "$WORK/ledger-stale"
STALE_RETRIEVAL_VAR="ledger-retrieval-credential-0123456789abcdef"
STALE_GATEWAY_VAR="ledger-model-gateway-credential-0123456789abcdef"
STALE_SEEDER_CREDENTIAL="stale-registry-corpus-seeder-0123456789abcdef"
printf 'CONTROL_POLICY_SERVICE_API_KEY=%s\n' "$STALE_RETRIEVAL_VAR" >>"$WORK/ledger-stale/apps/Control Plane/.env"
printf 'MODEL_GATEWAY_SERVICE_API_KEY=%s\n' "$STALE_GATEWAY_VAR" >>"$WORK/ledger-stale/apps/Model Plane/deploy/.env"
stale_registry="$(jq -c -n --arg r stale-retrieval-registry-0123456789abcdef --arg g stale-gateway-registry-0123456789abcdef --arg s "$STALE_SEEDER_CREDENTIAL" '{
  "retrieval-engine": {credential: $r, audiences: ["control-policy"], orgIds: [], allowAnyOrg: true, scopes: ["data:authorization:decide"], scopesByAudience: {"control-policy": ["data:authorization:decide"]}, retentionByAudience: {"control-policy": "zdr"}},
  "model-gateway": {credential: $g, audiences: ["session-core"], orgIds: [], allowAnyOrg: true, scopes: ["session:heartbeat"], scopesByAudience: {"session-core": ["session:heartbeat"]}, retentionByAudience: {"session-core": "zdr"}},
  "corpus-seeder": {credential: $s, audiences: ["data-plane"], orgIds: ["org-corpus-baseline"], allowAnyOrg: false, scopes: ["data:read"], scopesByAudience: {"data-plane": ["data:read"]}, retentionByAudience: {"data-plane": "persistent"}}
}' | tr -d '\015')"
printf 'PLANE_SERVICE_PRINCIPALS_JSON=%s\n' "$stale_registry" >>"$WORK/ledger-stale/apps/Control Plane/.env"
# C: a fresh ledger and a policy with one more principal than the committed one.
make_ledger "$WORK/ledger-extra"
jqr '. + {"parity-extra-worker": {"audiences": ["data-plane"], "consumerEnv": "PARITY_EXTRA_WORKER_API_KEY", "orgIds": [], "retentionByAudience": {"data-plane": "persistent"}, "scopes": ["documents:write"], "scopesByAudience": {"data-plane": ["documents:write"]}, "tenancy": "directory"}}' "$POLICY" >"$WORK/policy-extra.json"
# The launcher's own store is READ (never written) for a principal whose credential no ledger
# holds: that is where a principal with no home plane (corpus-seeder, the mail-task pair)
# already lives on a machine the launcher has run on. Minting a second one here would give
# the bootstrap's registry and the store the fleet reads two different credentials for one
# principal. The extra worker's credential is therefore planted in that store, and only there.
EXTRA_STORE_CREDENTIAL="launcher-store-extra-worker-0123456789abcdef"
printf 'PARITY_EXTRA_WORKER_API_KEY=%s\n' "$EXTRA_STORE_CREDENTIAL" >"$WORK/ledger-extra/apps/Control Plane/.env.generated-secrets"

run_bootstrap() { # ledger_dir [policy_override]
  if [[ -n "${2:-}" ]]; then
    CORE_ROOT_OVERRIDE="$1" PLANE_SERVICE_PRINCIPALS_POLICY_FILE="$2" bash "$BOOTSTRAP" >"$1.out" 2>&1
  else
    CORE_ROOT_OVERRIDE="$1" bash "$BOOTSTRAP" >"$1.out" 2>&1
  fi
}

# The three runs are independent and CPU-bound: run them side by side.
run_bootstrap "$WORK/ledger-fresh" & pid_fresh=$!
run_bootstrap "$WORK/ledger-stale" & pid_stale=$!
run_bootstrap "$WORK/ledger-extra" "$WORK/policy-extra.json" & pid_extra=$!
status_fresh=0; status_stale=0; status_extra=0
wait "$pid_fresh" || status_fresh=$?
wait "$pid_stale" || status_stale=$?
wait "$pid_extra" || status_extra=$?

report_failed_run() { # scenario status
  if [[ "$2" != "0" ]]; then
    fail "bootstrap ($1 ledger): the real bootstrap failed (exit $2): $(tail -n 4 "$WORK/ledger-$1.out" | tr '\n' ' ' | cut -c1-300)"
  fi
}
report_failed_run fresh "$status_fresh"
report_failed_run stale "$status_stale"
report_failed_run extra "$status_extra"

read_paths() { # dir -> fills the global array LEDGER_PATHS
  LEDGER_PATHS=()
  local f
  while IFS= read -r f; do LEDGER_PATHS+=("$f"); done < <(ledger_paths "$1")
}

if [[ "$status_fresh" == "0" ]]; then
  registry_of_dotenv "$WORK/ledger-fresh/apps/Control Plane/.env" >"$WORK/bootstrap-fresh.json"
  read_paths "$WORK/ledger-fresh"
  assert_registry "bootstrap (fresh ledger)" "$WORK/bootstrap-fresh.json" "$(view_of "$DEVELOPMENT_VIEW" "$POLICY")" "${LEDGER_PATHS[@]}"

  # Idempotent: a second run on the same ledger changes no value.
  before="$(ledger_snapshot "$WORK/ledger-fresh")"
  if run_bootstrap "$WORK/ledger-fresh"; then
    after="$(ledger_snapshot "$WORK/ledger-fresh")"
    if [[ "$before" == "$after" ]]; then
      pass "bootstrap: a second run changes no ledger value (no rotation, registry stable)"
    else
      fail "bootstrap: a second run changed the ledger"
    fi
  else
    fail "bootstrap: the second run failed: $(tail -n 3 "$WORK/ledger-fresh.out" | tr '\n' ' ' | cut -c1-300)"
  fi
fi

if [[ "$status_stale" == "0" ]]; then
  registry_of_dotenv "$WORK/ledger-stale/apps/Control Plane/.env" >"$WORK/bootstrap-stale.json"
  read_paths "$WORK/ledger-stale"
  assert_registry "bootstrap (stale inline registry converged)" "$WORK/bootstrap-stale.json" "$(view_of "$DEVELOPMENT_VIEW" "$POLICY")" "${LEDGER_PATHS[@]}"
  if [[ "$(jqr -r '.["retrieval-engine"].credential' "$WORK/bootstrap-stale.json")" == "$STALE_RETRIEVAL_VAR" \
    && "$(jqr -r '.["model-gateway"].credential' "$WORK/bootstrap-stale.json")" == "$STALE_GATEWAY_VAR" ]]; then
    pass "bootstrap: a credential the ledger already holds is never rotated, and wins over a stale registry's"
  else
    fail "bootstrap: a ledger credential was replaced"
  fi
  if [[ "$(jqr -r '.["corpus-seeder"].credential' "$WORK/bootstrap-stale.json")" == "$STALE_SEEDER_CREDENTIAL" \
    && "$(dotenv_value DATA_PLANE_CORPUS_SEEDER_API_KEY "$WORK/ledger-stale/apps/Control Plane/.env" || true)" == "$STALE_SEEDER_CREDENTIAL" ]]; then
    pass "bootstrap: a credential only the old registry held is adopted and published, not re-minted"
  else
    fail "bootstrap: the old registry's credential for corpus-seeder was not adopted"
  fi
fi

if [[ "$status_extra" == "0" ]]; then
  registry_of_dotenv "$WORK/ledger-extra/apps/Control Plane/.env" >"$WORK/bootstrap-extra.json"
  read_paths "$WORK/ledger-extra"
  extra_credential="$(jqr -r '.["parity-extra-worker"].credential // empty' "$WORK/bootstrap-extra.json")"
  # assert_registry compares against the committed policy file, so this one is checked by hand.
  if [[ "$(view_of "$POLICY_VIEW" "$WORK/bootstrap-extra.json")" == "$(view_of "$DEVELOPMENT_VIEW" "$WORK/policy-extra.json")" \
    && "${#extra_credential}" -ge 16 \
    && "$(dotenv_value PARITY_EXTRA_WORKER_API_KEY "${LEDGER_PATHS[@]}" || true)" == "$extra_credential" ]]; then
    pass "bootstrap: a principal added to the policy is assembled, its credential published to its consumerEnv, and neither script changes"
  else
    fail "bootstrap: a principal added to the policy was not picked up"
  fi
  # ...and the credential is the one the launcher's store already held (no second credential
  # for one principal), with the store itself left exactly as it was.
  if [[ "$extra_credential" == "$EXTRA_STORE_CREDENTIAL" \
    && "$(cat "$WORK/ledger-extra/apps/Control Plane/.env.generated-secrets")" == "PARITY_EXTRA_WORKER_API_KEY=$EXTRA_STORE_CREDENTIAL" ]]; then
    pass "bootstrap: a credential only the launcher's store held is adopted, and the store is read, never written"
  else
    fail "bootstrap: the credential in the launcher's store was not adopted, or the store was changed"
  fi
fi

printf '\n%d passed, %d failed\n' "$passes" "$failures"
[[ "$failures" -eq 0 ]]
