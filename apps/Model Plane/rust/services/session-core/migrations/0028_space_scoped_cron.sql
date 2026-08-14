-- Space-scoped cron is an explicit new contract. Existing org-wide schedule
-- rows remain structurally distinguishable by their empty bindings and must be
-- fail-closed by Capability Core rather than being silently treated as scoped.
ALTER TABLE cron_schedules
    ADD COLUMN IF NOT EXISTS space_ref TEXT NOT NULL DEFAULT '',
    ADD COLUMN IF NOT EXISTS creator_subject_id TEXT NOT NULL DEFAULT '',
    ADD COLUMN IF NOT EXISTS recipient_audience_ref TEXT NOT NULL DEFAULT '',
    ADD COLUMN IF NOT EXISTS recipient_audience_hash TEXT NOT NULL DEFAULT '',
    ADD COLUMN IF NOT EXISTS resource_authorization_ref TEXT NOT NULL DEFAULT '',
    ADD COLUMN IF NOT EXISTS credential_grant_ref TEXT NOT NULL DEFAULT '',
    ADD COLUMN IF NOT EXISTS delivery_target_ref TEXT NOT NULL DEFAULT '',
    ADD COLUMN IF NOT EXISTS approval_policy_ref TEXT NOT NULL DEFAULT '',
    ADD COLUMN IF NOT EXISTS privacy_policy_ref TEXT NOT NULL DEFAULT '',
    ADD COLUMN IF NOT EXISTS authority_revision BIGINT NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS membership_revision BIGINT NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS privacy_revision BIGINT NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS recipient_audience_revision BIGINT NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS entitlement_revision BIGINT NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS template_digest TEXT NOT NULL DEFAULT '';

CREATE INDEX IF NOT EXISTS cron_schedules_space_due_idx
    ON cron_schedules (space_ref, next_fire_at, enabled)
    WHERE deleted_at IS NULL;
