#!/usr/bin/env bash
# DB tier for session-core against the THROWAWAY Postgres/NATS of the proof (never the live stack).
set -u
ROOT="C:/dev/CoresSystem/.claude/worktrees/practical-mccarthy-1dc567"
LOG="$ROOT/.plan-work/space-proof/db-session-core.log"
cd "$ROOT/apps/Model Plane/rust" || exit 1
export DATABASE_URL="postgres://postgres:sp_test_pw@127.0.0.1:55432/space_test"
export NATS_URL="nats://127.0.0.1:54222"
export CARGO_TARGET_DIR=C:/dev/space-target-mp
export CARGO_INCREMENTAL=0
: > "$LOG"
echo "== start $(date +%H:%M:%S)" >> "$LOG"
cargo test -p session-core -- --ignored --test-threads=1 >> "$LOG" 2>&1
echo "== exit $? $(date +%H:%M:%S)" >> "$LOG"
