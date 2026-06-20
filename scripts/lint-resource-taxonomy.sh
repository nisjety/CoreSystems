#!/usr/bin/env bash
# lint-resource-taxonomy.sh — enforce the ownable-vs-team resource taxonomy as a
# single cross-language source of truth (Per-User Data Ownership & Sharing phase).
#
# The canonical lists live in user-core's resource_taxonomy.json (Go embeds it).
# The retrieval-engine Rust mirror (added in PR-3) MUST list the same types, or a
# resource could be "ownable" in one plane and "team-shared" in another — exactly
# the kind of split-brain that ships a false privacy guarantee. This lint fails CI
# on any drift. Until the Rust mirror exists it validates the JSON and passes.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
JSON="$ROOT/apps/Control Plane/user-core/internal/authztaxonomy/resource_taxonomy.json"
RUST="$ROOT/apps/Data Plane v2/services/retrieval-engine-rs/src/authz/taxonomy.rs"

if [[ ! -f "$JSON" ]]; then
  echo "FAIL: canonical taxonomy JSON missing: $JSON" >&2
  exit 1
fi

PY="$(command -v python3 || command -v python || true)"
if [[ -z "$PY" ]]; then
  echo "FAIL: python3 required for taxonomy lint" >&2
  exit 1
fi

"$PY" - "$JSON" "$RUST" <<'PYEOF'
import json, re, sys

json_path, rust_path = sys.argv[1], sys.argv[2]

with open(json_path) as f:
    data = json.load(f)

ownable = set(data.get("ownable", []))
team = set(data.get("team_shared", []))

if not ownable:
    print("FAIL: ownable set is empty in canonical JSON", file=sys.stderr); sys.exit(1)
if not team:
    print("FAIL: team_shared set is empty in canonical JSON", file=sys.stderr); sys.exit(1)
overlap = ownable & team
if overlap:
    print(f"FAIL: types in BOTH sets: {sorted(overlap)}", file=sys.stderr); sys.exit(1)

print(f"canonical JSON OK: {len(ownable)} ownable, {len(team)} team_shared")

import os
if not os.path.exists(rust_path):
    print(f"NOTE: Rust mirror not present yet ({rust_path}); parity check deferred to PR-3.")
    sys.exit(0)

with open(rust_path) as f:
    rust = f.read()

def extract(const_name):
    m = re.search(const_name + r"\s*:\s*&\[&str\]\s*=\s*&\[(.*?)\];", rust, re.S)
    if not m:
        print(f"FAIL: could not find {const_name} in Rust mirror", file=sys.stderr); sys.exit(1)
    return set(re.findall(r'"([^"]+)"', m.group(1)))

r_own = extract("OWNABLE")
r_team = extract("TEAM_SHARED")

ok = True
if r_own != ownable:
    print(f"FAIL: OWNABLE drift. JSON={sorted(ownable)} Rust={sorted(r_own)}", file=sys.stderr); ok = False
if r_team != team:
    print(f"FAIL: TEAM_SHARED drift. JSON={sorted(team)} Rust={sorted(r_team)}", file=sys.stderr); ok = False
if not ok:
    sys.exit(1)
print("Go/Rust taxonomy parity OK")
PYEOF
