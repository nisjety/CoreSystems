#!/usr/bin/env bash
# =============================================================================
# isolated-registry-parity-test.sh  (design task QB-T08)
#
# The isolated real-authority runner gives Auth Core a principal registry. Its quarry-edge row must
# be the committed row (apps/Control Plane/config/plane-service-principals.json), generated and not
# typed, so the isolated stack mints exactly the scopes production mints and a scope change in the
# committed policy reaches the isolated stack. This contract keeps that true:
#
#   1. The runner calls the generator for quarry-edge and types no quarry-edge row or scope itself.
#   2. The generator's quarry-edge row equals the committed row: every authority field, the launcher
#      key `consumerEnv` removed, one credential added. The other rows are preserved untouched.
#   3. The generator refuses a committed row that is allowAnyOrg (no permissive copy is possible).
#   4. The comparison itself can fail: a mutated policy must be reported as a disagreement.
#
# Run from anywhere:   bash tests/scripts/isolated-registry-parity-test.sh
# Exit status: 0 the contract holds, 1 it does not.
# =============================================================================
set -euo pipefail
set +x

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
GENERATOR="$ROOT/tests/scripts/isolated-plane-principals.mjs"
RUNNER="$ROOT/tests/scripts/run-isolated-real-authority.sh"
POLICY="$ROOT/../Control Plane/config/plane-service-principals.json"
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT

fail() {
  printf 'FAIL: %s\n' "$1" >&2
  exit 1
}

[ -f "$GENERATOR" ] || fail "missing generator $GENERATOR"
[ -f "$RUNNER" ] || fail "missing runner $RUNNER"
[ -f "$POLICY" ] || fail "missing committed principal policy $POLICY"

# 1. The runner generates the row from the committed file and types none of it.
grep -Fq 'isolated-plane-principals.mjs' "$RUNNER" || fail "the runner does not call the generator"
grep -Fq 'Control Plane/config/plane-service-principals.json' "$RUNNER" ||
  fail "the runner does not generate from the committed principal policy"
if grep -Eq '^[[:space:]]*.*"quarry-edge"[[:space:]]*:' "$RUNNER"; then
  fail "the runner types a quarry-edge registry row by hand"
fi
if grep -Fq 'data:rerank' "$RUNNER"; then
  fail "the runner types a quarry-edge scope by hand"
fi

# The committed row is the reference. It must hold the scopes the QB design needs, production-bound.
node -e '
  const fs = require("fs");
  const policy = JSON.parse(fs.readFileSync(process.argv[1], "utf8"));
  const row = policy["quarry-edge"];
  if (!row) throw new Error("no quarry-edge row in the committed policy");
  if (!row.scopes.includes("data:rerank")) throw new Error("quarry-edge lacks data:rerank in the committed policy");
  if (row.allowAnyOrg === true) throw new Error("quarry-edge is allowAnyOrg in the committed policy");
' "$POLICY" || fail "the committed quarry-edge row is not the production row the design expects"

# The comparison, shared by the real check and the negative control. It prints a disagreement and
# exits 1; it never prints a credential.
cat >"$TMP/compare.cjs" <<'NODE'
const fs = require("node:fs");
const [policyPath, mergedPath, keyEnv] = process.argv.slice(2);
const canon = (value) => {
  if (Array.isArray(value)) return value.map(canon);
  if (value !== null && typeof value === "object") {
    return Object.fromEntries(Object.keys(value).sort().map((key) => [key, canon(value[key])]));
  }
  return value;
};
const policy = JSON.parse(fs.readFileSync(policyPath, "utf8"));
const merged = JSON.parse(fs.readFileSync(mergedPath, "utf8"));
const { consumerEnv: _launcherKey, ...expected } = policy["quarry-edge"];
const { credential, ...generated } = merged["quarry-edge"] ?? {};
if (typeof credential !== "string" || credential.length < 32) {
  console.error("disagreement: the generated quarry-edge row has no credential");
  process.exit(1);
}
if (JSON.stringify(canon(generated)) !== JSON.stringify(canon(expected))) {
  const fields = [...new Set([...Object.keys(generated), ...Object.keys(expected)])]
    .filter((key) => JSON.stringify(canon(generated[key])) !== JSON.stringify(canon(expected[key])));
  console.error(`disagreement: the generated quarry-edge row differs from the committed row in: ${fields.join(", ")}`);
  process.exit(1);
}
if (merged["retrieval-engine"]?.credential !== keyEnv) {
  console.error("disagreement: a row that was already in the registry was changed");
  process.exit(1);
}
NODE

# Input registry: the other rows as the runner carries them (one fixture credential, never printed).
# Generated per run: a credential-shaped literal in source would be a secret-scan finding.
FIXTURE_OTHER_CREDENTIAL=$(openssl rand -hex 32)
EXPECTED_CREDENTIAL=$(openssl rand -hex 32)
export REAL_AUTHORITY_PLANE_PRINCIPALS_JSON="{\"retrieval-engine\":{\"credential\":\"$FIXTURE_OTHER_CREDENTIAL\",\"audiences\":[\"control-policy\"],\"orgIds\":[],\"allowAnyOrg\":true,\"scopes\":[\"data:authorization:decide\"],\"scopesByAudience\":{\"control-policy\":[\"data:authorization:decide\"]}}}"
export PARITY_QUARRY_CREDENTIAL="$EXPECTED_CREDENTIAL"

# 2. The real check: the generated row equals the committed row.
node "$GENERATOR" merge "$POLICY" quarry-edge PARITY_QUARRY_CREDENTIAL >"$TMP/merged.json" ||
  fail "the generator refused the committed quarry-edge row"
node "$TMP/compare.cjs" "$POLICY" "$TMP/merged.json" "$FIXTURE_OTHER_CREDENTIAL" ||
  fail "the isolated quarry-edge row disagrees with the committed policy"
node -e '
  const merged = JSON.parse(require("node:fs").readFileSync(process.argv[1], "utf8"));
  process.exit(merged["quarry-edge"].credential === process.argv[2] ? 0 : 1);
' "$TMP/merged.json" "$EXPECTED_CREDENTIAL" ||
  fail "the generator did not carry the credential it was given"

# 3. A committed row that is allowAnyOrg is refused: the isolated copy cannot be permissive.
node -e '
  const fs = require("node:fs");
  const policy = JSON.parse(fs.readFileSync(process.argv[1], "utf8"));
  policy["quarry-edge"].allowAnyOrg = true;
  fs.writeFileSync(process.argv[2], JSON.stringify(policy));
' "$POLICY" "$TMP/permissive-policy.json"
if node "$GENERATOR" merge "$TMP/permissive-policy.json" quarry-edge PARITY_QUARRY_CREDENTIAL >"$TMP/permissive.out" 2>"$TMP/permissive.err"; then
  fail "the generator copied an allowAnyOrg row"
fi
[ ! -s "$TMP/permissive.out" ] || fail "a refused generation still wrote a registry"

# 4. Negative control: a committed policy that gains a scope the registry lacks is a disagreement.
node -e '
  const fs = require("node:fs");
  const policy = JSON.parse(fs.readFileSync(process.argv[1], "utf8"));
  policy["quarry-edge"].scopes.push("org:data:write_all");
  policy["quarry-edge"].scopesByAudience["data-plane"].push("org:data:write_all");
  fs.writeFileSync(process.argv[2], JSON.stringify(policy));
' "$POLICY" "$TMP/drifted-policy.json"
if node "$TMP/compare.cjs" "$TMP/drifted-policy.json" "$TMP/merged.json" "$FIXTURE_OTHER_CREDENTIAL" 2>"$TMP/drift.err"; then
  fail "the comparison did not report a scope the registry lacks"
fi
grep -Fq 'disagreement' "$TMP/drift.err" || fail "the drift was not reported as a disagreement"
if grep -Fq "$EXPECTED_CREDENTIAL" "$TMP/drift.err"; then
  fail "a disagreement message exposed a credential"
fi

# 5. Refusing a second copy: a registry that already has quarry-edge is not extended.
export REAL_AUTHORITY_PLANE_PRINCIPALS_JSON="$(cat "$TMP/merged.json")"
if node "$GENERATOR" merge "$POLICY" quarry-edge PARITY_QUARRY_CREDENTIAL >/dev/null 2>"$TMP/twice.err"; then
  fail "the generator accepted a quarry-edge row that was already present"
fi

printf 'PASS: isolated registry parity contract\n'
