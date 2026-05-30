#!/usr/bin/env bash
# verify-sandbox-isolation.sh — verifies that the bubblewrap argv produced by
# execution-core/src/sandbox.rs (build_bwrap_argv) ACTUALLY isolates a process
# on real Linux + bwrap (matrix §G1). Run in a Linux container.
#
# It mirrors the exact argv build_bwrap_argv emits and asserts:
#   - ReadOnly{Disabled}: read-only root (writes to / fail) + no network.
#   - WorkspaceWrite:      a bound writable root accepts writes.
#
# CONSTRAINT (verified finding): bubblewrap needs unprivileged user namespaces
# (CLONE_NEWUSER). Docker's DEFAULT seccomp/userns profile BLOCKS this
# ("Creating new namespace failed: Operation not permitted"), so this check
# requires `--privileged` (or `--security-opt seccomp=unconfined` + userns
# enabled). The real executor (G1) must therefore run on a host that permits
# unprivileged userns, or hold the needed capabilities.
#
# Usage:   ./scripts/verify-sandbox-isolation.sh
# Requires: docker (will pull debian:bookworm-slim, run --privileged).
set -euo pipefail

# The shared base argv from build_bwrap_argv (assemble_argv), pre `-- <prog>`.
BASE="--die-with-parent --unshare-pid --unshare-uts --unshare-ipc --new-session --proc /proc --dev /dev --tmpfs /tmp --ro-bind / /"

docker run --rm -i --privileged debian:bookworm-slim sh -s <<SCRIPT 2>&1 | grep -E "^(VERIFY|ok:|FAIL:|PASS|ERROR)"
set -e
apt-get update -qq >/dev/null 2>&1
apt-get install -y -qq bubblewrap wget >/dev/null 2>&1
echo "VERIFY sandbox isolation (real Linux + bwrap)"

# ReadOnly{Disabled}: BASE + --unshare-net
if bwrap $BASE --unshare-net -- sh -c "echo x > /root/nope" 2>/dev/null; then
  echo "FAIL: read-only root allowed a write"; exit 1
fi
echo "ok: ReadOnly root blocks writes to /"

if bwrap $BASE --unshare-net -- sh -c "wget -T2 -q -O- http://1.1.1.1 >/dev/null 2>&1"; then
  echo "FAIL: --unshare-net allowed egress"; exit 1
fi
echo "ok: --unshare-net blocks network egress"

# WorkspaceWrite{[/work], Disabled}: BASE + --bind /work /work + --unshare-net
mkdir -p /work
out=\$(bwrap $BASE --bind /work /work --unshare-net -- sh -c "echo ok > /work/f && cat /work/f")
[ "\$out" = "ok" ] || { echo "FAIL: workspace-write bound root rejected a write (got '\$out')"; exit 1; }
echo "ok: WorkspaceWrite bound root accepts writes"

echo "PASS — bwrap argv from sandbox.rs isolates correctly on Linux."
SCRIPT
