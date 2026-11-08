#!/usr/bin/env bash
# user-core and frontend need named additional build contexts (compose additional_contexts).
set -u
ROOT="C:/dev/CoresSystem/.claude/worktrees/practical-mccarthy-1dc567"
REV="$(git -C "$ROOT" rev-parse --short HEAD)"
DATE="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
LOG="$ROOT/.plan-work/space-proof/build-other2.log"
: > "$LOG"
echo "== build user-core $(date +%H:%M:%S)" >> "$LOG"
docker build --build-arg "SOURCE_REVISION=$REV" --build-arg "BUILD_DATE=$DATE" \
  --build-context "model-plane-go-gen=$ROOT/apps/Model Plane/go/gen" \
  -t space-proof/user-core:w1 "$ROOT/apps/Control Plane/user-core" >> "$LOG" 2>&1
echo "== user-core exit $? $(date +%H:%M:%S)" >> "$LOG"
echo "== build frontend $(date +%H:%M:%S)" >> "$LOG"
docker build --build-arg "SOURCE_REVISION=$REV" --build-arg "BUILD_DATE=$DATE" \
  --build-context "quarry-client=$ROOT/apps/Ingestion Plane/Quarry-v2/sdks/typescript" \
  -t space-proof/frontend:w1 "$ROOT/apps/Frontend Plane/verevonv3" >> "$LOG" 2>&1
echo "== frontend exit $? $(date +%H:%M:%S)" >> "$LOG"
echo "ALL DONE" >> "$LOG"
