-- WS22-S1: one live thread lease per user and thread. AcquireThread upserts on
-- this index, so a retried acquire returns the same lease id instead of minting
-- a second sandbox for the same thread. state <> 4 (DESTROYED) keeps a released
-- lease out of the index, so the next acquire creates a fresh one. The literal
-- predicate must match the ON CONFLICT predicate in lease.AcquireThread exactly
-- for Postgres to infer this index.
--
-- Fails on a database that already holds two live thread leases for one owner
-- and thread. The sandbox-manager has never acquired thread-scoped leases in a
-- durable deployment, so there is none to reconcile.
CREATE UNIQUE INDEX IF NOT EXISTS leases_thread_live_uq
    ON leases (org_id, owner_id, scope_id)
    WHERE scope_type = 'thread' AND state <> 4;
