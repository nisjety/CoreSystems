#!/usr/bin/env bash
# Which areas of the monorepo does a change touch? (slice M0-CI, plan 4.5.5)
#
# space-gate.yml runs one job per touched area. GitHub's own `paths:` filter is
# all-or-nothing per workflow, so the per-area decision lives here, in a script
# that can be run and tested by hand:
#
#   BASE_SHA=<sha> HEAD_SHA=<sha> bash scripts/gates/space-changed-areas.sh
#   bash scripts/gates/space-changed-areas.sh --files <file-with-one-path-per-line>
#   bash scripts/gates/space-changed-areas.sh --self-test
#
# Output: `key=value` lines on stdout (and appended to $GITHUB_OUTPUT when set).
# Booleans are the strings `true` / `false`; the *_services keys are JSON arrays
# of service directory names, ready for fromJSON() in a job matrix.
#
# An unknown base (a new branch, a manual run, a force-push with no ancestor, or
# a diff that fails) selects EVERYTHING. A gate that cannot tell what changed
# must not decide that nothing did.
set -Eeuo pipefail

CP_GO=(user-core audit-core org-core)
MP_GO=(bridge-core browser-broker capability-core cost-core letta-bridge nats-provisioner orchestrator-core sandbox-manager)
FV='apps/Frontend Plane/verevonv3'

json_array() { # args: names -> ["a","b"]
  local out='[' first=1 n
  for n in "$@"; do
    if [ "$first" -eq 1 ]; then first=0; else out+=','; fi
    out+="\"$n\""
  done
  printf '%s]' "$out"
}

# compute <file-with-paths> <all:0|1> -> prints key=value lines
compute() {
  local files="$1" all="$2"
  local cp_hit=() mp_hit=() n
  local principals=false auth=false mp_rust=false proto=false gateway=false spa=false notif=false
  has() { [ "$all" = 1 ] && return 0; grep -qE "$1" "$files"; }

  if [ "$all" = 1 ]; then
    cp_hit=("${CP_GO[@]}"); mp_hit=("${MP_GO[@]}")
  else
    for n in "${CP_GO[@]}"; do grep -qF "apps/Control Plane/$n/" "$files" && cp_hit+=("$n") || true; done
    # A regenerated binding under go/gen can break any Model Plane Go service.
    if grep -qF 'apps/Model Plane/go/gen/' "$files"; then
      mp_hit=("${MP_GO[@]}")
    else
      for n in "${MP_GO[@]}"; do grep -qF "apps/Model Plane/go/services/$n/" "$files" && mp_hit+=("$n") || true; done
    fi
  fi

  has '^apps/Control Plane/config/|^apps/Control Plane/scripts/(plane-principal-registry\.jq|run-control-plane\.sh)$|^scripts/bootstrap_runtime_environment\.sh$|^scripts/tests/principal-registry-parity\.test\.sh$' && principals=true
  has '^apps/Control Plane/auth-core/|^apps/Control Plane/config/' && auth=true
  has '^apps/Model Plane/(rust|proto|api/contracts)/' && mp_rust=true
  has '^apps/Model Plane/proto/|^scripts/tests/zdr-(contract-sync\.test\.sh|contract-schema\.mjs)$|^scripts/gates/buf-lint-ratchet\.py$|^docs/compliance/evidence/' && proto=true
  has "^${FV}/apps/gateway/" && gateway=true
  # The SPA job covers everything under verevonv3 except the Rust gateway crate.
  if [ "$all" = 1 ] || grep -E "^${FV}/" "$files" | grep -qvE "^${FV}/apps/gateway/"; then spa=true; fi
  has '^apps/Application Plane/notification-core/' && notif=true

  local out_cp out_mp
  out_cp=$(json_array "${cp_hit[@]+"${cp_hit[@]}"}")
  out_mp=$(json_array "${mp_hit[@]+"${mp_hit[@]}"}")
  printf '%s\n' \
    "cp_go_services=$out_cp" \
    "mp_go_services=$out_mp" \
    "principals=$principals" \
    "auth_core=$auth" \
    "mp_rust=$mp_rust" \
    "proto=$proto" \
    "gateway=$gateway" \
    "spa=$spa" \
    "notification_core=$notif" \
    "full_run=$([ "$all" = 1 ] && echo true || echo false)"
}

emit() {
  if [ -n "${GITHUB_OUTPUT:-}" ]; then tee -a "$GITHUB_OUTPUT"; else cat; fi
}

self_test() {
  local tmp; tmp="$(mktemp -d)"
  local fail=0
  expect() { # <label> <key> <value> <files...>
    local label="$1" key="$2" want="$3"; shift 3
    printf '%s\n' "$@" > "$tmp/f"
    local got; got="$(compute "$tmp/f" 0 | grep "^$key=" | cut -d= -f2-)"
    if [ "$got" != "$want" ]; then echo "FAIL $label: $key=$got, wanted $want"; fail=1; else echo "ok   $label"; fi
  }
  expect 'user-core only'       cp_go_services '["user-core"]' 'apps/Control Plane/user-core/internal/http/spaces.go'
  expect 'unrelated file'       cp_go_services '[]'            'README.md'
  expect 'unrelated file spa'   spa            false           'README.md'
  expect 'gateway is not spa'   spa            false           'apps/Frontend Plane/verevonv3/apps/gateway/src/main.rs'
  expect 'gateway is gateway'   gateway        true            'apps/Frontend Plane/verevonv3/apps/gateway/src/main.rs'
  expect 'spa src'              spa            true            'apps/Frontend Plane/verevonv3/src/features/spaces/a.ts'
  expect 'proto hits rust'      mp_rust        true            'apps/Model Plane/proto/model_plane/v1/session.proto'
  expect 'proto hits proto'     proto          true            'apps/Model Plane/proto/model_plane/v1/session.proto'
  expect 'gen hits every go'    mp_go_services '["bridge-core","browser-broker","capability-core","cost-core","letta-bridge","nats-provisioner","orchestrator-core","sandbox-manager"]' 'apps/Model Plane/go/gen/model_plane/v1/x.pb.go'
  expect 'principals'           principals     true            'apps/Control Plane/config/plane-service-principals.json'
  expect 'convex is not here'   spa            false           'apps/Application Plane/convex-core/convex/schema.ts'
  printf '' > "$tmp/f"
  if [ "$(compute "$tmp/f" 1 | grep -c '=true')" -lt 7 ]; then echo "FAIL full run did not select every area"; fail=1; else echo "ok   full run selects every area"; fi
  rm -rf "$tmp"
  [ "$fail" -eq 0 ] || { echo "self-test FAILED"; return 1; }
  echo "self-test passed"
}

main() {
  case "${1:-}" in
    --self-test) self_test; return ;;
    --files)
      compute "$2" 0 | emit; return ;;
  esac
  tmp="$(mktemp)"; trap 'rm -f "$tmp"' EXIT
  local base="${BASE_SHA:-}" head="${HEAD_SHA:-HEAD}" all=0
  if [ -z "$base" ] || [[ "$base" =~ ^0+$ ]] || ! git cat-file -e "${base}^{commit}" 2>/dev/null; then
    echo "base commit unknown: selecting every area" >&2
    all=1
  elif ! git diff --name-only "${base}...${head}" > "$tmp" 2>/dev/null \
    && ! git diff --name-only "${base}" "${head}" > "$tmp"; then
    echo "diff failed: selecting every area" >&2
    all=1
  fi
  compute "$tmp" "$all" | emit
}

main "$@"
