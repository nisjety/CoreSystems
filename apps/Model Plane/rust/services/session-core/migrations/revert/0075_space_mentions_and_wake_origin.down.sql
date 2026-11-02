-- Reverses 0075_space_mentions_and_wake_origin.sql.
--
-- Not applied by sqlx: this file lives in a subdirectory, and sqlx's migrate! reads
-- only the top level of migrations/. Run it by hand, against the database only,
-- inside one transaction.
--
-- Dropping message_mentions loses which people and agents a post addressed and which agent
-- wakes were expected or refused; dropping the runs columns loses the Control-signed turn
-- origin and the (post, agent) pair of every woken run. Roll session-core back first (a build
-- that reads these columns fails with "column ... does not exist"), then run this file.
--
-- Safe to run twice.

BEGIN;
DROP INDEX IF EXISTS runs_trigger_idx;
ALTER TABLE runs DROP CONSTRAINT IF EXISTS runs_wake_pair_chk;
ALTER TABLE runs DROP CONSTRAINT IF EXISTS runs_turn_origin_chk;
ALTER TABLE runs
    DROP COLUMN IF EXISTS agent_subject_id,
    DROP COLUMN IF EXISTS trigger_message_id,
    DROP COLUMN IF EXISTS turn_origin;
DROP INDEX IF EXISTS message_mentions_subject_idx;
DROP TABLE IF EXISTS message_mentions;
COMMIT;
