-- Space authority context is deliberately separate from workspace_id. The
-- latter selects context content; these references record the Control-issued
-- scope/policy/audience that authorized thread creation.

ALTER TABLE threads
    ADD COLUMN IF NOT EXISTS space_id TEXT,
    ADD COLUMN IF NOT EXISTS space_decision_ref TEXT,
    ADD COLUMN IF NOT EXISTS recipient_audience_ref TEXT,
    ADD COLUMN IF NOT EXISTS privacy_policy_ref TEXT,
    ADD COLUMN IF NOT EXISTS resource_authorization_ref TEXT,
    ADD COLUMN IF NOT EXISTS authority_revision BIGINT;

ALTER TABLE threads
    ADD CONSTRAINT threads_space_context_complete_chk CHECK (
        (space_id IS NULL AND space_decision_ref IS NULL
            AND recipient_audience_ref IS NULL AND privacy_policy_ref IS NULL
            AND resource_authorization_ref IS NULL
            AND authority_revision IS NULL)
        OR
        (space_id IS NOT NULL AND space_decision_ref IS NOT NULL
            AND recipient_audience_ref IS NOT NULL AND privacy_policy_ref IS NOT NULL
            AND resource_authorization_ref IS NOT NULL
            AND authority_revision IS NOT NULL AND authority_revision > 0)
    ) NOT VALID;

CREATE INDEX IF NOT EXISTS idx_threads_space_id
    ON threads (org_id, space_id, created_at)
    WHERE space_id IS NOT NULL;

ALTER TABLE runs
    ADD COLUMN IF NOT EXISTS space_id TEXT,
    ADD COLUMN IF NOT EXISTS space_decision_ref TEXT,
    ADD COLUMN IF NOT EXISTS recipient_audience_ref TEXT,
    ADD COLUMN IF NOT EXISTS privacy_policy_ref TEXT,
    ADD COLUMN IF NOT EXISTS resource_authorization_ref TEXT,
    ADD COLUMN IF NOT EXISTS authority_revision BIGINT;

ALTER TABLE runs
    ADD CONSTRAINT runs_space_context_complete_chk CHECK (
        (space_id IS NULL AND space_decision_ref IS NULL
            AND recipient_audience_ref IS NULL AND privacy_policy_ref IS NULL
            AND resource_authorization_ref IS NULL
            AND authority_revision IS NULL)
        OR
        (space_id IS NOT NULL AND space_decision_ref IS NOT NULL
            AND recipient_audience_ref IS NOT NULL AND privacy_policy_ref IS NOT NULL
            AND resource_authorization_ref IS NOT NULL
            AND authority_revision IS NOT NULL AND authority_revision > 0)
    ) NOT VALID;

CREATE INDEX IF NOT EXISTS idx_runs_space_id
    ON runs (org_id, space_id, created_at)
    WHERE space_id IS NOT NULL;
