-- 0040: idempotency for ManagedRunLifecycle.CompleteDirectRunWithMessage.
--
-- A completed gateway-direct turn used to be two calls: AppendMessage, then
-- RecordTerminalOutcome. A durable cancel could land between them and leave a
-- saved answer on a cancelled run. The combined call writes the assistant
-- message, the Completed terminal receipt and this row's applied state in one
-- transaction. These two columns record which caller operation did that, so a
-- retry of the same operation after a lost response gets the original receipt
-- and message sequence back instead of a second message or a conflict.
--
-- Zero Data Retention posture is unchanged: the key is an opaque caller
-- operation id (the Gateway request id), never derived from content, and the
-- sequence is a transcript position. Rows applied by RecordTerminalOutcome or
-- by recovery keep both columns NULL. Additive only: no backfill.

ALTER TABLE managed_run_terminalization_outbox
    ADD COLUMN IF NOT EXISTS completion_key TEXT
        CHECK (completion_key IS NULL
            OR completion_key ~ '^[A-Za-z0-9][A-Za-z0-9_-]{0,127}$'),
    ADD COLUMN IF NOT EXISTS completion_message_sequence BIGINT
        CHECK (completion_message_sequence IS NULL
            OR completion_message_sequence >= 0);

-- Both are set together, and only on a Completed receipt: a failed or
-- reconciled outcome never carries a completion operation.
ALTER TABLE managed_run_terminalization_outbox
    DROP CONSTRAINT IF EXISTS managed_run_terminalization_outbox_completion_chk;

ALTER TABLE managed_run_terminalization_outbox
    ADD CONSTRAINT managed_run_terminalization_outbox_completion_chk
    CHECK (
        (completion_key IS NULL AND completion_message_sequence IS NULL)
        OR (completion_key IS NOT NULL
            AND completion_message_sequence IS NOT NULL
            AND state = 'applied'
            AND outcome = 'completed')
    );
