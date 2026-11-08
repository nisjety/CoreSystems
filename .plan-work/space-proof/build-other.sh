#!/usr/bin/env bash
# Build the remaining W1-changed images from the worktree under NEW tags (never retagging live images). Sequential.
set -u
ROOT="C:/dev/CoresSystem/.claude/worktrees/practical-mccarthy-1dc567"
REV="$(git -C "$ROOT" rev-parse --short HEAD)"
DATE="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
LOG="$ROOT/.plan-work/space-proof/build-other.log"
: > "$LOG"
build() { # name context
  echo "== build $1 $(date +%H:%M:%S)" >> "$LOG"
  docker build --build-arg "SOURCE_REVISION=$REV" --build-arg "BUILD_DATE=$DATE" -t "space-proof/$1:w1" "$2" >> "$LOG" 2>&1
  echo "== $1 exit $? $(date +%H:%M:%S)" >> "$LOG"
}
build user-core "$ROOT/apps/Control Plane/user-core"
build audit-core "$ROOT/apps/Control Plane/audit-core"
build auth-core "$ROOT/apps/Control Plane/auth-core"
build gateway "$ROOT/apps/Frontend Plane/verevonv3/apps/gateway"
build frontend "$ROOT/apps/Frontend Plane/verevonv3"
echo "ALL DONE" >> "$LOG"
