#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
PROOF="$ROOT_DIR/scripts/tests/application-delivery-proof.sh"

test -x "$PROOF"
source_text="$(sed -n '1,260p' "$PROOF")"
grep -F 'docker create --pull=never' <<<"$source_text" >/dev/null
grep -F 'NOTIFICATION_CORE_ALLOW_DB_MUTATION=1' <<<"$source_text" >/dev/null
grep -F 'TestDeliveryAttemptsLeaseAndReceiptAgainstRealPostgres' <<<"$source_text" >/dev/null

# The proof must never target a running CoreSystem database or consume a
# deployment-provided URL. Its only database is the short-lived local
# container created by the proof itself.
if rg -n 'model-plane-postgres|verevon-proof-pg|DATABASE_URL="\$|CONTROL_.*TOKEN|EXECUTION_CORE_.*TOKEN' "$PROOF" >/dev/null; then
  echo 'application delivery proof must remain disposable and credential-free' >&2
  exit 1
fi

printf 'application delivery proof runner contract: ok\n'
