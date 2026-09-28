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
#
# `--proc /proc` was in this string until 2026-09-13 but has NOT been in the
# real argv for some time: with --unshare-pid it needs CAP_SYS_ADMIN, the
# container runs CapEff=0, and every sandboxed call failed — sandbox.rs
# removed it and pinned the removal with `the_argv_never_mounts_a_fresh_proc`.
# A verification script that checks an argv the service does not emit verifies
# nothing, so it is dropped here too. Its absence is also load-bearing for the
# process-signal probe below, which reads the HOST procfs.
BASE="--die-with-parent --unshare-pid --unshare-uts --unshare-ipc --new-session --dev /dev --tmpfs /tmp --ro-bind / /"

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

# ---------------------------------------------------------------------------
# S4.2 process-signal probe: can the host reach a BACKGROUND process's command?
#
# execution-core's process_host.rs has to deliver SIGTERM to the sandboxed
# command, and the command is two /proc hops below the process it spawned:
# our child is bwrap's monitor, which forks the sandbox init, which forks the
# command. This asserts the mechanism that module depends on, and the
# negative result that rules out the obvious alternative:
#
#   A. SIGTERM to the pid resolved by walking
#      /proc/<pid>/task/<pid>/children twice REACHES the command.
#   B. SIGTERM to the bwrap monitor does NOT reach it — so "just signal the
#      child we spawned" would silently never fire a trap.
#   C. SIGKILL to the monitor leaves nothing alive, which is what makes the
#      TERM->grace->KILL escalation's guarantee hold regardless of A.
docker run --rm -i --privileged debian:bookworm-slim sh -s <<SCRIPT 2>&1 | grep -E "^(VERIFY|ok:|FAIL:|PASS|ERROR)"
set -e
apt-get update -qq >/dev/null 2>&1
apt-get install -y -qq bubblewrap procps >/dev/null 2>&1
echo "VERIFY background-process signal delivery (S4.2 step 4)"

OUT=/tmp/probe.out
children_of() { cat "/proc/\$1/task/\$1/children" 2>/dev/null | tr -s ' ' '\n' | grep -E '^[0-9]+\$'; }
start_traced() {
  : > "\$OUT"
  bwrap $BASE -- sh -c 'trap "echo GOT_TERM; exit 0" TERM; echo READY; while :; do sleep 0.2; done' > "\$OUT" 2>&1 &
  MON=\$!
  for _ in \$(seq 1 50); do grep -q READY "\$OUT" && return 0; sleep 0.1; done
  echo "FAIL: sandboxed command never started"; exit 1
}
awaited() { for _ in \$(seq 1 30); do grep -q GOT_TERM "\$OUT" && return 0; sleep 0.1; done; return 1; }

start_traced
INIT=\$(children_of "\$MON" | head -1)
[ -n "\$INIT" ] || { echo "FAIL: monitor has no /proc children (CONFIG_PROC_CHILDREN off?)"; exit 1; }
CMD=\$(children_of "\$INIT" | head -1)
[ -n "\$CMD" ] || CMD="\$INIT"
kill -TERM "\$CMD" 2>/dev/null || true
awaited || { echo "FAIL: SIGTERM to the resolved command pid did not reach it"; exit 1; }
echo "ok: SIGTERM to the pid resolved through two /proc hops reaches the command"
kill -9 "\$MON" 2>/dev/null || true

start_traced
kill -TERM "\$MON" 2>/dev/null || true
if awaited; then
  echo "FAIL: signalling the monitor reached the command — process_host.rs resolves a pid it no longer needs to"
  exit 1
fi
echo "ok: SIGTERM to the bwrap monitor does NOT reach the command (so the walk is required)"
kill -9 "\$MON" 2>/dev/null || true

: > "\$OUT"
bwrap $BASE -- sh -c 'echo READY; while :; do sleep 0.2; done' > "\$OUT" 2>&1 &
MON=\$!
for _ in \$(seq 1 50); do grep -q READY "\$OUT" && break; sleep 0.1; done
INIT=\$(children_of "\$MON" | head -1)
CMD=\$(children_of "\${INIT:-0}" | head -1)
kill -9 "\$MON" 2>/dev/null || true
sleep 1
for p in "\$MON" "\${INIT:-0}" "\${CMD:-0}"; do
  [ "\$p" = "0" ] && continue
  [ -d "/proc/\$p" ] && { echo "FAIL: pid \$p survived SIGKILL to the monitor"; exit 1; }
done
echo "ok: SIGKILL to the monitor leaves nothing alive"

echo "PASS — background-process signal delivery behaves as process_host.rs assumes."
SCRIPT
