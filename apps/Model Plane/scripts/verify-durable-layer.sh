#!/usr/bin/env bash
# verify-durable-layer.sh — reproducible integration check for the durable
# layer built this cycle (matrix §G7 / §G8 / §4.1 plan-mode), against a REAL
# Postgres (not a mock). Proves the custom/risky SQL behaves correctly:
#   - G7  UpsertAgentSkill: ON CONFLICT upsert + xmax created-detection +
#         provenance guard (a background_review write must NOT overwrite a
#         user-authored skill) + owner-scoped proposals (WS07 S7: the conflict
#         key is (org, scope, owner, name); a learned write refreshes only a
#         proposal and never touches enabled/scope/owner/review_state).
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

# WS07 S7: the conflict key carries the owner, a learned write is a disabled proposal and
# refreshes only a proposal (never enabled, scope, owner or review_state), and a learned skill
# never overwrites a human's.
UPSERT_TAIL="ON CONFLICT (org_id,scope,owner_user_id,name) DO UPDATE SET content=EXCLUDED.content, updated_at=now() WHERE agent_skills.origin = 'background_review' AND agent_skills.review_state = 'proposed' RETURNING (xmax=0);"
COLS="(id,org_id,name,description,content,trigger_keywords,trigger_file_patterns,tool_restrictions,enabled,origin,scope,owner_user_id,review_state,created_at,updated_at)"
LEARNED="false,'background_review','user'"

echo "==> G7 UpsertAgentSkill provenance guard and owner-scoped proposals (0058)"
r=$(q <<SQL
INSERT INTO agent_skills $COLS VALUES ('s1','org1','Cache','d','c1','[]'::jsonb,'[]'::jsonb,'[]'::jsonb,$LEARNED,'u1','proposed',now(),now()) $UPSERT_TAIL
SQL
)
expect "$r" "t" "fresh insert => created"
r=$(q <<SQL
INSERT INTO agent_skills $COLS VALUES ('s1b','org1','Cache','d','c2','[]'::jsonb,'[]'::jsonb,'[]'::jsonb,$LEARNED,'u1','proposed',now(),now()) $UPSERT_TAIL
SQL
)
expect "$r" "f" "re-upsert of a proposal => updated (not created)"
r=$(q <<SQL
INSERT INTO agent_skills $COLS VALUES ('s1c','org1','Cache','d','B-content','[]'::jsonb,'[]'::jsonb,'[]'::jsonb,$LEARNED,'u2','proposed',now(),now()) $UPSERT_TAIL
SQL
)
expect "$r" "t" "a second user's skill of the same name is a row of their own"
expect "$(q <<<"SELECT content FROM agent_skills WHERE org_id='org1' AND name='Cache' AND owner_user_id='u1';")" "c2" "the first user's skill is untouched by the second user's write"
expect "$(q <<<"SELECT enabled::text || '/' || review_state FROM agent_skills WHERE id='s1';")" "false/proposed" "the refresh left the row a disabled proposal"
q >/dev/null <<SQL
INSERT INTO agent_skills $COLS VALUES ('s2','org1','Runbook','d','human','[]'::jsonb,'[]'::jsonb,'[]'::jsonb,true,'user','user','u1','accepted',now(),now());
SQL
r=$(q <<SQL
INSERT INTO agent_skills $COLS VALUES ('s3','org1','Runbook','d','MACHINE','[]'::jsonb,'[]'::jsonb,'[]'::jsonb,$LEARNED,'u1','proposed',now(),now()) $UPSERT_TAIL
SQL
)
expect "${r:-EMPTY}" "EMPTY" "background_review cannot overwrite user skill (0 rows)"
expect "$(q <<<"SELECT content FROM agent_skills WHERE org_id='org1' AND name='Runbook';")" "human" "user skill body preserved"
q >/dev/null <<SQL
INSERT INTO agent_skills $COLS VALUES ('s4','org1','Decided','d','original','[]'::jsonb,'[]'::jsonb,'[]'::jsonb,false,'background_review','user','u1','accepted',now(),now());
SQL
r=$(q <<SQL
INSERT INTO agent_skills $COLS VALUES ('s5','org1','Decided','d','rewritten','[]'::jsonb,'[]'::jsonb,'[]'::jsonb,$LEARNED,'u1','proposed',now(),now()) $UPSERT_TAIL
SQL
)
expect "${r:-EMPTY}" "EMPTY" "an accepted (here admin-disabled) row is neither rewritten nor re-enabled (0 rows)"
expect "$(q <<<"SELECT content || '/' || enabled::text FROM agent_skills WHERE org_id='org1' AND name='Decided';")" "original/false" "the decided row keeps its body and stays disabled"
if q >/dev/null 2>&1 <<<"INSERT INTO agent_skills (id,org_id,name,review_state) VALUES ('bad','org1','Bad','pending');"; then
  echo "FAIL: a review_state outside the closed set was accepted"; exit 1
fi
echo "  ok: the CHECK refuses a review_state outside the closed set"

echo "==> P3 SetRunMode"
q >/dev/null <<SQL
INSERT INTO threads (id,session_key,org_id,user_id) VALUES ('t1','sk1','org1','u1');
INSERT INTO runs (id,thread_id,goal,org_id,user_id) VALUES ('r1','t1','g','org1','u1');
SQL
expect "$(q <<<"UPDATE runs SET mode='plan' WHERE id='r1' RETURNING mode;")" "plan" "SetRunMode -> plan"
expect "$(q <<<"UPDATE runs SET mode='plan' WHERE id='nope' RETURNING id;")" "" "SetRunMode missing run => 0 rows"

echo "==> WS04 S4 durable plan grant (0050_thread_autonomy_grant)"
GRANT_SET="UPDATE threads SET granted_rung='workspace_write', granted_justification='writes the report', granted_plan_id='plan-1', granted_at=now(), granted_expires_at=now() + (86400::bigint * interval '1 second') WHERE id='t1' AND org_id='org1' RETURNING granted_rung || '/' || granted_plan_id;"
expect "$(q <<<"SELECT granted_rung IS NULL AND granted_expires_at IS NULL FROM threads WHERE id='t1';")" "t" "a thread that never held a grant keeps every column NULL"
expect "$(q <<<"$GRANT_SET")" "workspace_write/plan-1" "the approval writes the grant on its thread"
expect "$(q <<<"UPDATE threads SET granted_rung='workspace_write' WHERE id='t1' AND org_id='other-org' RETURNING id;")" "" "a thread of another org is not touched"
expect "$(q <<<"SELECT granted_expires_at > now() + interval '23 hours' AND granted_expires_at <= now() + interval '24 hours' FROM threads WHERE id='t1';")" "t" "the grant lapses 24 hours after it was made"
if q >/dev/null 2>&1 <<<"UPDATE threads SET granted_rung='root' WHERE id='t1';"; then
  echo "FAIL: a rung label outside the ladder was accepted"; exit 1
fi
echo "  ok: the CHECK refuses a rung label outside the ladder"
q >/dev/null <<<"UPDATE threads SET granted_rung=NULL, granted_justification=NULL, granted_plan_id=NULL, granted_at=NULL, granted_expires_at=NULL WHERE id='t1';"
echo "  -- reversal and re-application"
docker exec -i "$CTR" psql -U postgres -d session_core -v ON_ERROR_STOP=1 -q < "$MIG/revert/0050_thread_autonomy_grant.down.sql"
expect "$(q <<<"SELECT count(*) FROM information_schema.columns WHERE table_name='threads' AND column_name LIKE 'granted\_%';")" "0" "the reversal drops every grant column"
# The boot check of session-core (ThreadGrantColumns::detect) counts these five columns and keeps
# its gate open only when all five are there.
GRANT_COLS="SELECT count(*) FROM information_schema.columns WHERE table_schema = current_schema() AND table_name = 'threads' AND column_name::text = ANY(ARRAY['granted_rung','granted_justification','granted_plan_id','granted_at','granted_expires_at']::text[]);"
expect "$(q <<<"$GRANT_COLS")" "0" "the boot check finds none of the five columns after the reversal"
docker exec -i "$CTR" psql -U postgres -d session_core -v ON_ERROR_STOP=1 -q < "$MIG/0050_thread_autonomy_grant.sql"
docker exec -i "$CTR" psql -U postgres -d session_core -v ON_ERROR_STOP=1 -q < "$MIG/0050_thread_autonomy_grant.sql"
expect "$(q <<<"SELECT count(*) FROM information_schema.columns WHERE table_name='threads' AND column_name LIKE 'granted\_%';")" "5" "the migration re-applies, and twice is harmless"
expect "$(q <<<"$GRANT_COLS")" "5" "the boot check finds all five columns once the migration is applied"

echo "==> G8 approval id-alignment"
q >/dev/null <<SQL
INSERT INTO approvals (id,run_id,kind,status,requested_by,org_id,user_id,idempotency_key,metadata,requested_at)
VALUES ('appr-gw-1','r1','tool_call','requested','svc','org1','u1','','{}'::jsonb,now());
SQL
expect "$(q <<<"UPDATE approvals SET status='granted',decided_by='a',decided_at=now() WHERE id='appr-gw-1' RETURNING status;")" "granted" "DecideApproval targets gateway-supplied id"

echo
echo "PASS — durable layer verified against real Postgres."
