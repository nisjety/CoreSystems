#!/usr/bin/env bash
set -euo pipefail
set +x

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
SCRIPT="$ROOT/scripts/smoke-test.sh"
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT

fail() {
  printf 'FAIL: %s\n' "$1" >&2
  exit 1
}

mkdir -p "$TMP/bin"
cat >"$TMP/bin/docker" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
[ "${1:-}" = "compose" ] || exit 91
[ "${2:-}" = "exec" ] || exit 92
[ "${3:-}" = "-T" ] || exit 93
service=${4:-}
[ "${5:-}" = "curl" ] || exit 94
[ "${6:-}" = "--config" ] || exit 95
[ "${7:-}" = "-" ] || exit 96
config=$(cat)
printf '%s\n' "$service" >>"$FAKE_DOCKER_LOG"
printf '%s' "$config" | grep -Fq 'Authorization:' && exit 97
printf '200'
EOF
chmod +x "$TMP/bin/docker"

: >"$TMP/docker.log"
PATH="$TMP/bin:$PATH" FAKE_DOCKER_LOG="$TMP/docker.log" "$SCRIPT" >"$TMP/out" 2>&1 || {
  sed -n '1,160p' "$TMP/out" >&2
  fail "safe health smoke test failed"
}

grep -Fq '14 passed, 0 failed' "$TMP/out" || fail "smoke test did not cover health and readiness for seven services"
[ "$(wc -l <"$TMP/docker.log" | tr -d ' ')" = "14" ] || fail "smoke test used an unexpected request count"

if grep -Eq 'INTERNAL_API_KEY|x-api-key|CreateDocument|DeleteDocument|POST /v1/documents|X-Org-ID: smoke|localhost:80(10|11|12|13|14)' "$SCRIPT"; then
  fail "smoke test still contains shared-key, mutation, tenant, or published-port assumptions"
fi

printf 'PASS: safe smoke test contract\n'
