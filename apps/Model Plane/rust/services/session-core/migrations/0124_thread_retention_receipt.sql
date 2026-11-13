-- 0124: a thread carries a retention receipt (ZDR Z-17, thread half; decision D-ZDR-04).
--
-- A run is stamped with the retention posture it started under (0044). A thread was not: an
-- AppendMessage into an existing thread was never checked against the posture the thread was
-- created under, so a waived caller could append into a persistent thread, and a persistent
-- caller into a waived one. The receipt lets the append be checked, before any write, against
-- the same type a run uses (retention_receipt::RunRetention):
--
--   persistent  an ordinary thread.
--   zdr         a zero-retention thread. A zero-retention credential cannot create a thread, so the
--               value is allowed for completeness and is never written by session-core today.
--   zdr_waived  a thread created under a connection-bound waiver: it names the acknowledgement and
--               the one connection the waiver was issued for.
--
-- Written once, at CreateThread, and only while SESSION_THREAD_RECEIPT_ENFORCE is true. Nothing
-- reads the columns while that switch is off.
--
-- NO BACKFILL, on purpose (D-ZDR-04): NULL reads as persistent. A backfill would assert a past
-- retention posture without evidence. A thread created before this migration, or while the switch
-- was off, therefore reads as persistent.
--
-- Zero Data Retention: the three columns are ids and a closed class word, never content. A thread
-- row is erased with the thread and with the org (gdpr.rs), so the receipt is erased with it.
--
-- Additive only: three nullable columns. The CHECK on the class is dropped and re-added by name
-- (as 0100_user_profiles.sql does for its CHECK), NOT VALID then VALIDATE, so a re-run is safe and
-- the swap holds no long exclusive lock. The reversal is in
-- revert/0124_thread_retention_receipt.down.sql.

ALTER TABLE threads
    ADD COLUMN IF NOT EXISTS retention_class TEXT NULL,
    ADD COLUMN IF NOT EXISTS waiver_ack_id TEXT NULL
        CHECK (waiver_ack_id IS NULL OR length(waiver_ack_id) BETWEEN 1 AND 128),
    ADD COLUMN IF NOT EXISTS waiver_connection_id TEXT NULL
        CHECK (waiver_connection_id IS NULL OR length(waiver_connection_id) BETWEEN 1 AND 128);

ALTER TABLE threads DROP CONSTRAINT IF EXISTS threads_retention_class_check;
ALTER TABLE threads
    ADD CONSTRAINT threads_retention_class_check
    CHECK (retention_class IS NULL OR retention_class IN ('persistent', 'zdr', 'zdr_waived')) NOT VALID;
ALTER TABLE threads VALIDATE CONSTRAINT threads_retention_class_check;
