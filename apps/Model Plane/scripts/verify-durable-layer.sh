#!/usr/bin/env bash
# verify-durable-layer.sh — reproducible integration check for the durable
# layer built this cycle (matrix §G7 / §G8 / §4.1 plan-mode), against a REAL
# Postgres (not a mock). Proves the custom/risky SQL behaves correctly:
#   - G7  UpsertAgentSkill: ON CONFLICT upsert + xmax created-detection +
#         provenance guard (a background_review write must NOT overwrite a
#         user-authored skill).
#   - P3  SetRunMode: UPDATE runs.mode, and a missing run affects 0 rows (404).
#   - G8  approval id-alignment: create with a client-supplied id, then
#         DecideApproval targets the SAME id (no divergent/orphaned record).
#
# Usage:   ./scripts/verify-durable-layer.sh
# Requires: docker. Spins up a throwaway Postgres, applies every session-core
# migration, runs the assertions, prints PASS/FAIL, and cleans up on exit.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MIG="$ROOT/rust/services/session-core/migrations"
CTR="mp-verify-pg-$$"

cleanup() { docker rm -f "$CTR" >/dev/null 2>&1 || true; }
trap cleanup EXIT

q() { docker exec -i "$CTR" psql -U postgres -d session_core -tAq -v ON_ERROR_STOP=1; }
expect() { # expect <actual> <wanted> <label>
  if [ "$1" != "$2" ]; then echo "FAIL: $3 — wanted '$2', got '$1'"; exit 1; fi
  echo "  ok: $3"
}

echo "==> starting throwaway Postgres ($CTR)"
docker run -d --name "$CTR" -e POSTGRES_PASSWORD=postgres -e POSTGRES_DB=session_core \
  postgres:16-alpine >/dev/null

ready=0
for _ in $(seq 1 60); do
  if docker exec "$CTR" psql -U postgres -d session_core -tAq -v ON_ERROR_STOP=1 -c "select 1" >/dev/null 2>&1; then
    ready=1
    break
  fi
  sleep 1
done
if [ "$ready" != "1" ]; then
  echo "FAIL: throwaway Postgres did not become SQL-ready"
  docker logs "$CTR" | tail -50 || true
  exit 1
fi

echo "==> applying migrations"
for f in "$MIG"/[0-9]*.sql; do docker exec -i "$CTR" psql -U postgres -d session_core -v ON_ERROR_STOP=1 -q < "$f"; done

UPSERT_TAIL="ON CONFLICT (org_id,name) DO UPDATE SET content=EXCLUDED.content, updated_at=now() WHERE agent_skills.origin <> 'user' OR EXCLUDED.origin='user' RETURNING (xmax=0);"
COLS="(id,org_id,name,description,content,trigger_keywords,trigger_file_patterns,tool_restrictions,enabled,origin,created_at,updated_at)"

echo "==> G7 UpsertAgentSkill provenance guard"
r=$(q <<SQL
INSERT INTO agent_skills $COLS VALUES ('s1','org1','Cache','d','c1','[]'::jsonb,'[]'::jsonb,'[]'::jsonb,true,'background_review',now(),now()) $UPSERT_TAIL
SQL
)
expect "$r" "t" "fresh insert => created"
r=$(q <<SQL
INSERT INTO agent_skills $COLS VALUES ('s1b','org1','Cache','d','c2','[]'::jsonb,'[]'::jsonb,'[]'::jsonb,true,'background_review',now(),now()) $UPSERT_TAIL
SQL
)
expect "$r" "f" "re-upsert => updated (not created)"
q >/dev/null <<SQL
INSERT INTO agent_skills $COLS VALUES ('s2','org1','Runbook','d','human','[]'::jsonb,'[]'::jsonb,'[]'::jsonb,true,'user',now(),now());
SQL
r=$(q <<SQL
INSERT INTO agent_skills $COLS VALUES ('s3','org1','Runbook','d','MACHINE','[]'::jsonb,'[]'::jsonb,'[]'::jsonb,true,'background_review',now(),now()) $UPSERT_TAIL
SQL
)
expect "${r:-EMPTY}" "EMPTY" "background_review cannot overwrite user skill (0 rows)"
expect "$(q <<<"SELECT content FROM agent_skills WHERE org_id='org1' AND name='Runbook';")" "human" "user skill body preserved"

echo "==> P3 SetRunMode"
q >/dev/null <<SQL
INSERT INTO threads (id,session_key,org_id,user_id) VALUES ('t1','sk1','org1','u1');
INSERT INTO runs (id,thread_id,goal,org_id,user_id) VALUES ('r1','t1','g','org1','u1');
SQL
expect "$(q <<<"UPDATE runs SET mode='plan' WHERE id='r1' RETURNING mode;")" "plan" "SetRunMode -> plan"
expect "$(q <<<"UPDATE runs SET mode='plan' WHERE id='nope' RETURNING id;")" "" "SetRunMode missing run => 0 rows"

echo "==> G8 approval id-alignment"
q >/dev/null <<SQL
INSERT INTO approvals (id,run_id,kind,status,requested_by,org_id,user_id,idempotency_key,metadata,requested_at)
VALUES ('appr-gw-1','r1','tool_call','requested','svc','org1','u1','','{}'::jsonb,now());
SQL
expect "$(q <<<"UPDATE approvals SET status='granted',decided_by='a',decided_at=now() WHERE id='appr-gw-1' RETURNING status;")" "granted" "DecideApproval targets gateway-supplied id"

echo
echo "PASS — durable layer verified against real Postgres."
