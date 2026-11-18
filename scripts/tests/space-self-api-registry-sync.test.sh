#!/usr/bin/env bash
# Drift check for the Space self-API registry (WS18-S1, WS18-S6, WS19-S9).
#
# The registry is the one list of Space actions a model may request through the self API: each row
# says what the action is, who may run it, its argument schema, its schema hash and its digest vector.
# The golden is AUTHORED with the Model Plane contract (apps/Model Plane/api/contracts/), its .sha256
# sidecar carries the same digest, and four consumers vendor byte-identical copies:
#
#   * Control, user-core      apps/Control Plane/user-core/internal/spaces/registry/
#   * Model Plane, capability-core (Go)  apps/Model Plane/go/services/capability-core/internal/api/registry/
#   * Model Plane, mp-contracts (Rust)   apps/Model Plane/rust/crates/mp-contracts/contracts/
#     (under the crate because the Model Plane Dockerfiles copy only rust/ and proto/)
#   * the SPA                 apps/Frontend Plane/verevonv3/src/shared/api/contracts/ (with its .sha256)
#
# This script checks, in order:
#   * the golden hashes to the digest pinned below, and its .sha256 sidecar agrees;
#   * every copy is byte-identical to the golden, and the SPA pin is byte-identical to the golden pin;
#   * the structural rules in space-self-api-registry-check.mjs hold on the golden: no forbidden part in
#     an action id tail or an argument name, reservation only for tickets.create, schema hashes equal
#     the sha256 of parameters_json (tickets.create is the pinned legacy row and exempt), argument_order
#     names each parameter once, and the digest vectors reproduce their keys and digests.
#
# Changing the registry is a deliberate second step: the owner's change edits the golden, the pin below,
# the sidecar and every copy together. Adding a copy: append it to COPIES in the change that adds it.
#
#   scripts/tests/space-self-api-registry-sync.test.sh              check the repository
#   scripts/tests/space-self-api-registry-sync.test.sh --self-test  prove each check fails on a tampered tree
set -Eeuo pipefail

SELF="${BASH_SOURCE[0]}"
SCRIPT_DIR="$(cd "$(dirname "$SELF")" && pwd)"
# SPACE_SELF_API_ROOT lets the self-test point the same checks at a scratch tree.
ROOT="${SPACE_SELF_API_ROOT:-$(cd "$SCRIPT_DIR/../.." && pwd)}"
CHECKER="$SCRIPT_DIR/space-self-api-registry-check.mjs"

GOLDEN="apps/Model Plane/api/contracts/space-self-api-registry.json"
# The digest the golden must hash to. Pinned here, not read from the sidecar, so a golden that is
# edited together with its sidecar still fails until this pin is updated in the same change.
GOLDEN_SHA256="635587a25114dcd34d183a7de0ff43e13ce935a990d849a57a063a6640c09f8d"

# Byte-identical copies of the golden.
COPIES=(
  "apps/Control Plane/user-core/internal/spaces/registry/space-self-api-registry.json"
  "apps/Model Plane/go/services/capability-core/internal/api/registry/space-self-api-registry.json"
  "apps/Model Plane/rust/crates/mp-contracts/contracts/space-self-api-registry.json"
  "apps/Frontend Plane/verevonv3/src/shared/api/contracts/space-self-api-registry.json"
)
# The SPA copy carries the same pin file beside it; that pin must match the golden's.
SPA_PIN="apps/Frontend Plane/verevonv3/src/shared/api/contracts/space-self-api-registry.json.sha256"

passes=0
failures=0
pass() { printf 'ok - %s\n' "$1"; passes=$((passes + 1)); }
fail() { printf 'not ok - %s\n' "$1" >&2; failures=$((failures + 1)); }

sha256_of() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | cut -d' ' -f1
  else
    shasum -a 256 "$1" | cut -d' ' -f1
  fi
}

# Node on Windows cannot read a Cygwin path; everywhere else this is the identity.
to_native() {
  if command -v cygpath >/dev/null 2>&1; then cygpath -w "$1"; else printf '%s' "$1"; fi
}

check_golden_pin() {
  local golden="$ROOT/$GOLDEN" sidecar="$ROOT/$GOLDEN.sha256" hash="" name=""
  if [[ ! -f "$golden" ]]; then
    fail "golden is missing: $GOLDEN"
    return
  fi
  if [[ "$(sha256_of "$golden")" == "$GOLDEN_SHA256" ]]; then
    pass "golden matches its pinned digest: $GOLDEN"
  else
    fail "golden differs from its pinned digest: $GOLDEN (pinned $GOLDEN_SHA256, actual $(sha256_of "$golden"))"
  fi
  if [[ ! -f "$sidecar" ]]; then
    fail "pin file beside the golden is missing: $GOLDEN.sha256"
    return
  fi
  read -r hash name <"$sidecar" || true
  name="${name#\*}"
  if [[ "$hash" == "$GOLDEN_SHA256" && "$name" == "space-self-api-registry.json" ]]; then
    pass "pin file beside the golden agrees with the pinned digest: $GOLDEN.sha256"
  else
    fail "pin file beside the golden disagrees with the pinned digest: $GOLDEN.sha256 says '$hash $name'"
  fi
}

check_copies() {
  local copy
  for copy in "${COPIES[@]}"; do
    if [[ ! -f "$ROOT/$copy" ]]; then
      fail "copy is missing: $copy"
    elif cmp -s "$ROOT/$copy" "$ROOT/$GOLDEN"; then
      pass "copy is byte-identical to the golden: $copy"
    else
      fail "copy differs from the golden: $copy"
    fi
  done
  if [[ ! -f "$ROOT/$SPA_PIN" ]]; then
    fail "SPA pin is missing: $SPA_PIN"
  elif cmp -s "$ROOT/$SPA_PIN" "$ROOT/$GOLDEN.sha256"; then
    pass "SPA pin is byte-identical to the golden pin: $SPA_PIN"
  else
    fail "SPA pin differs from the golden pin: $SPA_PIN"
  fi
}

# Runs the structural rules on one registry file. The checker prints one ok/not ok line per rule;
# each line is counted here, and a non-zero exit with no failed rule named is itself a failure.
check_rules() {
  local file="$1" output="" rc=0 before="$failures" line
  if ! command -v node >/dev/null 2>&1; then
    fail "node is required to check the registry rules (scripts/tests/space-self-api-registry-check.mjs); a check that cannot run is a failure"
    return
  fi
  output="$(node "$(to_native "$CHECKER")" "$(to_native "$file")" 2>&1)" || rc=$?
  while IFS= read -r line; do
    case "$line" in
      "ok - "*) pass "${line#ok - }" ;;
      "not ok - "*) fail "${line#not ok - }" ;;
    esac
  done <<<"$output"
  if [[ "$rc" -ne 0 && "$failures" -eq "$before" ]]; then
    fail "the registry rules checker exited $rc without naming a failed rule: $output"
  fi
}

# Writes one mutant of the real golden to $SCRATCH/mutant.json with the node program $3, then requires
# the rules to reject it on the line that owns the broken property ($2 is a substring of that line).
expect_rule_to_fail() {
  local label="$1" words="$2" program="$3" output="" rc=0
  if ! node -e "$program" "$(to_native "$SCRATCH/mutant.json")" "$(to_native "$ROOT/$GOLDEN")"; then
    fail "self-test: could not write the mutant for $label"
    return
  fi
  output="$(node "$(to_native "$CHECKER")" "$(to_native "$SCRATCH/mutant.json")" 2>&1)" || rc=$?
  if [[ "$rc" -eq 0 ]]; then
    fail "self-test: the rules accept $label"
  elif grep -q "^not ok - .*${words}" <<<"$output"; then
    pass "self-test: $label fails the rule that owns it"
  else
    fail "self-test: $label fails, but not on the rule that owns it: $output"
  fi
}

self_test() {
  SCRATCH="$(mktemp -d)"
  trap 'rm -rf "$SCRATCH"' EXIT
  local copy
  mkdir -p "$SCRATCH/$(dirname "$GOLDEN")"
  cp "$ROOT/$GOLDEN" "$SCRATCH/$GOLDEN"
  cp "$ROOT/$GOLDEN.sha256" "$SCRATCH/$GOLDEN.sha256"
  for copy in "${COPIES[@]}"; do
    mkdir -p "$SCRATCH/$(dirname "$copy")"
    cp "$ROOT/$copy" "$SCRATCH/$copy"
  done
  mkdir -p "$SCRATCH/$(dirname "$SPA_PIN")"
  cp "$ROOT/$SPA_PIN" "$SCRATCH/$SPA_PIN"

  if SPACE_SELF_API_ROOT="$SCRATCH" bash "$SELF" >/dev/null 2>&1; then
    pass "self-test: an untouched scratch tree passes"
  else
    fail "self-test: the checks reject an untouched scratch tree"
  fi

  printf ' ' >>"$SCRATCH/$GOLDEN"
  if SPACE_SELF_API_ROOT="$SCRATCH" bash "$SELF" >/dev/null 2>&1; then
    fail "self-test: the checks accept a tampered golden"
  else
    pass "self-test: a tampered golden fails its pin"
  fi
  cp "$ROOT/$GOLDEN" "$SCRATCH/$GOLDEN"

  printf ' ' >>"$SCRATCH/${COPIES[0]}"
  if SPACE_SELF_API_ROOT="$SCRATCH" bash "$SELF" >/dev/null 2>&1; then
    fail "self-test: the checks accept a drifted copy"
  else
    pass "self-test: a drifted copy fails the byte check"
  fi
  cp "$ROOT/${COPIES[0]}" "$SCRATCH/${COPIES[0]}"

  printf ' ' >>"$SCRATCH/$SPA_PIN"
  if SPACE_SELF_API_ROOT="$SCRATCH" bash "$SELF" >/dev/null 2>&1; then
    fail "self-test: the checks accept a drifted SPA pin"
  else
    pass "self-test: a drifted SPA pin fails the byte check"
  fi
  cp "$ROOT/$SPA_PIN" "$SCRATCH/$SPA_PIN"

  # Each mutant keeps every other rule true (hashes recomputed where needed), so exactly one rule trips.
  local prelude='const fs=require("fs"),crypto=require("crypto");const g=JSON.parse(fs.readFileSync(process.argv[2],"utf8"));const row=(id)=>g.rows.find((r)=>r.action_id===id);const save=()=>fs.writeFileSync(process.argv[1],JSON.stringify(g,null,2)+"\n");'

  expect_rule_to_fail "a Space argument named with a forbidden part" "argument names carry" \
    "$prelude"'const r=row("space.thread.rename");const s=JSON.parse(r.parameters_json);s.properties.user_title=s.properties.title;delete s.properties.title;r.argument_order=["user_title"];r.parameters_json=JSON.stringify(s);r.schema_sha256="sha256:"+crypto.createHash("sha256").update(r.parameters_json).digest("hex");save();'

  expect_rule_to_fail "a Space action id whose tail carries a forbidden part" "action id tail names" \
    "$prelude"'row("space.thread.pin").action_id="space.thread.role_pin";save();'

  expect_rule_to_fail "a Space row marked as a reservation" "reservation admits" \
    "$prelude"'row("space.thread.rename").reservation=true;save();'

  expect_rule_to_fail "a parameters_json edited without re-hashing it" "schema hash is the sha256" \
    "$prelude"'const r=row("space.thread.pin");r.parameters_json=r.parameters_json+" ";save();'

  expect_rule_to_fail "the legacy tickets.create row claiming the derived hash basis" "schema hash is the sha256" \
    "$prelude"'row("tickets.create").hash_basis="parameters_json";save();'

  expect_rule_to_fail "an argument_order naming a non-property" "argument_order names each parameter" \
    "$prelude"'row("space.canvas.write").argument_order.push("ghost");save();'

  expect_rule_to_fail "a digest vector whose payload_digest does not reproduce" "digest vectors reproduce" \
    "$prelude"'g.digest_vectors[0].payload_digest="sha256:"+"0".repeat(64);save();'

  expect_rule_to_fail "a registry without the space.thread.rename digest vector" "space.thread.rename digest vector" \
    "$prelude"'g.digest_vectors=[];save();'

  printf '%s passed, %s failed\n' "$passes" "$failures"
  [[ "$failures" -eq 0 ]]
}

if [[ "${1:-}" == "--self-test" ]]; then
  self_test
  exit $?
fi

check_golden_pin
check_copies
check_rules "$ROOT/$GOLDEN"
printf '%d passed, %d failed\n' "$passes" "$failures"
[[ "$failures" -eq 0 ]]
