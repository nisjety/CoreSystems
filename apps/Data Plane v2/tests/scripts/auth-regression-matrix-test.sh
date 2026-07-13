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

: >"$TMP/docker.log"
run_matrix >"$TMP/matrix.out" 2>&1 || {
  sed -n '1,160p' "$TMP/matrix.out" >&2
  fail "matrix rejected the safe four-shape fixture"
}

grep -Fq '28 passed, 0 failed' "$TMP/matrix.out" || fail "matrix did not execute 7 x 4 assertions"
[ "$(wc -l <"$TMP/docker.log" | tr -d ' ')" = "28" ] || fail "unexpected transport request count"
for service in graph-index data-quality data-orchestrator documents-api retrieval-engine wiki-store quickwit-adapter; do
  [ "$(grep -Fxc "$service" "$TMP/docker.log")" = "4" ] || fail "$service was not covered four times"
done

: >"$TMP/docker.log"
DPV2_COMPOSE_PROJECT="isolated-matrix" run_matrix >"$TMP/project-matrix.out" 2>&1 || fail "matrix rejected an explicit isolated Compose project"
[ "$(grep -c '^project=isolated-matrix$' "$TMP/docker.log")" = "28" ] || fail "matrix ignored the isolated Compose project"

# Never echo disposable identifiers or credentials, even on success.
for secret in matrix-own-org matrix-spoof-org header.payload.signature quality.payload.signature admin.payload.signature; do
  if grep -Fq "$secret" "$TMP/matrix.out"; then
    fail "matrix output exposed a credential or organization identifier"
  fi
done

# External endpoints are explicit and restricted to loopback for this local-only harness.
if DPV2_MATRIX_TRANSPORT=external \
  DPV2_TEST_ORG_ID="matrix-own-org" \
  DPV2_SPOOF_ORG_ID="matrix-spoof-org" \
  DPV2_USER_BEARER="header.payload.signature" \
  DPV2_QUALITY_BEARER="quality.payload.signature" \
  DPV2_QUICKWIT_ADMIN_BEARER="admin.payload.signature" \
  DPV2_GRAPH_URL="https://production.invalid" \
  "$SCRIPT" >"$TMP/external.out" 2>&1; then
  fail "matrix accepted a non-loopback external endpoint"
fi

# The harness must not contain destructive route families or mutation flags.
if grep -Eq 'cleanup/orphans|bulk delete|index reset|purge|\"global\":true|\"clear\":true|\"break_glass\":true' "$SCRIPT"; then
  fail "matrix contains a destructive operation"
fi

printf 'PASS: auth regression matrix contract\n'
