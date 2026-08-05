#!/usr/bin/env bash
set -euo pipefail

gateway_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
report="$(mktemp "${TMPDIR:-/tmp}/verevon-gateway-security-coverage.XXXXXX")"
trap 'rm -f "$report"' EXIT

if ! cargo llvm-cov --version >/dev/null 2>&1; then
  echo "cargo-llvm-cov is required; install it outside this script before running the gate" >&2
  exit 2
fi

cd "$gateway_root"
cargo llvm-cov --locked --all-targets --summary-only --json --output-path "$report"
node scripts/security-coverage-gate.mjs \
  "$report" \
  "$gateway_root" \
  80 \
  src/audience_tokens.rs \
  src/config.rs \
  src/domains/orgs/members.rs \
  src/middleware.rs \
  src/upstream.rs
