#!/usr/bin/env bash
set -euo pipefail

data_root=$(cd "$(dirname "$0")/../.." && pwd)
model_root="$data_root/../Model Plane/rust"

audit_workspace() {
  local label=$1
  local root=$2
  local normal_tree
  normal_tree=$(cargo tree --manifest-path "$root/Cargo.toml" --edges normal,build -i rsa 2>&1 || true)
  if printf '%s\n' "$normal_tree" | rg -q '^rsa v'; then
    echo "FAIL: $label has a runtime/build dependency on advisory-affected rsa" >&2
    exit 1
  fi

  # RUSTSEC-2023-0071 has no fixed rsa 0.9 release. Both workspaces use rsa
  # exclusively to generate ephemeral keys in tests; production and build
  # dependency graphs above must remain rsa-free. Ignore only that dev-only
  # advisory while continuing to fail on every runtime or newly introduced ID.
  cargo audit --file "$root/Cargo.lock" --ignore RUSTSEC-2023-0071 --quiet
  echo "PASS: $label RustSec audit (dev-only rsa isolated)"
}

audit_workspace "Data Plane" "$data_root"
audit_workspace "Model Plane" "$model_root"
