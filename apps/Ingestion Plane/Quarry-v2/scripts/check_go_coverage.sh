#!/usr/bin/env bash
set -euo pipefail

MIN_COVERAGE="${1:-80}"
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_DIR="$ROOT_DIR/target/coverage"
MERGED_PROFILE="$OUT_DIR/go-coverage.out"

MODULES=(
  "pkg/quarrycontracts"
  "services/quarry-control"
  "services/quarry-orchestrator"
)

mkdir -p "$OUT_DIR"
echo "mode: atomic" > "$MERGED_PROFILE"

for module in "${MODULES[@]}"; do
  tmp_profile="$(mktemp)"
  echo "Running Go coverage for $module"
  (
    cd "$ROOT_DIR/$module"
    go test ./... -covermode=atomic -coverprofile="$tmp_profile"
  )
  tail -n +2 "$tmp_profile" >> "$MERGED_PROFILE"
  rm -f "$tmp_profile"
done

total_pct="$(awk '
  NR == 1 { next }
  {
    stmts += $2
    if ($3 > 0) {
      covered += $2
    }
  }
  END {
    if (stmts == 0) {
      print ""
      exit
    }
    printf("%.2f", (covered / stmts) * 100)
  }
' "$MERGED_PROFILE")"

if [[ -z "$total_pct" ]]; then
  echo "Failed to compute Go coverage total from $MERGED_PROFILE"
  exit 1
fi

echo "Go total coverage: ${total_pct}%"

awk -v total="$total_pct" -v min="$MIN_COVERAGE" 'BEGIN {
  if (total + 0 < min + 0) {
    printf("Go coverage gate failed: %.2f%% < %.2f%%\n", total, min)
    exit 1
  }
}'

echo "Go coverage gate passed: ${total_pct}% >= ${MIN_COVERAGE}%"
