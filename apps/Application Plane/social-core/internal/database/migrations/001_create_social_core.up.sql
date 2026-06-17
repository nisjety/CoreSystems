CREATE TABLE IF NOT EXISTS social_accounts (
    id TEXT PRIMARY KEY,
    org_id TEXT NOT NULL,
    provider_key TEXT NOT NULL,
    connection_id TEXT NOT NULL DEFAULT '',
    display_name TEXT NOT NULL DEFAULT '',
    handle TEXT NOT NULL DEFAULT '',
    status TEXT NOT NULL DEFAULT 'disconnected',
    capabilities JSONB NOT NULL DEFAULT '[]'::jsonb,
    token_state TEXT NOT NULL DEFAULT 'missing',
    token_expires_at TIMESTAMPTZ,
    metadata JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CHECK (status IN ('connected', 'disconnected', 'expired', 'error')),
    CHECK (jsonb_typeof(capabilities) = 'array'),
    CHECK (jsonb_typeof(metadata) = 'object')
);

CREATE UNIQUE INDEX IF NOT EXISTS social_accounts_org_provider_connection_unique
    ON social_accounts (org_id, provider_key, connection_id)
    WHERE connection_id <> '';
CREATE INDEX IF NOT EXISTS social_accounts_org_provider_idx
    ON social_accounts (org_id, provider_key, status);

CREATE TABLE IF NOT EXISTS social_campaigns (
    id TEXT PRIMARY KEY,
    org_id TEXT NOT NULL,
    name TEXT NOT NULL,
    brief TEXT NOT NULL DEFAULT '',
    goal TEXT NOT NULL DEFAULT '',
    status TEXT NOT NULL DEFAULT 'draft',
    platforms JSONB NOT NULL DEFAULT '[]'::jsonb,
    starts_at TIMESTAMPTZ,
    ends_at TIMESTAMPTZ,
    source JSONB NOT NULL DEFAULT '{}'::jsonb,
    metadata JSONB NOT NULL DEFAULT '{}'::jsonb,
    owner_user_id TEXT NOT NULL DEFAULT '',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CHECK (status IN ('draft', 'active', 'completed', 'archived')),
    CHECK (jsonb_typeof(platforms) = 'array'),
    CHECK (jsonb_typeof(source) = 'object'),
    CHECK (jsonb_typeof(metadata) = 'object'),
    CHECK (ends_at IS NULL OR starts_at IS NULL OR ends_at >= starts_at)
);

CREATE INDEX IF NOT EXISTS social_campaigns_org_status_idx
    ON social_campaigns (org_id, status, starts_at ASC NULLS LAST, updated_at DESC);

CREATE TABLE IF NOT EXISTS social_posts (
    id TEXT PRIMARY KEY,
    org_id TEXT NOT NULL,
    title TEXT NOT NULL DEFAULT '',
    body TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'draft',
    platforms JSONB NOT NULL DEFAULT '[]'::jsonb,
    media JSONB NOT NULL DEFAULT '[]'::jsonb,
    source JSONB NOT NULL DEFAULT '{}'::jsonb,
    previews JSONB NOT NULL DEFAULT '[]'::jsonb,
    ai_context JSONB NOT NULL DEFAULT '{}'::jsonb,
    approval_required BOOLEAN NOT NULL DEFAULT TRUE,
    approval_state TEXT NOT NULL DEFAULT 'pending',
    scheduled_at TIMESTAMPTZ,
    created_by_user_id TEXT NOT NULL DEFAULT '',
    updated_by_user_id TEXT NOT NULL DEFAULT '',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CHECK (status IN ('draft', 'scheduled', 'publishing', 'published', 'failed', 'blocked', 'archived')),
    CHECK (approval_state IN ('pending', 'approved', 'rejected', 'not_required')),
    CHECK (jsonb_typeof(platforms) = 'array'),
    CHECK (jsonb_typeof(media) = 'array'),
    CHECK (jsonb_typeof(source) = 'object'),
    CHECK (jsonb_typeof(previews) = 'array'),
    CHECK (jsonb_typeof(ai_context) = 'object')
);

CREATE INDEX IF NOT EXISTS social_posts_org_status_scheduled_idx
    ON social_posts (org_id, status, scheduled_at ASC NULLS LAST, updated_at DESC);
CREATE INDEX IF NOT EXISTS social_posts_org_updated_idx
    ON social_posts (org_id, updated_at DESC);
CREATE INDEX IF NOT EXISTS social_posts_platforms_gin_idx
    ON social_posts USING GIN (platforms);

CREATE TABLE IF NOT EXISTS social_approvals (
    id TEXT PRIMARY KEY,
    org_id TEXT NOT NULL,
    post_id TEXT NOT NULL DEFAULT '' REFERENCES social_posts(id) ON DELETE CASCADE,
    campaign_id TEXT NOT NULL DEFAULT '',
    state TEXT NOT NULL DEFAULT 'pending',
    requested_by_user_id TEXT NOT NULL DEFAULT '',
    requested_of_user_id TEXT NOT NULL DEFAULT '',
    decided_by_user_id TEXT NOT NULL DEFAULT '',
    decision_reason TEXT NOT NULL DEFAULT '',
    due_at TIMESTAMPTZ,
    decided_at TIMESTAMPTZ,
    metadata JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CHECK (state IN ('pending', 'approved', 'rejected')),
    CHECK (jsonb_typeof(metadata) = 'object')
);

CREATE UNIQUE INDEX IF NOT EXISTS social_approvals_org_post_pending_unique
    ON social_approvals (org_id, post_id)
    WHERE state = 'pending' AND post_id <> '';
CREATE INDEX IF NOT EXISTS social_approvals_org_state_idx
    ON social_approvals (org_id, state, created_at ASC);
CREATE INDEX IF NOT EXISTS social_approvals_org_campaign_idx
    ON social_approvals (org_id, campaign_id, created_at DESC)
    WHERE campaign_id <> '';

CREATE TABLE IF NOT EXISTS social_publish_jobs (
    id TEXT PRIMARY KEY,
    org_id TEXT NOT NULL,
    post_id TEXT NOT NULL REFERENCES social_posts(id) ON DELETE CASCADE,
    status TEXT NOT NULL DEFAULT 'queued',
    idempotency_key TEXT NOT NULL,
    requested_by_user_id TEXT NOT NULL DEFAULT '',
    scheduled_for TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    locked_at TIMESTAMPTZ,
    locked_by TEXT NOT NULL DEFAULT '',
    attempts INTEGER NOT NULL DEFAULT 0,
    last_error TEXT NOT NULL DEFAULT '',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CHECK (status IN ('queued', 'running', 'completed', 'failed', 'blocked', 'canceled'))
);

CREATE UNIQUE INDEX IF NOT EXISTS social_publish_jobs_org_idempotency_unique
    ON social_publish_jobs (org_id, idempotency_key);
CREATE INDEX IF NOT EXISTS social_publish_jobs_queue_idx
    ON social_publish_jobs (status, scheduled_for ASC, created_at ASC)
    WHERE status = 'queued';
CREATE INDEX IF NOT EXISTS social_publish_jobs_post_idx
    ON social_publish_jobs (org_id, post_id, created_at DESC);

CREATE TABLE IF NOT EXISTS social_publish_attempts (
    id TEXT PRIMARY KEY,
    org_id TEXT NOT NULL,
    job_id TEXT NOT NULL REFERENCES social_publish_jobs(id) ON DELETE CASCADE,
    post_id TEXT NOT NULL REFERENCES social_posts(id) ON DELETE CASCADE,
    provider_key TEXT NOT NULL,
    status TEXT NOT NULL,
    mode TEXT NOT NULL DEFAULT 'api',
    endpoint TEXT NOT NULL DEFAULT '',
    external_id TEXT NOT NULL DEFAULT '',
    message TEXT NOT NULL DEFAULT '',
    warnings JSONB NOT NULL DEFAULT '[]'::jsonb,
    response JSONB NOT NULL DEFAULT '{}'::jsonb,
    attempted_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CHECK (status IN ('queued', 'running', 'succeeded', 'failed', 'blocked')),
    CHECK (jsonb_typeof(warnings) = 'array'),
    CHECK (jsonb_typeof(response) = 'object')
);

CREATE INDEX IF NOT EXISTS social_publish_attempts_job_idx
    ON social_publish_attempts (job_id, created_at ASC);
CREATE INDEX IF NOT EXISTS social_publish_attempts_org_provider_idx
    ON social_publish_attempts (org_id, provider_key, created_at DESC);

CREATE TABLE IF NOT EXISTS social_audit_events (
    id TEXT PRIMARY KEY,
    org_id TEXT NOT NULL,
    post_id TEXT NOT NULL DEFAULT '',
    actor_user_id TEXT NOT NULL DEFAULT '',
    action TEXT NOT NULL,
    payload JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CHECK (jsonb_typeof(payload) = 'object')
);

CREATE INDEX IF NOT EXISTS social_audit_events_org_created_idx
    ON social_audit_events (org_id, created_at DESC);
