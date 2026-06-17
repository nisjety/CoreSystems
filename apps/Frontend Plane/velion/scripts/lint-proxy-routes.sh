#!/usr/bin/env bash
# Lint: enforce ADR 0003 guardrail #1 — every proxy route under
# src/app/api/*/route.ts that does cross-plane fetch must route through
# src/app/api/_lib/control-plane-auth.ts (so session validation, internal
# auth headers, and correlation IDs are uniform).
#
# A violation is a route file that calls `fetch(...)` AND does not import
# from `_lib/control-plane-auth`. Routes that pre-date the helper are
# listed in `scripts/lint-proxy-routes.baseline` (a ratchet — remove an
# entry when you migrate a route). The lint fails ONLY for NEW violations
# that aren't already in the baseline, or for stale baseline entries that
# no longer violate (= someone migrated, baseline should shrink).
#
# Exempted classes (declared in EXEMPT_PATTERNS) target Better Auth's own
# proxy, OAuth flows, external integrations, and other genuinely
# non-session contracts. Add to the exemption list only with a comment
# justifying why.

set -uo pipefail
cd "$(dirname "$0")/.."

API_DIR="src/app/api"
HELPER_RE='from .*_lib/control-plane-auth'
BASELINE_FILE="scripts/lint-proxy-routes.baseline"

EXEMPT_PATTERNS=(
  '^src/app/api/auth/\[\.\.\.path\]/route\.ts$'
  '^src/app/api/auth/sign-in/'
  '^src/app/api/auth/consent/'
  '^src/app/api/convex-auth/'
  '^src/app/api/external/'
  '^src/app/api/sentry/'
  '^src/app/api/stripe/'
  '^src/app/api/webhooks/'
  '^src/app/api/health/'
  '^src/app/api/\.well-known/'
  # G25 telemetry sink — accepts events, doesn't forward to CP.
  '^src/app/api/telemetry/'
)

is_exempt() {
  local path="$1"
  for pat in "${EXEMPT_PATTERNS[@]}"; do
    if [[ "$path" =~ $pat ]]; then return 0; fi
  done
  return 1
}

TMP_CURRENT=$(mktemp)
TMP_BASELINE=$(mktemp)
TMP_NEW=$(mktemp)
TMP_STALE=$(mktemp)
trap 'rm -f "$TMP_CURRENT" "$TMP_BASELINE" "$TMP_NEW" "$TMP_STALE"' EXIT

# Compute current violations.
while IFS= read -r f; do
  if is_exempt "$f"; then continue; fi
  if ! grep -q "fetch(" "$f"; then continue; fi
  if grep -qE "$HELPER_RE" "$f"; then continue; fi
  echo "$f"
done < <(find "$API_DIR" -name "route.ts" -type f) | sort > "$TMP_CURRENT"

# Load baseline (filter blanks + comments).
if [ -f "$BASELINE_FILE" ]; then
  grep -vE '^\s*(#|$)' "$BASELINE_FILE" | sort > "$TMP_BASELINE"
else
  : > "$TMP_BASELINE"
fi

# New violations = current minus baseline.
comm -23 "$TMP_CURRENT" "$TMP_BASELINE" > "$TMP_NEW"
# Stale baseline entries = baseline minus current (migrated or deleted).
comm -13 "$TMP_CURRENT" "$TMP_BASELINE" > "$TMP_STALE"

exit_code=0

new_count=$(wc -l < "$TMP_NEW" | tr -d ' ')
if [ "$new_count" -gt 0 ]; then
  echo "lint-proxy-routes: $new_count NEW route(s) call fetch() without going through control-plane-auth.ts:"
  sed 's/^/  - /' "$TMP_NEW"
  echo
  echo "Fix: import requireSession + buildControlPlaneHeaders from"
  echo "     '../_lib/control-plane-auth' and forward through there."
  exit_code=1
fi

stale_count=$(wc -l < "$TMP_STALE" | tr -d ' ')
if [ "$stale_count" -gt 0 ]; then
  echo "lint-proxy-routes: $stale_count stale baseline entr(y/ies) — remove from $BASELINE_FILE:"
  sed 's/^/  - /' "$TMP_STALE"
  exit_code=1
fi

if [ $exit_code -eq 0 ]; then
  grand_count=$(wc -l < "$TMP_CURRENT" | tr -d ' ')
  if [ "$grand_count" -gt 0 ]; then
    echo "lint-proxy-routes: OK ($grand_count grandfathered route(s) pending migration — see $BASELINE_FILE)"
  else
    echo "lint-proxy-routes: OK (all routes use control-plane-auth helper)"
  fi
fi

exit $exit_code
