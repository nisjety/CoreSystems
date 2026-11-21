-- WS22-S5 durable blob erasure (F-f06-02): one tombstone per thread whose blobs
-- are still to be removed from the object store.
--
-- eraseRows writes the tombstone in the same transaction that deletes the
-- thread's files and header, so a crash can never leave the rows gone with no
-- record that the blobs remain. The blob removal runs after that commit. Its
-- success deletes the tombstone; its failure leaves the tombstone for the
-- retention sweeper, which retries it at most once per sweep interval.
--
-- Ids and counters only: no file, path, content or blob key is stored here. A
-- ZDR run never reaches this table: AcquireLease refuses a ZDR thread lease before
-- any workspace row is written.
--
-- Idempotent: the entrypoint re-runs every up.sql at each boot, with no ledger.
CREATE TABLE IF NOT EXISTS thread_workspace_erasures (
    org_id           TEXT        NOT NULL,
    owner_id         TEXT        NOT NULL,
    thread_id        TEXT        NOT NULL,
    requested_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    attempts         INT         NOT NULL DEFAULT 0,
    last_attempt_at  TIMESTAMPTZ,
    PRIMARY KEY (org_id, owner_id, thread_id)
);

CREATE INDEX IF NOT EXISTS thread_workspace_erasures_requested_idx
    ON thread_workspace_erasures (requested_at);
