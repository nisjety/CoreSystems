-- Reverses 0050_thread_autonomy_grant.sql.
--
-- Not applied by sqlx: this file lives in a subdirectory, and sqlx's migrate!
-- reads only the top level of migrations/. Run it by hand, against the database
-- only, inside one transaction. Dropping the columns discards every thread's
-- recorded plan grant: a run started afterwards sees no durable grant, so turn
-- MODEL_GATEWAY_DURABLE_PLAN_GRANT off on every gateway replica first (they then
-- fall back to the in-memory grants). The run-level copy of the grant on
-- runs.metadata is untouched.
--
-- ORDER, all three steps:
--   1. turn MODEL_GATEWAY_DURABLE_PLAN_GRANT off on every gateway replica;
--   2. run this file;
--   3. RESTART session-core. It checks once, at boot, whether the grant columns
--      exist (information_schema). Until it restarts it still believes they do,
--      and every SetRunMode that states a grant fails with "column granted_rung
--      does not exist" and rolls its run update back with it, and GetThreadGrant
--      fails. After the restart a stated grant is skipped with a warning (the run
--      update proceeds) and GetThreadGrant answers "no grant" without touching
--      the columns. To re-apply 0050 later, run the up file by hand and restart
--      session-core again: sqlx records 0050 as applied and will not re-run it.
--
-- Safe to run twice (every statement is IF EXISTS).

BEGIN;
ALTER TABLE threads DROP CONSTRAINT IF EXISTS threads_granted_rung_check;
ALTER TABLE threads
    DROP COLUMN IF EXISTS granted_expires_at,
    DROP COLUMN IF EXISTS granted_at,
    DROP COLUMN IF EXISTS granted_plan_id,
    DROP COLUMN IF EXISTS granted_justification,
    DROP COLUMN IF EXISTS granted_rung;
COMMIT;
