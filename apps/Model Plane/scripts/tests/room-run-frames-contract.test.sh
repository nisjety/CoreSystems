#!/usr/bin/env bash
# Drift check for the Space room run frame golden (WS09-S3).
#
#   * the golden's bytes match the .sha256 pin beside it
#   * the vendored SPA copy (and its pin) is byte-identical to the golden
#   * the golden is valid JSON and carries the closed class list
#
# Run from the repository root. LF files, compared byte for byte.
#
# Usage:
#   "apps/Model Plane/scripts/tests/room-run-frames-contract.test.sh"              run the checks
#   "apps/Model Plane/scripts/tests/room-run-frames-contract.test.sh" --self-test  prove the checks fail on a drifted copy
set -Eeuo pipefail

SELF="${BASH_SOURCE[0]}"
SCRIPT_DIR="$(cd "$(dirname "$SELF")" && pwd)"
# CONTRACT_SYNC_ROOT lets the self-test point the same checks at a scratch tree.
ROOT="${CONTRACT_SYNC_ROOT:-$(cd "$SCRIPT_DIR/../../../.." && pwd)}"

GOLDEN="apps/Model Plane/api/contracts/space-room-run-frames.v1.json"
SPA_COPY="apps/Frontend Plane/verevonv3/src/shared/api/contracts/space-room-run-frames.v1.json"

failures=0
pass() { printf 'ok - %s\n' "$1"; }
fail() { printf 'not ok - %s\n' "$1" >&2; failures=$((failures + 1)); }

sha256_of() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | cut -d' ' -f1
  else
    shasum -a 256 "$1" | cut -d' ' -f1
  fi
}

run_checks() {
  local golden="$ROOT/$GOLDEN" copy="$ROOT/$SPA_COPY"
  for f in "$golden" "$golden.sha256" "$copy" "$copy.sha256"; do
    [[ -f "$f" ]] || { fail "missing file: ${f#"$ROOT"/}"; return; }
  done

  local pinned actual
  pinned="$(cut -d' ' -f1 "$golden.sha256")"
  actual="$(sha256_of "$golden")"
  if [[ "$pinned" == "$actual" ]]; then pass "golden matches its pin"; else fail "golden differs from its pin ($actual vs $pinned)"; fi

  if cmp -s "$golden" "$copy"; then pass "SPA copy is identical to the golden"; else fail "SPA copy differs from the golden"; fi
  if cmp -s "$golden.sha256" "$copy.sha256"; then pass "SPA pin is identical to the golden pin"; else fail "SPA pin differs from the golden pin"; fi

  if command -v node >/dev/null 2>&1; then
    if node -e '
      const g = JSON.parse(require("fs").readFileSync(process.argv[1], "utf8"));
      const want = ["search_knowledge","web","code","artifact","lookup","effect","memory","delegate","plan","check","tool"];
      if (JSON.stringify(g.classes) !== JSON.stringify(want)) process.exit(1);
      if (g.default_for_unnamed_source_event !== "drop") process.exit(1);
    ' "$golden"; then pass "golden is valid JSON with the closed class list and default deny"; else fail "golden class list or default is wrong"; fi
  else
    pass "node not available; JSON shape check skipped"
  fi
}

if [[ "${1:-}" == "--self-test" ]]; then
  scratch="$(mktemp -d)"
  trap 'rm -rf "$scratch"' EXIT
  for rel in "$GOLDEN" "$GOLDEN.sha256" "$SPA_COPY" "$SPA_COPY.sha256"; do
    mkdir -p "$scratch/$(dirname "$rel")"
    cp "$ROOT/$rel" "$scratch/$rel"
  done
  printf ' ' >> "$scratch/$SPA_COPY"
  if CONTRACT_SYNC_ROOT="$scratch" bash "$SELF" >/dev/null 2>&1; then
    echo "not ok - the check passed on a drifted copy" >&2
    exit 1
  fi
  echo "ok - the check fails on a drifted copy"
  exit 0
fi

run_checks
[[ "$failures" -eq 0 ]]
