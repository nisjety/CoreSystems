-- Run retention receipt (Z-17, zero data retention).
--
-- Every run records the retention posture that applied when it was created:
--   zdr         a zero-retention run: its content is not persisted.
--   persistent  an ordinary run.
--   zdr_waived  a persistent run under a connection-bound waiver. The org is ZDR
--               and the session user acknowledged that one connection cannot
--               give ZDR, so this run alone runs on that connection with ZDR off.
--
-- Session Core stamps the class from the caller's verified claims when the run
-- starts. A child run inherits its parent's receipt. The receipt never changes
-- after the row is written (runs_retention_receipt_immutable below).
--
-- Existing rows read as persistent: the default fills them when the column is
-- added, and they carry no waiver. The one exception is a run that was already
-- started under zero retention: its managed obligation recorded the ZDR flag at
-- start, so it is stamped zdr below. Without that, a retry of its start would no
-- longer match its receipt. The reversal is in
-- revert/0044_run_retention_receipt.down.sql.

ALTER TABLE runs
    ADD COLUMN IF NOT EXISTS retention_class TEXT NOT NULL DEFAULT 'persistent'
        CHECK (retention_class IN ('zdr', 'persistent', 'zdr_waived')),
    ADD COLUMN IF NOT EXISTS waiver_ack_id TEXT
        CHECK (waiver_ack_id IS NULL OR length(waiver_ack_id) BETWEEN 1 AND 128),
    ADD COLUMN IF NOT EXISTS waiver_connection_id TEXT
        CHECK (waiver_connection_id IS NULL OR length(waiver_connection_id) BETWEEN 1 AND 128);

-- A waiver names its acknowledgement and its connection, and no other class does.
ALTER TABLE runs
    DROP CONSTRAINT IF EXISTS runs_retention_waiver_consistent,
    ADD CONSTRAINT runs_retention_waiver_consistent CHECK (
        (retention_class = 'zdr_waived'
            AND waiver_ack_id IS NOT NULL
            AND waiver_connection_id IS NOT NULL)
        OR
        (retention_class <> 'zdr_waived'
            AND waiver_ack_id IS NULL
            AND waiver_connection_id IS NULL)
    );

-- Stamp the runs that were started under zero retention before this migration.
-- Runs the default already reads as persistent, and runs with no managed
-- obligation (legacy StartRun), are left alone. This runs before the trigger
-- below, which refuses any change to the receipt.
UPDATE runs AS r
SET retention_class = 'zdr'
FROM managed_run_terminalization_outbox AS o
WHERE o.run_id = r.id
  AND o.zdr
  AND r.retention_class <> 'zdr';

-- The receipt is immutable. Only an UPDATE that names one of its columns can
-- change it, and any change to those columns is refused.
CREATE OR REPLACE FUNCTION runs_retention_receipt_immutable()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF NEW.retention_class IS DISTINCT FROM OLD.retention_class
       OR NEW.waiver_ack_id IS DISTINCT FROM OLD.waiver_ack_id
       OR NEW.waiver_connection_id IS DISTINCT FROM OLD.waiver_connection_id THEN
        RAISE EXCEPTION 'run retention receipt is immutable (run %)', OLD.id
            USING ERRCODE = 'check_violation';
    END IF;
    RETURN NEW;
END;
$$;

DROP TRIGGER IF EXISTS runs_retention_receipt_immutable_trg ON runs;
CREATE TRIGGER runs_retention_receipt_immutable_trg
    BEFORE UPDATE OF retention_class, waiver_ack_id, waiver_connection_id ON runs
    FOR EACH ROW
    EXECUTE FUNCTION runs_retention_receipt_immutable();

-- Background derivation (dreaming) asks, per thread, whether any run was waived.
-- Only waived runs are indexed, so the lookup stays small.
CREATE INDEX IF NOT EXISTS runs_zdr_waived_thread_idx
    ON runs (thread_id)
    WHERE retention_class = 'zdr_waived';
