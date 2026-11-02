#!/usr/bin/env bash
set -euo pipefail
set +x

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
SCRIPT="$ROOT/scripts/auth-regression-matrix.sh"
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT

fail() {
  printf 'FAIL: %s\n' "$1" >&2
  exit 1
}

run_matrix() {
  PATH="$TMP/bin:$PATH" \
    FAKE_DOCKER_LOG="$TMP/docker.log" \
    DPV2_TEST_ORG_ID="matrix-own-org" \
    DPV2_SPOOF_ORG_ID="matrix-spoof-org" \
    DPV2_USER_BEARER="header.payload.signature" \
    DPV2_SERVICE_BEARER="service.payload.signature" \
    DPV2_QUALITY_BEARER="quality.payload.signature" \
    DPV2_QUICKWIT_ADMIN_BEARER="admin.payload.signature" \
    "$SCRIPT"
}

mkdir -p "$TMP/bin"
cat >"$TMP/bin/docker" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail

[ "${1:-}" = "compose" ] || exit 91
shift
if [ "${1:-}" = "--project-name" ]; then
  printf 'project=%s\n' "${2:-}" >>"$FAKE_DOCKER_LOG"
  shift 2
fi
[ "${1:-}" = "exec" ] || exit 92
[ "${2:-}" = "-T" ] || exit 93
service=${3:-}
[ "${4:-}" = "curl" ] || exit 94
[ "${5:-}" = "--config" ] || exit 95
[ "${6:-}" = "-" ] || exit 96

config=$(cat)
printf '%s\n' "$service" >>"$FAKE_DOCKER_LOG"
# Which bearer each call carries and which route it names. The service rows are told apart by these,
# not only by their printed status: a row that sent the user token would answer 403 as well.
bearer=none
if printf '%s' "$config" | grep -Fq 'Authorization: Bearer service.payload.signature'; then
  bearer=service
elif printf '%s' "$config" | grep -Fq 'Authorization: Bearer header.payload.signature'; then
  bearer=user
elif printf '%s' "$config" | grep -Fq 'Authorization: Bearer '; then
  bearer=other
fi
route=$(printf '%s' "$config" | grep -o 'url = "http[^"]*"' | grep -o '/v1/[^"]*' | head -n 1 || true)
printf '%s %s %s\n' "$service" "$bearer" "$route" >>"$FAKE_DOCKER_LOG.calls"

if [ "$service" = "retrieval-engine" ] && [ -n "${FAKE_RETRIEVAL_STATUS:-}" ]; then
  printf '%s' "$FAKE_RETRIEVAL_STATUS"
  exit 0
fi

# QB-T08 service rows. The fake answers as the route table does: no bearer is 401, and every
# other caller of these two routes is 403 (the rerank route is service-only and the service
# bearer carries no data:rerank; the admin route is not granted to any service). The prune
# body must be a dry run, so a harness change that sends a live prune fails here.
if [ "$service" = "retrieval-engine" ] && printf '%s' "$config" | grep -Fq '/v1/rerank'; then
  if ! printf '%s' "$config" | grep -Fq 'Authorization: Bearer '; then
    printf '401'
  else
    printf '403'
  fi
  exit 0
fi
if [ "$service" = "retrieval-engine" ] && printf '%s' "$config" | grep -Fq '/v1/cache/semantic/prune'; then
  printf '%s' "$config" | grep -Fq '\"dry_run\":true' || exit 99
  if ! printf '%s' "$config" | grep -Fq 'Authorization: Bearer '; then
    printf '401'
  else
    printf '403'
  fi
  exit 0
fi

if [ "$service" = "quickwit-adapter" ]; then
  printf '%s' "$config" | grep -Fq '\"dry_run\":true' || exit 97
  if printf '%s' "$config" | grep -Eq '\"(global|clear|break_glass)\":true'; then
    exit 98
  fi
fi

if ! printf '%s' "$config" | grep -Fq 'Authorization: Bearer '; then
  printf '401'
elif printf '%s' "$config" | grep -Fq 'matrix-spoof-org'; then
  printf '403'
else
  printf '200'
fi
EOF
chmod +x "$TMP/bin/docker"

# RED/GREEN: every required credential and disposable tenant input is mandatory.
if PATH="$TMP/bin:$PATH" "$SCRIPT" >"$TMP/missing.out" 2>&1; then
  fail "matrix accepted missing credentials"
fi
grep -Fq 'DPV2_TEST_ORG_ID' "$TMP/missing.out" || fail "missing-input error was not actionable"

# QB-T08: the service bearer is a required input of its own, validated like the others.
if PATH="$TMP/bin:$PATH" \
  DPV2_TEST_ORG_ID="matrix-own-org" \
  DPV2_SPOOF_ORG_ID="matrix-spoof-org" \
  DPV2_USER_BEARER="header.payload.signature" \
  DPV2_QUALITY_BEARER="quality.payload.signature" \
  DPV2_QUICKWIT_ADMIN_BEARER="admin.payload.signature" \
  "$SCRIPT" >"$TMP/no-service.out" 2>&1; then
  fail "matrix accepted a missing service bearer"
fi
grep -Fq 'DPV2_SERVICE_BEARER' "$TMP/no-service.out" || fail "missing service bearer was not named"
if PATH="$TMP/bin:$PATH" \
  DPV2_TEST_ORG_ID="matrix-own-org" \
  DPV2_SPOOF_ORG_ID="matrix-spoof-org" \
  DPV2_USER_BEARER="header.payload.signature" \
  DPV2_SERVICE_BEARER="not-a-compact-jwt" \
  DPV2_QUALITY_BEARER="quality.payload.signature" \
  DPV2_QUICKWIT_ADMIN_BEARER="admin.payload.signature" \
  "$SCRIPT" >"$TMP/bad-service.out" 2>&1; then
  fail "matrix accepted a malformed service bearer"
fi
grep -Fq 'DPV2_SERVICE_BEARER must be a compact JWT' "$TMP/bad-service.out" || fail "malformed service bearer was not refused as a compact JWT"

: >"$TMP/docker.log"
: >"$TMP/docker.log.calls"
run_matrix >"$TMP/matrix.out" 2>&1 || {
  sed -n '1,200p' "$TMP/matrix.out" >&2
  fail "matrix rejected the safe fixture"
}

# 28 family assertions (seven families x four shapes) plus the five QB-T08 service rows.
grep -Fq '33 passed, 0 failed' "$TMP/matrix.out" || fail "matrix did not execute 7 x 4 + 5 assertions"
[ "$(wc -l <"$TMP/docker.log" | tr -d ' ')" = "33" ] || fail "unexpected transport request count"
for service in graph-index data-quality data-orchestrator documents-api wiki-store quickwit-adapter; do
  [ "$(grep -Fxc "$service" "$TMP/docker.log")" = "4" ] || fail "$service was not covered four times"
done
# retrieval-engine: four retrieval-family shapes plus the five service rows.
[ "$(grep -Fxc retrieval-engine "$TMP/docker.log")" = "9" ] || fail "retrieval-engine was not covered nine times"

# QB-T08: each service row PASSes with its encoded expected status. The printed line is the
# matrix's own format, so a row that answers differently cannot be mistaken for a pass.
for expected in \
  "$(printf '  PASS %-24s %-28s status=%s' web-rerank 'no credentials' 401)" \
  "$(printf '  PASS %-24s %-28s status=%s' web-rerank 'user token' 403)" \
  "$(printf '  PASS %-24s %-28s status=%s' web-rerank 'service without scope' 403)" \
  "$(printf '  PASS %-24s %-28s status=%s' service-admin 'service on admin route' 403)" \
  "$(printf '  PASS %-24s %-28s status=%s' service-retrieval 'service bearer' 200)"; do
  grep -Fxq "$expected" "$TMP/matrix.out" || fail "service row did not pass as encoded: $expected"
done
# Each service row sends the bearer it names to the route it names: the user token on the rerank
# route, no bearer for the 401 row, the service token for the scope-less row, the admin prune and
# the retrieval. A row that carried the wrong token or pointed at another path fails here.
for call in \
  'retrieval-engine none /v1/rerank' \
  'retrieval-engine user /v1/rerank' \
  'retrieval-engine service /v1/rerank' \
  'retrieval-engine service /v1/cache/semantic/prune' \
  'retrieval-engine service /v1/retrieve'; do
  grep -Fxq "$call" "$TMP/docker.log.calls" || fail "a service row does not send the bearer and route it encodes: $call"
done

# A row whose encoded expectation is wrong must FAIL the run, not pass silently.
sed 's/"no credentials" 401 retrieval-engine POST "\$RERANK_URL"/"no credentials" 403 retrieval-engine POST "$RERANK_URL"/' \
  "$SCRIPT" >"$TMP/mutated-matrix.sh"
chmod +x "$TMP/mutated-matrix.sh"
grep -Fq '"no credentials" 403 retrieval-engine POST "$RERANK_URL"' "$TMP/mutated-matrix.sh" || fail "mutation of the rerank expectation did not apply"
if PATH="$TMP/bin:$PATH" FAKE_DOCKER_LOG="$TMP/docker.log" \
  DPV2_TEST_ORG_ID="matrix-own-org" \
  DPV2_SPOOF_ORG_ID="matrix-spoof-org" \
  DPV2_USER_BEARER="header.payload.signature" \
  DPV2_SERVICE_BEARER="service.payload.signature" \
  DPV2_QUALITY_BEARER="quality.payload.signature" \
  DPV2_QUICKWIT_ADMIN_BEARER="admin.payload.signature" \
  "$TMP/mutated-matrix.sh" >"$TMP/mutated.out" 2>&1; then
  fail "matrix passed a rerank row with a wrong expected status"
fi

: >"$TMP/docker.log"
DPV2_COMPOSE_PROJECT="isolated-matrix" run_matrix >"$TMP/project-matrix.out" 2>&1 || fail "matrix rejected an explicit isolated Compose project"
[ "$(grep -c '^project=isolated-matrix$' "$TMP/docker.log")" = "33" ] || fail "matrix ignored the isolated Compose project"

# Never echo disposable identifiers or credentials, even on success.
for secret in matrix-own-org matrix-spoof-org header.payload.signature service.payload.signature quality.payload.signature admin.payload.signature; do
  if grep -Fq "$secret" "$TMP/matrix.out"; then
    fail "matrix output exposed a credential or organization identifier"
  fi
done

# A real HTTP failure stays distinguishable from a missing/failed transport,
# while response bodies and sensitive request material remain suppressed.
if FAKE_RETRIEVAL_STATUS=500 run_matrix >"$TMP/http-error.out" 2>&1; then
  fail "matrix accepted retrieval HTTP 500"
fi
grep -Fq 'retrieval                valid bearer                 status=500' "$TMP/http-error.out" ||
  fail "matrix collapsed a real HTTP status into a transport error"

# Curl reports 000 when no HTTP response exists. Keep that distinct from an
# actual three-digit server response.
if FAKE_RETRIEVAL_STATUS=000 run_matrix >"$TMP/transport-error.out" 2>&1; then
  fail "matrix accepted retrieval transport failure"
fi
grep -Fq 'retrieval                valid bearer                 status=transport-error' "$TMP/transport-error.out" ||
  fail "matrix treated curl HTTP code 000 as a server response"

# External endpoints are explicit and restricted to loopback for this local-only harness.
if DPV2_MATRIX_TRANSPORT=external \
  DPV2_TEST_ORG_ID="matrix-own-org" \
  DPV2_SPOOF_ORG_ID="matrix-spoof-org" \
  DPV2_USER_BEARER="header.payload.signature" \
  DPV2_SERVICE_BEARER="service.payload.signature" \
  DPV2_QUALITY_BEARER="quality.payload.signature" \
  DPV2_QUICKWIT_ADMIN_BEARER="admin.payload.signature" \
  DPV2_GRAPH_URL="https://production.invalid" \
  "$SCRIPT" >"$TMP/external.out" 2>&1; then
  fail "matrix accepted a non-loopback external endpoint"
fi
grep -Fq 'DPV2_GRAPH_URL must be an explicit loopback HTTP endpoint' "$TMP/external.out" ||
  fail "external endpoint was refused for another reason"

# The harness must not contain destructive route families or mutation flags.
if grep -Eq 'cleanup/orphans|bulk delete|index reset|purge|\"global\":true|\"clear\":true|\"break_glass\":true' "$SCRIPT"; then
  fail "matrix contains a destructive operation"
fi

# QB-T08: the isolated MVP runner is the matrix's in-repo caller that mints bearers, so it must hand the
# matrix the service bearer as well (the matrix refuses to run without one). That bearer is a fixture token.
# This checks that the token the matrix would receive is a verifiable quarry-edge service token with exactly
# the scope asked for, signed under the key the data plane trusts, and that a wider scope is refused.
RUNNER="$ROOT/tests/e2e/run-isolated-mvp.sh"
FIXTURE="$ROOT/tests/e2e/isolated/fixture.mjs"
grep -Fq 'DPV2_SERVICE_BEARER="$service_bearer"' "$RUNNER" ||
  fail "the isolated MVP runner does not hand the matrix a service bearer"
grep -Fq 'mint-service' "$RUNNER" || fail "the isolated MVP runner does not mint its service bearer with the fixture"

cat >"$TMP/verify-service-bearer.cjs" <<'NODE'
const fs = require("node:fs");
const crypto = require("node:crypto");
const [publicKeyPath, issuer, orgId] = process.argv.slice(2);
const token = process.env.MATRIX_TEST_TOKEN || "";
const parts = token.split(".");
const problems = [];
if (parts.length !== 3) {
  problems.push("shape");
} else {
  const [header, payload, signature] = parts;
  const verifier = crypto.createVerify("RSA-SHA256");
  verifier.update(`${header}.${payload}`);
  if (!verifier.verify(fs.readFileSync(publicKeyPath), Buffer.from(signature, "base64url"))) {
    problems.push("signature");
  }
  const claims = JSON.parse(Buffer.from(payload, "base64url").toString("utf8"));
  if (claims.sub !== "service:quarry-edge") problems.push("subject");
  if (claims.service_id !== claims.sub) problems.push("service_id");
  if (claims.principal_type !== "service") problems.push("principal_type");
  if (claims.aud !== "data-plane") problems.push("audience");
  if (claims.iss !== issuer) problems.push("issuer");
  if (claims.org_id !== orgId) problems.push("organization");
  if (claims.user_id !== undefined || claims.acting_user_id !== undefined) problems.push("user identity");
  if (JSON.stringify(claims.scopes) !== JSON.stringify(["data:read"])) problems.push("scopes");
  if (!(claims.exp > claims.iat)) problems.push("lifetime");
}
if (problems.length > 0) {
  console.error(`service bearer refused: ${problems.join(", ")}`);
  process.exit(1);
}
NODE

KEYS="$TMP/fixture-keys"
ISSUER="https://auth-core.isolated.test/api/convex-auth"
node "$FIXTURE" prepare "$KEYS" || fail "the isolated fixture could not prepare its keys"
MATRIX_TEST_TOKEN=$(JWT_ISSUER="$ISSUER" JWT_ORG_ID="matrix-own-org" JWT_SCOPES=data:read \
  node "$FIXTURE" mint-service "$KEYS") || fail "the fixture did not mint a service bearer"
export MATRIX_TEST_TOKEN
node "$TMP/verify-service-bearer.cjs" "$KEYS/public.pem" "$ISSUER" matrix-own-org ||
  fail "the minted service bearer is not the quarry-edge service token the matrix sends"

# Negative control: the same checker refuses a service token that carries the rerank scope as well, so a
# wider token cannot pass as the bearer of the scope-less row. Its output names problems, never the token.
MATRIX_TEST_TOKEN=$(JWT_ISSUER="$ISSUER" JWT_ORG_ID="matrix-own-org" JWT_SCOPES=data:read,data:rerank \
  node "$FIXTURE" mint-service "$KEYS") || fail "the fixture did not mint a wider service bearer"
export MATRIX_TEST_TOKEN
if node "$TMP/verify-service-bearer.cjs" "$KEYS/public.pem" "$ISSUER" matrix-own-org 2>"$TMP/wide.err"; then
  fail "the service bearer checker accepted a token with a wider scope"
fi
grep -Fq 'scopes' "$TMP/wide.err" || fail "the wider scope was refused for another reason"
if grep -Fq "$MATRIX_TEST_TOKEN" "$TMP/wide.err"; then
  fail "the service bearer checker printed the token"
fi
unset MATRIX_TEST_TOKEN

# The paid-call bearer (runbook section 12, step 3) is minted with JWT_ZDR=false: a signed zdr claim skips every
# rerank call. The claim follows JWT_ZDR, and the default stays zdr true, as for the user bearers. Only the claim
# value is printed, never a token.
zdr_claim_of() {
  MATRIX_TEST_TOKEN="$1" node -e 'const payload = process.env.MATRIX_TEST_TOKEN.split(".")[1]; process.stdout.write(String(JSON.parse(Buffer.from(payload, "base64url").toString("utf8")).zdr));'
}
paid_bearer=$(JWT_ISSUER="$ISSUER" JWT_ORG_ID=matrix-own-org JWT_SCOPES=data:read JWT_ZDR=false \
  node "$FIXTURE" mint-service "$KEYS") || fail "the fixture did not mint a non-ZDR service bearer"
[ "$(zdr_claim_of "$paid_bearer")" = "false" ] || fail "mint-service did not carry JWT_ZDR=false into the zdr claim"
default_bearer=$(JWT_ISSUER="$ISSUER" JWT_ORG_ID=matrix-own-org JWT_SCOPES=data:read \
  node "$FIXTURE" mint-service "$KEYS") || fail "the fixture did not mint the default service bearer"
[ "$(zdr_claim_of "$default_bearer")" = "true" ] || fail "mint-service default is not zdr true"
unset paid_bearer default_bearer

printf 'PASS: auth regression matrix contract\n'
