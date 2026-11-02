-- Session-core's copy of an organization's memory and learning-review policy
-- (build plan WS07 S4).
--
-- The policy is owned by org-core (organizations.metadata.agentLearning). A poller
-- (org_policy.rs) copies it here so the memory gate can answer without a network
-- call, and so a policy that cannot be refreshed is visibly old instead of
-- invisibly missing. One row per organization:
--
--   memory_allowed        false when an org admin disabled memory for the org
--   learning_mode         'disabled' (the default) or 'proposals' (the org opted in
--                         to a paid review model reading conversations)
--   memory_enabled_since  set when a poll sees memory go from disabled to allowed;
--                         Dreaming never reads a message at or before it, so the
--                         period memory was off is not mined after it is back on.
--                         NULL for an org that was never seen disabled.
--   revision              org-core's revision of the policy at the last poll
--   fetched_at            when the row was last refreshed; the gate treats a row
--                         older than ORG_POLICY_MAX_STALE as unknown when
--                         ORG_POLICY_FAIL_CLOSED is on
--
-- Content-free: flags, a timestamp and a counter. Rows are erased with the
-- organization (gdpr::purge_org_data).
--
-- Additive: one new table, no backfill, nothing reads it until the poller is
-- configured. The reversal is in revert/0057_org_learning_policy_mirror.down.sql.
CREATE TABLE IF NOT EXISTS org_learning_policy_mirror (
    org_id TEXT PRIMARY KEY,
    memory_allowed BOOLEAN NOT NULL,
    learning_mode TEXT NOT NULL DEFAULT 'disabled'
        CHECK (learning_mode IN ('disabled', 'proposals')),
    memory_enabled_since TIMESTAMPTZ,
    revision BIGINT NOT NULL DEFAULT 0,
    fetched_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
