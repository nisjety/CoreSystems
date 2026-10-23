#!/usr/bin/env bash
# ws4a-gateway-clippy-triage.sh: WS4.A "clippy triage against HEAD" (plan slice WS4.A,
# Definition of done: "All tests green including clippy -D warnings").
#
# An earlier round recorded the gateway's clippy red in 17 unrelated places (not re-run
# since). This runs the CI command locally, `cargo clippy --all-targets -- -D warnings`
# (verevonv3-ci.yml job Gateway), and sorts every finding into two groups:
#   OWNED   a file the WS4 slices (A, C, D, E, F, G) wrote or changed in the gateway; a finding
#           here is a defect of the slice and must be fixed;
#   OTHER   everything else; the triage list for its owner (recorded, not fixed here).
# It prints counts only (per lint, per file); never a source snippet.
#
# Expected result, written down BEFORE the run so the run can disagree with it:
#   * the command's exit status is 0 on a clean tree (CI requires it);
#   * if it is not 0: OWNED=0, and every OTHER finding sits in a file no WS4 slice touched;
#     the earlier "17 places" is a count to compare with, not a promise.
# The script exits 0 when OWNED=0 (OTHER findings are reported, not failed), 1 when OWNED>0,
# 2 when clippy could not run.
#
# Native on the host (no docker, no stack). Needs cargo, clippy and node. It builds in the
# crate's own target directory, so run it when no other build is using it.
#
# Usage:
#   scripts/evidence/ws4a-gateway-clippy-triage.sh
#   scripts/evidence/ws4a-gateway-clippy-triage.sh --self-test    # the classifier on canned JSON
set -uo pipefail

HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
GATEWAY_DIR="$HERE/../../apps/gateway"

# Gateway files the WS4 slices own (paths relative to apps/gateway, forward slashes).
OWNED_FILES='src/cache.rs src/cache_admission.rs src/zdr.rs src/zdr_log.rs src/observability.rs src/gdpr_nats.rs src/domains/search.rs src/domains/knowledge/quarry.rs src/domains/knowledge/products.rs tests/cache_admission_only.rs tests/log_field_discipline.rs'

summarise() { # <jsonl file>
  OWNED_FILES="$OWNED_FILES" node -e '
    const fs = require("fs");
    const owned = new Set(process.env.OWNED_FILES.split(/\s+/).filter(Boolean));
    const byLint = {}, byFile = {};
    let ownedCount = 0, otherCount = 0;
    for (const line of fs.readFileSync(process.argv[1], "utf8").split("\n")) {
      if (!line.startsWith("{")) continue;
      let m; try { m = JSON.parse(line); } catch { continue; }
      if (m.reason !== "compiler-message" || !m.message) continue;
      const msg = m.message;
      if (msg.level !== "warning" && msg.level !== "error") continue;
      const code = msg.code && msg.code.code ? msg.code.code : null;
      if (!code) continue;                       // "N warnings emitted" and the like
      const span = (msg.spans || []).find((s) => s.is_primary) || (msg.spans || [])[0];
      const file = span ? span.file_name.replace(/\\/g, "/") : "(no span)";
      byLint[code] = (byLint[code] || 0) + 1;
      byFile[file] = (byFile[file] || 0) + 1;
      if (owned.has(file)) ownedCount++; else otherCount++;
    }
    const sorted = (o) => Object.entries(o).sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]));
    console.log("findings by lint:");
    for (const [k, v] of sorted(byLint)) console.log("  " + String(v).padStart(4) + "  " + k);
    console.log("findings by file:");
    for (const [k, v] of sorted(byFile)) console.log("  " + String(v).padStart(4) + "  " + k + (owned.has(k) ? "   [OWNED]" : ""));
    console.log("OWNED=" + ownedCount + " OTHER=" + otherCount);
    process.exit(ownedCount > 0 ? 1 : 0);
  ' "$1"
}

if [ "${1:-}" = "--self-test" ]; then
  tmp=$(mktemp)
  cat >"$tmp" <<'EOF'
{"reason":"compiler-message","message":{"level":"warning","code":{"code":"clippy::needless_return"},"spans":[{"is_primary":true,"file_name":"src\\domains\\browser.rs"}]}}
{"reason":"compiler-message","message":{"level":"error","code":{"code":"clippy::too_many_arguments"},"spans":[{"is_primary":true,"file_name":"src/cache.rs"}]}}
{"reason":"compiler-message","message":{"level":"warning","code":null,"spans":[]}}
{"reason":"compiler-artifact"}
EOF
  out=$(summarise "$tmp"); rc=$?
  echo "$out"
  echo "$out" | grep -q "OWNED=1 OTHER=1" && [ "$rc" -eq 1 ] && echo "self-test OK" || { echo "self-test FAILED"; rm -f "$tmp"; exit 1; }
  rm -f "$tmp"
  exit 0
fi

command -v cargo >/dev/null || { echo "cargo is required" >&2; exit 2; }
cargo clippy --version >/dev/null 2>&1 || { echo "clippy is not installed (rustup component add clippy)" >&2; exit 2; }
command -v node >/dev/null || { echo "node is required" >&2; exit 2; }

out=$(mktemp)
echo "revision: $(git -C "$GATEWAY_DIR" rev-parse --short HEAD 2>/dev/null || echo unknown)  (uncommitted changes in apps/gateway: $(git -C "$GATEWAY_DIR" status --porcelain -- . 2>/dev/null | wc -l | tr -d ' ') files)"
(cd "$GATEWAY_DIR" && cargo clippy --all-targets --message-format=json -- -D warnings >"$out" 2>/dev/null)
clippy_rc=$?
echo "cargo clippy --all-targets -- -D warnings: exit status $clippy_rc"
summary=$(summarise "$out")
rc=$?
echo "$summary"
rm -f "$out"
if [ "$clippy_rc" -ne 0 ] && echo "$summary" | grep -q "OWNED=0 OTHER=0"; then
  echo "clippy failed without a lint finding (a compile error?): nothing to triage" >&2
  exit 2
fi
if [ "$clippy_rc" -eq 0 ] && [ "$rc" -eq 0 ]; then
  echo "PASS clippy clean"
elif [ "$rc" -eq 0 ]; then
  echo "TRIAGE clippy is red outside the WS4 files only: list the OTHER files above for their owners (not fixed here)"
else
  echo "FAIL a WS4-owned file has a clippy finding"
fi
exit "$rc"
