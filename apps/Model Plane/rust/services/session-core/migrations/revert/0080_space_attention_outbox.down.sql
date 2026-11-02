-- Reverses 0080_space_attention_outbox.sql.
--
-- Not applied by sqlx: this file lives in a subdirectory, and sqlx's migrate! reads
-- only the top level of migrations/. Run it by hand, against the database only, inside
-- one transaction.
--
-- Drops the trigger first so no log write enqueues an outbox row, then deletes the
-- unpublished space rows (the narrower subject CHECK would otherwise refuse to re-add),
-- then restores the two-family CHECK from 0016. Rows already published are deleted too:
-- the outbox is a delivery queue, not a record. Safe to run twice.

BEGIN;
DROP TRIGGER IF EXISTS space_attention_outbox_ins ON space_event_log;
DROP FUNCTION IF EXISTS space_attention_to_outbox();
DROP FUNCTION IF EXISTS space_message_mentions(TEXT);
DROP TABLE IF EXISTS space_attention_settings;

DELETE FROM session_audit_outbox WHERE subject LIKE 'mp.v1.space.%.event';

ALTER TABLE session_audit_outbox
    DROP CONSTRAINT IF EXISTS session_audit_outbox_subject_check;
ALTER TABLE session_audit_outbox
    ADD CONSTRAINT session_audit_outbox_subject_check
    CHECK (
        subject LIKE 'verevon.audit.v2.model.session-core.%'
        OR subject LIKE 'mp.v1.run.%.event'
    );
COMMIT;
