-- Approval scope grants (build plan WS13 S6).
--
-- A "this thread" or "this run" decision on an eligible pause leaves one grant in the same
-- transaction as the decision. The next identical call (same owner, same thread, same tool,
-- action key and declared class, and for a run grant the same run) is released without a new
-- pause, and each release is audited. A grant is inert once it expires, once its owner revokes
-- it, once its run ends (a run grant) and for every call of a child agent.
--
--   scope                    thread or run. Offered only where the cause allows it (WS13 S6).
--   action_key               tool and operation, never argument values.
--   effect_class             the declared class the decision was made under. A release needs
--                            the same class, read again from the signed decision.
--   cause_code               the closed cause of the approval that created the grant, kept for
--                            the audit row of each release.
--   created_from_approval_id the decision that created the grant. Erased with it.
--   expires_at               always set: APPROVAL_THREAD_GRANT_TTL_SECS (default 3600, max 28800).
--   revoked_at, revoked_by   set together, once, by the owner.
--   use_count, last_used_at  counted in the same transaction as the release's audit row.
--
-- Rows are erased with their thread (ON DELETE CASCADE) and with the approval that created
-- them. A zero-retention or waived run never creates one. The reversal is in
-- revert/0062_approval_scope_grants.down.sql.

CREATE TABLE IF NOT EXISTS approval_scope_grants (
    id TEXT PRIMARY KEY
        CHECK (id LIKE 'apg\_%'),
    org_id TEXT NOT NULL
        CHECK (length(org_id) BETWEEN 1 AND 128),
    user_id TEXT NOT NULL
        CHECK (length(user_id) BETWEEN 1 AND 128),
    thread_id TEXT NOT NULL
        REFERENCES threads(id) ON DELETE CASCADE,
    run_id TEXT
        REFERENCES runs(id) ON DELETE CASCADE,
    scope TEXT NOT NULL
        CHECK (scope IN ('thread', 'run')),
    tool_name TEXT NOT NULL
        CHECK (length(tool_name) BETWEEN 1 AND 256),
    action_key TEXT NOT NULL
        CHECK (length(action_key) BETWEEN 1 AND 256),
    effect_class TEXT NOT NULL
        CHECK (length(effect_class) BETWEEN 1 AND 64),
    cause_code TEXT NOT NULL DEFAULT 'unclassified_gate'
        CHECK (length(cause_code) BETWEEN 1 AND 64),
    created_from_approval_id TEXT NOT NULL
        REFERENCES approvals(id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at TIMESTAMPTZ NOT NULL,
    revoked_at TIMESTAMPTZ,
    revoked_by TEXT NOT NULL DEFAULT ''
        CHECK (length(revoked_by) <= 128),
    use_count BIGINT NOT NULL DEFAULT 0
        CHECK (use_count >= 0),
    last_used_at TIMESTAMPTZ,
    -- A run grant names its run, and a thread grant names none.
    CONSTRAINT approval_scope_grants_run_shape
        CHECK ((scope = 'run') = (run_id IS NOT NULL)),
    -- Revocation sets its time and its owner together, and only revocation does.
    CONSTRAINT approval_scope_grants_revoke_shape
        CHECK ((revoked_at IS NULL) = (revoked_by = '')),
    -- A grant that was never used has no last use, and a used one has one.
    CONSTRAINT approval_scope_grants_use_shape
        CHECK ((use_count = 0) = (last_used_at IS NULL))
);

-- One unrevoked grant per owner, thread, scope, run and action: a second decision for the same
-- action extends that grant's expiry (also after it expired) rather than adding a second grant.
CREATE UNIQUE INDEX IF NOT EXISTS approval_scope_grants_live_idx
    ON approval_scope_grants (org_id, user_id, thread_id, scope, COALESCE(run_id, ''), action_key)
    WHERE revoked_at IS NULL;

-- The owner's grants in one thread, oldest first, for the list the thread shows.
CREATE INDEX IF NOT EXISTS approval_scope_grants_thread_idx
    ON approval_scope_grants (org_id, user_id, thread_id, created_at, id);
