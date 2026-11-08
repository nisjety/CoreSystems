#!/usr/bin/env bash
# Build the W1-changed Model Plane Rust images from the space worktree under NEW tags that cannot overwrite any live tag.
# Sequential on purpose (Docker Desktop wedges under parallel cargo builds).
set -u
ROOT="C:/dev/CoresSystem/.claude/worktrees/practical-mccarthy-1dc567"
CTX="$ROOT/apps/Model Plane"
REV="$(git -C "$ROOT" rev-parse --short HEAD)"
DATE="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
LOG="$ROOT/.plan-work/space-proof/build-mp.log"
: > "$LOG"
for svc in session-core model-gateway execution-core; do
  echo "== build $svc $(date +%H:%M:%S)" >> "$LOG"
  docker build -f "$CTX/rust/services/$svc/Dockerfile" \
    --build-arg "SOURCE_REVISION=$REV" --build-arg "BUILD_DATE=$DATE" \
    -t "space-proof/$svc:w1" "$CTX" >> "$LOG" 2>&1
  echo "== $svc exit $? $(date +%H:%M:%S)" >> "$LOG"
done
echo "ALL DONE" >> "$LOG"
