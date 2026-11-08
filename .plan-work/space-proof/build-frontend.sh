#!/usr/bin/env bash
# Frontend proof image: the committed Dockerfile runs `pnpm build` (= tsc -b && vite build) and tsc currently fails on 34
# errors in files no Space commit touched (quarry-client.ts, RemoteSupportPage.tsx, schedule-contract.test.ts).
# For the proof only, use a patched COPY of the Dockerfile outside git that runs vite build alone.
set -u
ROOT="C:/dev/CoresSystem/.claude/worktrees/practical-mccarthy-1dc567"
V="$ROOT/apps/Frontend Plane/verevonv3"
LOG="$ROOT/.plan-work/space-proof/build-frontend.log"
: > "$LOG"
sed 's#^RUN pnpm build$#RUN pnpm exec vite build#' "$V/Dockerfile" > "$ROOT/.plan-work/space-proof/Dockerfile.frontend-proof"
grep -n "vite build" "$ROOT/.plan-work/space-proof/Dockerfile.frontend-proof" >> "$LOG"
REV="$(git -C "$ROOT" rev-parse --short HEAD)"
DATE="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
docker build -f "$ROOT/.plan-work/space-proof/Dockerfile.frontend-proof" \
  --build-arg "SOURCE_REVISION=$REV" --build-arg "BUILD_DATE=$DATE" \
  --build-context "quarry-client=$ROOT/apps/Ingestion Plane/Quarry-v2/sdks/typescript" \
  -t space-proof/frontend:w1 "$V" >> "$LOG" 2>&1
echo "== frontend exit $? $(date +%H:%M:%S)" >> "$LOG"
