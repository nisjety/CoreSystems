#!/usr/bin/env bash
# DB tier for the Go Control services (user-core, audit-core) against the THROWAWAY Postgres of the proof.
set -u
ROOT="C:/dev/CoresSystem/.claude/worktrees/practical-mccarthy-1dc567"
LOG="$ROOT/.plan-work/space-proof/db-go2.log"
: > "$LOG"
for svc in user-core; do
  db="${svc//-/_}_test2"
  docker exec space-proof-pg psql -U postgres -d postgres -c "CREATE DATABASE $db" >> "$LOG" 2>&1
  echo "== $svc start $(date +%H:%M:%S)" >> "$LOG"
  ( cd "$ROOT/apps/Control Plane/$svc" && TEST_DATABASE_URL="postgres://postgres:sp_test_pw@127.0.0.1:55432/$db?sslmode=disable" timeout 1500 go test ./... -p 1 -count=1 -timeout 25m >> "$LOG" 2>&1 )
  echo "== $svc exit $? $(date +%H:%M:%S)" >> "$LOG"
done
echo "ALL DONE" >> "$LOG"
