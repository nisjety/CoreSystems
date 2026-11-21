-- browser-broker 0009: the read-only level on the durable grant store (WS21-S4).
--
-- 0002 stays reserved for WS7 (R-3) and 0008 is the web-policy snapshot, so this
-- is the next free number.
--
-- The container entrypoint re-applies every *.up.sql on each start, so this
-- statement is idempotent: ADD COLUMN IF NOT EXISTS skips it once it exists.
--
-- What changes:
--   * read_only   true when the grant is issued at the read level. A non-ZDR
--                 read-only grant is refused until BROWSER_BROKER_DURABLE_READ_ONLY_ENABLED
--                 is on, and cmd/main.go keeps that switch off while this column is
--                 absent, so the level is never dropped on the way to the table.
--                 A run derived from, or an origin extension under, a read-only grant
--                 copies the level, as the in-memory store does. A row written before
--                 this column reads false: the full-access state it always had.
--
-- Still no zdr column: a ZDR grant lives in the broker's memory with a "z_" id
-- and never reaches this table (X-2), so no ZDR read level is stored here.

ALTER TABLE browser_grants
    ADD COLUMN IF NOT EXISTS read_only BOOLEAN NOT NULL DEFAULT false;
