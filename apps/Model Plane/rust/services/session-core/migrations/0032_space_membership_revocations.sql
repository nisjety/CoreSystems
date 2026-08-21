-- 0032_space_membership_revocations.sql
-- Local, durable projection of Control Plane's aqencia.controlplane.space.membership_revoked
-- events. resolve_run_owner/owner_matches (run_service_grpc.rs) join against
-- this to deny resource-scoped run/thread/memory access once a subject's
-- Space membership has been revoked, closing a gap where org_id+user_id
-- ownership alone never noticed a member was removed from the Space a run
-- or thread happened in.
--
-- Durable (not an in-memory cache) so the fix survives a session-core
-- restart without needing Control Plane to replay history it does not keep;
-- a missed event just means this table doesn't yet reflect a revocation
-- that already happened elsewhere, never a regression below today's
-- org+user-only check.

CREATE TABLE IF NOT EXISTS space_membership_revocations (
    space_ref  TEXT NOT NULL,
    org_id     TEXT NOT NULL,
    subject_id TEXT NOT NULL,
    revoked_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (space_ref, subject_id)
);
