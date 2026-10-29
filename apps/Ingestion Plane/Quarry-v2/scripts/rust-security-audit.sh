#!/usr/bin/env bash
set -euo pipefail

workspace_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$workspace_root"

normal_graph="$(mktemp)"
trap 'rm -f "$normal_graph"' EXIT

if cargo tree \
    --workspace \
    --all-features \
    --target all \
    --edges normal,build \
    --invert rsa@0.9.10 >"$normal_graph" 2>&1; then
    if grep -q '^rsa v0\.9\.10' "$normal_graph"; then
        echo "RUSTSEC-2023-0071 entered the normal/build dependency graph" >&2
        cat "$normal_graph" >&2
        exit 1
    fi
else
    if ! grep -Eq 'did not match any packages|nothing to print' "$normal_graph"; then
        cat "$normal_graph" >&2
        exit 1
    fi
fi

# rsa 0.9.10 is retained only in Cargo.lock through sqlx-mysql metadata; it
# has no fixed release and is absent from every workspace normal/build graph.
# cargo-audit still checks every other advisory without an allowlist.
cargo audit --ignore RUSTSEC-2023-0071
