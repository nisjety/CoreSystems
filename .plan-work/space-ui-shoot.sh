#!/usr/bin/env bash
# Restart the worktree dev server on :5174 (its file watcher misses edits on this path), then take the Space UI shots.
# usage: space-ui-shoot.sh <outDirName>
ROOT="C:/dev/CoresSystem/.claude/worktrees/practical-mccarthy-1dc567"
SPA="$ROOT/apps/Frontend Plane/verevonv3"
OUT="C:/Users/IMAFER~1/AppData/Local/Temp/claude/C--dev-CoresSystem--claude-worktrees-practical-mccarthy-1dc567/98a6fa7c-4507-4864-9c20-d81fc3ffb9de/scratchpad/$1"
powershell -NoProfile -Command '$c = Get-NetTCPConnection -LocalPort 5174 -State Listen -ErrorAction SilentlyContinue | Select-Object -First 1; if ($c) { Stop-Process -Id $c.OwningProcess -Force }' >/dev/null 2>&1
cd "$SPA" || exit 1
(pnpm dev --port 5174 --strictPort > /tmp/space-ui-vite.log 2>&1 &)
for i in $(seq 1 40); do curl -s -o /dev/null http://localhost:5174/ && break; sleep 1; done
timeout 200 node tests/visual/space-ui-shots.mjs "$OUT" 2>&1 | grep -v "Failed to load resource"
