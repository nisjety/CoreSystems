-- E3.4-1: widen the browser-run checkpoint status with `suspended` and `expired`.
--
-- These are the durable side of the one session state machine (D8-10): `expired` is set
-- by the TTL reaper, `suspended` by a deliberate release with the profile persisted; both
-- may be resumed with a fresh grant (never under ZDR). `closed` stays terminal. No new
-- table, no new column, no backfill: existing rows keep `active` or `closed`.
--
-- The 0006 CHECK was declared inline, so Postgres named it
-- `quarry_agent_run_checkpoints_status_check`; it is dropped and re-added under the same
-- name, which makes this file safe to apply twice. Old binaries write only `active` and
-- `closed`, which the wider CHECK still accepts.
--
-- Number 0020: W2-U49a takes 0019 (W23 queue, step S1); the lead arbitrates at merge.
ALTER TABLE quarry_agent_run_checkpoints
    DROP CONSTRAINT IF EXISTS quarry_agent_run_checkpoints_status_check;

ALTER TABLE quarry_agent_run_checkpoints
    ADD CONSTRAINT quarry_agent_run_checkpoints_status_check
    CHECK (status IN ('active', 'suspended', 'expired', 'closed'));
