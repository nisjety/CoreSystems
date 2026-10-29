#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
PROOF="$ROOT_DIR/scripts/tests/coresystem-qm-proof.sh"

test -x "$PROOF"
help="$("$PROOF" --help)"
grep -F 'source/disposable' <<<"$help" >/dev/null
grep -F 'never creates, rotates, or prints credential values' <<<"$help" >/dev/null

# Keep the aggregate proof explicit. A future change that silently drops one
# of the four objective lanes must fail this contract before it can look like a
# whole-stack proof.
for child in \
  capability-health-proof.sh \
  scheduled-step-proof.sh \
  approval-continuation-proof.sh \
  approval-continuation-proof-test.sh \
  tickets-create-proof.sh \
  customer-proof-e2e-test.sh \
  scheduled-step-config-contract-test.sh \
  tickets-create-config-contract-test.sh \
  application-delivery-proof.sh \
  application-delivery-proof-test.sh \
  coresystem-conformance-artifact-test.sh \
  coresystem-cross-plane-preflight-test.sh; do
  grep -F "$child" "$PROOF" >/dev/null
done

grep -F 'coresystem-acceptance-trace-test.sh' "$PROOF" >/dev/null

grep -F 'source_only' "$PROOF" >/dev/null
grep -F 'candidate evidence' "$PROOF" >/dev/null
printf 'CoreSystem QM proof runner contract: ok\n'
