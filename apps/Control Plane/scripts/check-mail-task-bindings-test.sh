#!/usr/bin/env bash
set -euo pipefail

# Contract test for scripts/check-mail-task-bindings.sh (chat gap closure plan
# task J3). Everything runs in a temporary sandbox with fake values and, for
# --live, a fake `docker` that executes the check's REAL in-container probes
# against fixture environments. It never touches the real stack, the real
# secrets store or the network, and it asserts that no fake secret ever appears
# in the check's output.

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
check_script="$root/scripts/check-mail-task-bindings.sh"

sandbox="$(mktemp -d "${TMPDIR:-/tmp}/mail-task-bindings-test.XXXXXX")"
trap 'rm -rf "$sandbox"' EXIT INT TERM
mkdir -p "$sandbox/bin" "$sandbox/containers"

pass=0
fail=0
output=''
status=0

# Distinct fake secrets, each 40+ characters, so a leak is unmistakable.
worker_token='fake-worker-token-0123456789abcdef0123456789abcdef'
bridge_key='fake-bridge-key-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa'
gateway_bridge_key='fake-gateway-bridge-key-bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb'
ingest_token='fake-ingest-token-cccccccccccccccccccccccccccccccccc'
webhook_secret='fake-webhook-secret-dddddddddddddddddddddddddddddd'
other_ingest_token='fake-other-ingest-eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee'
all_secrets=(
  "$worker_token" "$bridge_key" "$gateway_bridge_key" "$ingest_token"
  "$webhook_secret" "$other_ingest_token"
)

# run_check VAR=value... -- args...   Runs the check with ONLY those variables
# (plus PATH and the fake docker directory) and records output and status.
run_check() {
  local assignments=()
  while (( $# > 0 )) && [[ "$1" != "--" ]]; do
    assignments+=("$1")
    shift
  done
  shift
  set +e
  output="$(env -i PATH="$sandbox/bin:$PATH" HOME="$sandbox" \
    FAKE_DOCKER_DIR="$sandbox/containers" \
    ${assignments[@]+"${assignments[@]}"} \
    bash "$check_script" "$@" 2>&1)"
  status=$?
  set -e
}

expect() {
  local description=$1 condition=$2
  if eval "$condition"; then
    pass=$((pass + 1))
    printf 'ok   - %s\n' "$description"
  else
    fail=$((fail + 1))
    printf 'FAIL - %s\n' "$description"
    printf '       exit %s, output:\n' "$status"
    printf '%s\n' "$output" | sed 's/^/       | /'
  fi
}

expect_status() { expect "$1" "[[ \$status -eq $2 ]]"; }
expect_output() { expect "$1" "grep -qF -- $(printf '%q' "$2") <<<\"\$output\""; }
expect_no_secret() {
  local leaked=0 secret
  for secret in "${all_secrets[@]}"; do
    if grep -qF -- "$secret" <<<"$output"; then
      leaked=1
    fi
  done
  expect "$1" "[[ $leaked -eq 0 ]]"
}

write_store() {
  # write_store FILE [name=value ...]
  local file=$1 pair
  shift
  : >"$file"
  for pair in "$@"; do
    printf '%s\n' "$pair" >>"$file"
  done
}

minted_store="$sandbox/minted.store"
write_store "$minted_store" \
  "USER_CORE_MAIL_TASK_WORKER_TOKEN=$worker_token" \
  "MAIL_TASK_BRIDGE_SERVICE_API_KEY=$bridge_key" \
  "MAIL_TASK_GATEWAY_BRIDGE_SERVICE_API_KEY=$gateway_bridge_key"

# 1. Nothing set at all.
run_check -- --no-store
expect_status 'nothing set: exits 0' 0
expect_output 'nothing set: reports feature off' 'feature off'
expect 'nothing set: all nine bindings are absent' '[[ $(grep -c " absent$" <<<"$output") -eq 9 ]]'

run_check -- --store "$sandbox/does-not-exist"
expect_status 'a missing store reads as nothing set: exits 0' 0
expect_output 'a missing store is reported' 'secrets store: not found'
expect_output 'a missing store still reports feature off' 'feature off'

# 2. Launcher minted the credentials, no switch is set: still off, still 0.
run_check -- --store "$minted_store"
expect_status 'minted credentials only: exits 0' 0
expect_output 'minted credentials only: reports feature off' 'feature off'
expect_output 'minted credentials only: says the credentials are unused' 'present and unused'
expect_output 'minted credentials only: reads them from the store' 'present (secrets store)'
expect_no_secret 'minted credentials only: prints no value'

# 3. Minted bindings only partly present: half-set.
partial_store="$sandbox/partial.store"
write_store "$partial_store" \
  "USER_CORE_MAIL_TASK_WORKER_TOKEN=$worker_token" \
  "MAIL_TASK_BRIDGE_SERVICE_API_KEY=$bridge_key"
run_check -- --store "$partial_store"
expect_status 'minted bindings half-present: exits 1' 1
expect_output 'minted bindings half-present: names the missing one' 'MAIL_TASK_GATEWAY_BRIDGE_SERVICE_API_KEY is absent'
expect_no_secret 'minted bindings half-present: prints no value'

# 4. A switch set while the others are absent: half-set.
run_check "MAIL_TASK_PROVIDER=postmark" -- --store "$minted_store"
expect_status 'one switch set: exits 1' 1
expect_output 'one switch set: names the absent webhook secret' 'MAIL_TASK_WEBHOOK_SECRET is absent'
expect_output 'one switch set: names the absent receiving domain' 'MAIL_TASK_RECEIVING_DOMAIN is absent'
expect_output 'one switch set: names the absent ingest token' 'MAIL_TASK_GATEWAY_SERVICE_TOKEN is absent'

run_check "MAIL_TASK_RECEIVING_DOMAIN=tasks.example.test" -- --store "$minted_store"
expect_status 'receiving domain alone: exits 1' 1

# 5. A switch set but nothing minted: half-set, with the launcher named.
run_check "MAIL_TASK_PROVIDER=postmark" "MAIL_TASK_WEBHOOK_SECRET=$webhook_secret" \
  "MAIL_TASK_RECEIVING_DOMAIN=tasks.example.test" \
  "MAIL_TASK_GATEWAY_SERVICE_TOKEN=$ingest_token" -- --no-store
expect_status 'switches set, nothing minted: exits 1' 1
expect_output 'switches set, nothing minted: points at the launcher' 'run scripts/run-control-plane.sh'
expect_no_secret 'switches set, nothing minted: prints no value'

# 6. Everything set and valid.
ready_env=(
  "MAIL_TASK_PROVIDER=postmark"
  "MAIL_TASK_WEBHOOK_SECRET=$webhook_secret"
  "MAIL_TASK_RECEIVING_DOMAIN=tasks.example.test"
  "MAIL_TASK_GATEWAY_SERVICE_TOKEN=$ingest_token"
)
run_check "${ready_env[@]}" -- --store "$minted_store"
expect_status 'everything set: exits 0' 0
expect_output 'everything set: reports ready' 'result: ready'
expect_output 'everything set: shows where a switch came from' 'present (environment)'
expect_no_secret 'everything set: prints no value'

# 7. Shape checks.
run_check "MAIL_TASK_PROVIDER=mailgun" "MAIL_TASK_WEBHOOK_SECRET=$webhook_secret" \
  "MAIL_TASK_RECEIVING_DOMAIN=tasks.example.test" \
  "MAIL_TASK_GATEWAY_SERVICE_TOKEN=$ingest_token" -- --store "$minted_store"
expect_status 'a provider other than postmark: exits 1' 1
expect_output 'a provider other than postmark: says so without the value' 'is not postmark'
expect "a provider other than postmark: does not print it" '! grep -qF mailgun <<<"$output"'

run_check "MAIL_TASK_PROVIDER=postmark" "MAIL_TASK_WEBHOOK_SECRET=$webhook_secret" \
  "MAIL_TASK_RECEIVING_DOMAIN=tasks.example.test" \
  "MAIL_TASK_GATEWAY_SERVICE_TOKEN=too-short" -- --store "$minted_store"
expect_status 'an ingest token under 32 characters: exits 1' 1
expect_output 'an ingest token under 32 characters: names it' 'MAIL_TASK_GATEWAY_SERVICE_TOKEN is shorter than 32 characters'
expect "an ingest token under 32 characters: does not print it" '! grep -qF too-short <<<"$output"'

short_store="$sandbox/short.store"
write_store "$short_store" \
  "USER_CORE_MAIL_TASK_WORKER_TOKEN=short" \
  "MAIL_TASK_BRIDGE_SERVICE_API_KEY=$bridge_key" \
  "MAIL_TASK_GATEWAY_BRIDGE_SERVICE_API_KEY=$gateway_bridge_key"
run_check -- --store "$short_store"
expect_status 'a minted token under 32 characters: exits 1 even with the feature off' 1

run_check "MAIL_TASK_RATE_LIMIT_PER_HOUR=abc" -- --store "$minted_store"
expect_status 'a non-numeric rate limit: exits 1' 1
run_check "MAIL_TASK_RATE_LIMIT_PER_HOUR=0" -- --store "$minted_store"
expect_status 'a zero rate limit: exits 1' 1
run_check "MAIL_TASK_RATE_LIMIT_PER_HOUR=12.5" -- --store "$minted_store"
expect_status 'a valid rate limit with the feature off: exits 0' 0

# 8. Precedence: environment over --env-file over the store.
override_file="$sandbox/override.env"
write_store "$override_file" "MAIL_TASK_PROVIDER=postmark"
run_check -- --store "$minted_store" --env-file "$override_file"
expect_output 'an --env-file binding is read' 'present (env-file)'
expect_status 'a lone --env-file switch is half-set: exits 1' 1

# 9. Usage errors.
run_check -- --bogus
expect_status 'an unknown argument: exits 2' 2
run_check -- --store
expect_status 'a flag without its value: exits 2' 2

# 10. --live with a fake docker that runs the real probes against fixtures.
cat >"$sandbox/bin/docker" <<'FAKE_DOCKER'
#!/usr/bin/env bash
# Fake docker for check-mail-task-bindings-test.sh. A container exists when
# $FAKE_DOCKER_DIR/<name>.env exists; its lines are the container environment.
command_name=${1:-}
shift || true
case "$command_name" in
  version)
    [[ -z "${FAKE_DOCKER_DOWN:-}" ]] || exit 1
    exit 0
    ;;
  inspect)
    name=${*: -1}
    if [[ -f "$FAKE_DOCKER_DIR/$name.env" ]]; then echo true; else echo false; exit 1; fi
    ;;
  exec)
    name=$1
    shift
    [[ -f "$FAKE_DOCKER_DIR/$name.env" ]] || exit 1
    container_env=()
    while IFS= read -r line || [[ -n "$line" ]]; do
      [[ -n "$line" ]] && container_env+=("$line")
    done <"$FAKE_DOCKER_DIR/$name.env"
    exec env -i PATH="$PATH" ${container_env[@]+"${container_env[@]}"} "$@"
    ;;
  *)
    exit 1
    ;;
esac
FAKE_DOCKER
chmod +x "$sandbox/bin/docker"

write_containers_consistent() {
  rm -f "$sandbox"/containers/*.env
  write_store "$sandbox/containers/integration-api.env" \
    "MAIL_TASK_PROVIDER=postmark" \
    "MAIL_TASK_WEBHOOK_SECRET=$webhook_secret" \
    "MAIL_TASK_RECEIVING_DOMAIN=tasks.example.test" \
    "MAIL_TASK_USER_CORE_SERVICE_TOKEN=$worker_token" \
    "MAIL_TASK_GATEWAY_SERVICE_TOKEN=$ingest_token"
  write_store "$sandbox/containers/verevon-gateway-rs.env" \
    "MAIL_TASK_INGEST_SERVICE_TOKEN=$ingest_token" \
    "MAIL_TASK_GATEWAY_BRIDGE_SERVICE_API_KEY=$gateway_bridge_key"
  write_store "$sandbox/containers/model-plane-model-gateway-1.env" \
    "MAIL_TASK_BRIDGE_SERVICE_API_KEY=$bridge_key"
  write_store "$sandbox/containers/user-service.env" \
    "MAIL_TASK_RECEIVING_DOMAIN=tasks.example.test"
  write_store "$sandbox/containers/auth-service.env" \
    "PLANE_SERVICE_PRINCIPALS_JSON={\"model-gateway-mail-task-bridge\":{\"credential\":\"$bridge_key\"},\"verevon-gateway\":{\"credential\":\"$gateway_bridge_key\"}}"
}

write_containers_consistent
run_check "${ready_env[@]}" -- --store "$minted_store" --live
expect_status 'live, everything consistent: exits 0' 0
expect_output 'live, everything consistent: reports ready' 'result: ready'
expect_output 'live, everything consistent: the ingest key agrees' 'hold the same secret: yes'
expect_output 'live, everything consistent: the principals are registered with the store credentials' 'model-gateway-mail-task-bridge'
expect "live, everything consistent: no principal is missing or mismatched" '! grep -qE "missing|matches the store: no" <<<"$output"'
expect 'live, everything consistent: every probe was readable' '! grep -q unreadable <<<"$output"'
expect 'live, everything consistent: all nine container bindings agree with the offline copy' '[[ $(grep -c "agrees with offline copy: yes" <<<"$output") -eq 9 ]]'
expect 'live, everything consistent: both principals match the store' '[[ $(grep -c "credential matches the store: yes" <<<"$output") -eq 2 ]]'
expect_no_secret 'live, everything consistent: prints no value'

# The BFF holds a different ingest key than integration-api.
write_containers_consistent
write_store "$sandbox/containers/verevon-gateway-rs.env" \
  "MAIL_TASK_INGEST_SERVICE_TOKEN=$other_ingest_token" \
  "MAIL_TASK_GATEWAY_BRIDGE_SERVICE_API_KEY=$gateway_bridge_key"
run_check "${ready_env[@]}" -- --store "$minted_store" --live
expect_status 'live, ingest keys differ: exits 1' 1
expect_output 'live, ingest keys differ: says the two ends differ' 'ingest HMAC key differs'
expect_no_secret 'live, ingest keys differ: prints no value'

# The ingest key is set on one side only.
write_containers_consistent
write_store "$sandbox/containers/verevon-gateway-rs.env" \
  "MAIL_TASK_GATEWAY_BRIDGE_SERVICE_API_KEY=$gateway_bridge_key"
run_check "${ready_env[@]}" -- --store "$minted_store" --live
expect_status 'live, ingest key on one side only: exits 1' 1
expect_output 'live, ingest key on one side only: names the half-set pair' 'set on only one of'

# auth-service holds a different credential for a principal than the store.
write_containers_consistent
write_store "$sandbox/containers/auth-service.env" \
  "PLANE_SERVICE_PRINCIPALS_JSON={\"model-gateway-mail-task-bridge\":{\"credential\":\"$other_ingest_token\"},\"verevon-gateway\":{\"credential\":\"$gateway_bridge_key\"}}"
run_check "${ready_env[@]}" -- --store "$minted_store" --live
expect_status 'live, a principal credential differs from the store: exits 1' 1
expect_output 'live, a principal credential differs from the store: names the principal' 'differs from the one in the store'
expect_no_secret 'live, a principal credential differs from the store: prints no value'

# The feature is switched on here but a container never received it.
write_containers_consistent
write_store "$sandbox/containers/integration-api.env" \
  "MAIL_TASK_USER_CORE_SERVICE_TOKEN=$worker_token"
run_check "${ready_env[@]}" -- --store "$minted_store" --live
expect_status 'live, switch on but a container lacks it: exits 1' 1
expect_output 'live, switch on but a container lacks it: says to recreate it' 'not applied: recreate integration-api'

# Feature off and the containers carry nothing: dark, exit 0, principals not yet loaded.
rm -f "$sandbox"/containers/*.env
for fixture in integration-api verevon-gateway-rs model-plane-model-gateway-1 user-service; do
  write_store "$sandbox/containers/$fixture.env" "UNRELATED=1"
done
write_store "$sandbox/containers/auth-service.env" 'PLANE_SERVICE_PRINCIPALS_JSON={"execution-core":{"credential":"x"}}'
run_check -- --store "$minted_store" --live
expect_status 'live, feature off and containers dark: exits 0' 0
expect_output 'live, feature off and containers dark: reports feature off' 'feature off'
expect_output 'live, feature off and containers dark: notes the principals are not loaded yet' 'has not registered the principal'

# Docker unreachable: exit 3 only when nothing else failed.
run_check FAKE_DOCKER_DOWN=1 -- --store "$minted_store" --live
expect_status 'live, docker unreachable and feature off: exits 3' 3
expect_output 'live, docker unreachable: says so' 'docker is not reachable'
run_check FAKE_DOCKER_DOWN=1 "MAIL_TASK_PROVIDER=postmark" -- --store "$minted_store" --live
expect_status 'live, docker unreachable but a binding is half-set: exits 1' 1

printf '\n%d passed, %d failed\n' "$pass" "$fail"
(( fail == 0 ))
