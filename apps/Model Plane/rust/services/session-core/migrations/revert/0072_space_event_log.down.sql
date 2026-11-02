-- Reverses 0072_space_event_log.sql.
--
-- Not applied by sqlx: this file lives in a subdirectory, and sqlx's migrate! reads
-- only the top level of migrations/. Run it by hand, against the database only, inside
-- one transaction.
--
-- Drops the triggers first, so no write path on messages, threads or runs fires them,
-- then the functions, then the log. The log holds ids and kinds only; dropping it loses
-- no transcript, but every connected Space stream loses its resume point and must
-- resnapshot. Stop (or roll back) any build that reads space_event_* before running
-- this, or its reads fail until it restarts.
--
-- Safe to run twice (every statement is IF EXISTS).

BEGIN;
DROP TRIGGER IF EXISTS space_event_messages_ins ON messages;
DROP TRIGGER IF EXISTS space_event_threads_ins ON threads;
DROP TRIGGER IF EXISTS space_event_threads_upd ON threads;
DROP TRIGGER IF EXISTS space_event_threads_arch ON threads;
DROP TRIGGER IF EXISTS space_event_threads_del ON threads;
DROP TRIGGER IF EXISTS space_event_runs_ins ON runs;
DROP TRIGGER IF EXISTS space_event_runs_upd ON runs;

DROP FUNCTION IF EXISTS space_event_forget_subject(TEXT, TEXT);
DROP FUNCTION IF EXISTS space_event_from_runs_update();
DROP FUNCTION IF EXISTS space_event_from_runs_insert();
DROP FUNCTION IF EXISTS space_event_from_threads_delete();
DROP FUNCTION IF EXISTS space_event_from_threads_archive();
DROP FUNCTION IF EXISTS space_event_from_threads_update();
DROP FUNCTION IF EXISTS space_event_from_threads_insert();
DROP FUNCTION IF EXISTS space_event_from_messages();
DROP FUNCTION IF EXISTS space_event_emit(TEXT, TEXT, TEXT, TEXT, TEXT, TEXT, TEXT, TEXT, BIGINT, TEXT, JSONB);
DROP FUNCTION IF EXISTS space_event_run_state(TEXT);

DROP TABLE IF EXISTS space_event_errors;
DROP TABLE IF EXISTS space_event_log;
DROP TABLE IF EXISTS space_event_heads;
DROP TABLE IF EXISTS space_event_settings;
COMMIT;
