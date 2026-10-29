#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
PROOF="$ROOT_DIR/scripts/tests/approval-continuation-proof.sh"

test -x "$PROOF"
proof_source="$(sed -n '1,220p' "$PROOF")"
grep -F 'disposable Postgres' <<<"$proof_source" >/dev/null
grep -F 'immutable start-receipt idempotency' <<<"$proof_source" >/dev/null
grep -F 'deployed signer' <<<"$proof_source" >/dev/null

# This helper must remain disposable-only and must not grow a path to the
# running databases or a credential-bearing deployment step.
if rg -n 'model-plane-postgres|verevon-proof-pg|EXECUTION_CORE_SERVICE_API_KEY|CONTROL_.*TOKEN' "$PROOF" >/dev/null; then
  echo 'approval proof must not target a running database or contain service credentials' >&2
  exit 1
fi

printf 'approval-continuation proof runner contract: ok\n'
