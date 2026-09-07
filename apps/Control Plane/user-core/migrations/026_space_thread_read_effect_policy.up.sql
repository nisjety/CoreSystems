-- 026_space_thread_read_effect_policy — reading a shared Space's conversation
-- record is its own effect class.
--
-- Until now every transcript read was owner-bound: Model Plane answered only
-- for the thread's own `user_id`, so members of one room each saw a disjoint
-- slice of it. Admitting a member to another member's turns is a real
-- disclosure, so it gets its own deny-by-default bit rather than inheriting
-- thread_create_entitled — the same separation retrieval_read_entitled makes
-- against thread creation.
--
-- Scope note: this entitlement authorizes reading the SHARED record of a Space
-- the caller is a current member and current recipient of. It says nothing
-- about personal Spaces, which keep the owner-bound path unchanged.

BEGIN;

ALTER TABLE space_effect_policies
    ADD COLUMN IF NOT EXISTS thread_read_entitled BOOLEAN NOT NULL DEFAULT FALSE;

COMMIT;
