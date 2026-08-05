#!/usr/bin/env bash
# Verevon typecheck: runs `tsc --noEmit` and ignores type errors that
# originate inside third-party `@blocksuite/...@0.19.5` package source files
# (G12). Those packages ship a broken `dist/index.d.ts` that re-exports from
# `../src/*.ts`, dragging unfixable third-party source into every type-check.
# `next build` already routes around this via `typescript.ignoreBuildErrors`;
# this script preserves the same signal for CI without losing visibility into
# errors in our own code.
#
# Exits 0 iff `tsc` produced no errors outside of the `@blocksuite` packages.

set -uo pipefail
cd "$(dirname "$0")/.."

OUT=$(mktemp)
trap 'rm -f "$OUT"' EXIT

npx tsc --noEmit 2>&1 > "$OUT"

# Strip @blocksuite noise and check what remains.
FILTERED=$(grep -E 'error TS' "$OUT" \
  | grep -v 'node_modules/@blocksuite/' \
  | grep -v 'node_modules/\.pnpm/@blocksuite' || true)

if [ -n "$FILTERED" ]; then
  echo "$FILTERED"
  echo
  echo "typecheck failed: $(echo "$FILTERED" | wc -l | tr -d ' ') error(s) outside @blocksuite"
  exit 1
fi

BLOCKSUITE_NOISE=$(grep -cE 'error TS' "$OUT" || echo 0)
if [ "$BLOCKSUITE_NOISE" -gt 0 ]; then
  echo "typecheck: 0 errors in verevon source ($BLOCKSUITE_NOISE @blocksuite third-party errors ignored — see G12)"
fi
