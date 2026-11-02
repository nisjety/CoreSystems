-- Reverses 0078_thread_activity_summary.sql.
--
-- Not applied by sqlx: this file lives in a subdirectory, and sqlx's migrate! reads only the
-- top level of migrations/. Run it by hand, against the database only, inside one transaction.
--
-- Dropping the columns drops the summary with them; nothing else reads them until the timeline
-- and attention RPCs (WS15-S6, WS11-S2) land, and those must be rolled back or switched off
-- first or their reads fail with "column last_message_seq does not exist". threads.last_activity_at
-- belongs to migration 0049 and is left alone. Safe to run twice.

BEGIN;
DROP TRIGGER IF EXISTS messages_thread_activity ON messages;
DROP FUNCTION IF EXISTS thread_activity_apply();
DROP FUNCTION IF EXISTS thread_activity_recompute(TEXT);
DROP INDEX IF EXISTS idx_threads_activity_pending;
DROP INDEX IF EXISTS idx_threads_space_activity;
ALTER TABLE threads
    DROP COLUMN IF EXISTS activity_max_audience_revision,
    DROP COLUMN IF EXISTS recent_author_subject_ids,
    DROP COLUMN IF EXISTS last_author_subject_id,
    DROP COLUMN IF EXISTS last_role,
    DROP COLUMN IF EXISTS turn_count,
    DROP COLUMN IF EXISTS last_message_seq;
COMMIT;
