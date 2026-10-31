-- WS4.J (review round): the org-erasure marker for the ZDR run tombstones.
--
-- PurgeOrg deletes an org's tombstones, but an edge that does not know the org was
-- erased can still send the tombstone of a run that ends afterwards (an agent run that
-- expires later, a scrape that was in flight). Without this table that late tombstone
-- would re-create a row for the erased org. PurgeOrg writes one marker row in the same
-- transaction as its deletes; the tombstone writer refuses (silently: the receiver still
-- answers 204) a tombstone whose org has a marker; the tombstone sweeper deletes a marker
-- once it is older than max(retention, 7 days), long past any run that was in flight.
--
-- The marker holds an org id and a timestamp and nothing else. It is created only for
-- an id of the same shape the tombstone table accepts. Own migration (not an edit of 015)
-- because 015 may already be recorded in schema_migrations on a database that ran it.
--
-- OFF BY DEFAULT with the rest of WS4.J, in quarry-control as well as on the edge: the
-- table is created by every deploy (an empty table retains nothing), but PurgeOrg writes
-- a marker, and the receiver is mounted, only when QUARRY_ZDR_TOMBSTONE_RECEIVER is set,
-- which counsel review gates together with QUARRY_EDGE__ZDR_TOMBSTONE. A marker keeps an
-- erased org's id for at least 7 days (the sweeper), and indefinitely under
-- QUARRY_ZDR_TOMBSTONE_DRY_RUN, so it is part of the counsel review scope. Idempotent.

CREATE TABLE IF NOT EXISTS zdr_tombstone_erasures (
    org_id    TEXT PRIMARY KEY
        CONSTRAINT zdr_tombstone_erasures_org_id_shape CHECK (org_id ~ '^[A-Za-z0-9_-]{1,64}$'),
    erased_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS zdr_tombstone_erasures_erased_idx
    ON zdr_tombstone_erasures (erased_at);
