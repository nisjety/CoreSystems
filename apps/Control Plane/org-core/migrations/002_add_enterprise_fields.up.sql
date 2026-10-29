-- Add quotas table for resource limits
CREATE TABLE IF NOT EXISTS org_quotas (
    org_id TEXT NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    quota_key TEXT NOT NULL,
    quota_value BIGINT NOT NULL,
    quota_limit BIGINT NOT NULL,
    reset_period TEXT, -- 'daily', 'monthly', 'none'
    last_reset_at TIMESTAMPTZ,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (org_id, quota_key)
);

-- Add billing flags table
CREATE TABLE IF NOT EXISTS org_billing (
    org_id TEXT PRIMARY KEY REFERENCES organizations(id) ON DELETE CASCADE,
    billing_email TEXT,
    payment_method_id TEXT,
    subscription_id TEXT,
    subscription_status TEXT, -- 'active', 'past_due', 'canceled', 'trialing'
    trial_ends_at TIMESTAMPTZ,
    current_period_start TIMESTAMPTZ,
    current_period_end TIMESTAMPTZ,
    auto_renew BOOLEAN NOT NULL DEFAULT true,
    billing_address JSONB,
    tax_id TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Add compliance settings table
CREATE TABLE IF NOT EXISTS org_compliance (
    org_id TEXT PRIMARY KEY REFERENCES organizations(id) ON DELETE CASCADE,
    data_residency TEXT, -- 'us', 'eu', 'asia', etc.
    gdpr_compliant BOOLEAN NOT NULL DEFAULT false,
    hipaa_compliant BOOLEAN NOT NULL DEFAULT false,
    soc2_compliant BOOLEAN NOT NULL DEFAULT false,
    data_retention_days INTEGER,
    require_mfa BOOLEAN NOT NULL DEFAULT false,
    ip_allowlist JSONB,
    audit_log_retention_days INTEGER DEFAULT 90,
    encryption_at_rest BOOLEAN NOT NULL DEFAULT true,
    encryption_in_transit BOOLEAN NOT NULL DEFAULT true,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Add role mappings table for granular permissions
CREATE TABLE IF NOT EXISTS org_role_mappings (
    id TEXT PRIMARY KEY,
    org_id TEXT NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    role_name TEXT NOT NULL, -- 'owner', 'admin', 'member', 'viewer', custom roles
    permissions JSONB NOT NULL DEFAULT '[]'::jsonb, -- Array of permission strings
    is_custom BOOLEAN NOT NULL DEFAULT false,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(org_id, role_name)
);

-- Add plan history table for auditing plan changes
CREATE TABLE IF NOT EXISTS org_plan_history (
    id TEXT PRIMARY KEY,
    org_id TEXT NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    previous_plan TEXT,
    new_plan TEXT NOT NULL,
    changed_by TEXT,
    change_reason TEXT,
    changed_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    metadata JSONB DEFAULT '{}'::jsonb
);

-- Add indexes for performance
CREATE INDEX IF NOT EXISTS idx_org_quotas_org_id ON org_quotas(org_id);
CREATE INDEX IF NOT EXISTS idx_org_billing_subscription_status ON org_billing(subscription_status);
CREATE INDEX IF NOT EXISTS idx_org_role_mappings_org_id ON org_role_mappings(org_id);
CREATE INDEX IF NOT EXISTS idx_org_plan_history_org_id ON org_plan_history(org_id);
CREATE INDEX IF NOT EXISTS idx_org_plan_history_changed_at ON org_plan_history(changed_at DESC);

-- Insert default quotas for existing organizations
INSERT INTO org_quotas (org_id, quota_key, quota_value, quota_limit, reset_period)
SELECT 
    id as org_id,
    'api_calls' as quota_key,
    0 as quota_value,
    CASE 
        WHEN plan IN ('free', 'trial') THEN 1000
        WHEN plan IN ('hobby', 'standard', 'pro') THEN 10000
        WHEN plan = 'enterprise' THEN 100000
        ELSE 1000
    END as quota_limit,
    'monthly' as reset_period
FROM organizations
ON CONFLICT (org_id, quota_key) DO NOTHING;

INSERT INTO org_quotas (org_id, quota_key, quota_value, quota_limit, reset_period)
SELECT 
    id as org_id,
    'users' as quota_key,
    0 as quota_value,
    CASE 
        WHEN plan IN ('free', 'trial') THEN 5
        WHEN plan IN ('hobby', 'standard', 'pro') THEN 50
        WHEN plan = 'enterprise' THEN -1
        ELSE 5
    END as quota_limit,
    'none' as reset_period
FROM organizations
ON CONFLICT (org_id, quota_key) DO NOTHING;

INSERT INTO org_quotas (org_id, quota_key, quota_value, quota_limit, reset_period)
SELECT 
    id as org_id,
    'storage_mb' as quota_key,
    0 as quota_value,
    CASE 
        WHEN plan IN ('free', 'trial') THEN 1000
        WHEN plan IN ('hobby', 'standard', 'pro') THEN 10000
        WHEN plan = 'enterprise' THEN -1
        ELSE 1000
    END as quota_limit,
    'none' as reset_period
FROM organizations
ON CONFLICT (org_id, quota_key) DO NOTHING;

-- Insert default billing records for existing organizations
INSERT INTO org_billing (org_id, subscription_status, auto_renew)
SELECT id, 'active', true
FROM organizations
ON CONFLICT (org_id) DO NOTHING;

-- Insert default compliance settings for existing organizations
INSERT INTO org_compliance (org_id, data_residency, gdpr_compliant)
SELECT id, 'us', false
FROM organizations
ON CONFLICT (org_id) DO NOTHING;

-- Insert default role mappings for existing organizations
INSERT INTO org_role_mappings (id, org_id, role_name, permissions, is_custom)
SELECT 
    org_id || '_owner' as id,
    org_id,
    'owner' as role_name,
    '["org:delete", "org:update", "members:invite", "members:remove", "billing:manage", "roles:manage"]'::jsonb as permissions,
    false as is_custom
FROM (SELECT DISTINCT id as org_id FROM organizations) orgs
ON CONFLICT (org_id, role_name) DO NOTHING;

INSERT INTO org_role_mappings (id, org_id, role_name, permissions, is_custom)
SELECT 
    org_id || '_admin' as id,
    org_id,
    'admin' as role_name,
    '["org:update", "members:invite", "members:remove", "roles:manage"]'::jsonb as permissions,
    false as is_custom
FROM (SELECT DISTINCT id as org_id FROM organizations) orgs
ON CONFLICT (org_id, role_name) DO NOTHING;

INSERT INTO org_role_mappings (id, org_id, role_name, permissions, is_custom)
SELECT 
    org_id || '_member' as id,
    org_id,
    'member' as role_name,
    '["org:read", "resources:create", "resources:read", "resources:update"]'::jsonb as permissions,
    false as is_custom
FROM (SELECT DISTINCT id as org_id FROM organizations) orgs
ON CONFLICT (org_id, role_name) DO NOTHING;

INSERT INTO org_role_mappings (id, org_id, role_name, permissions, is_custom)
SELECT 
    org_id || '_viewer' as id,
    org_id,
    'viewer' as role_name,
    '["org:read", "resources:read"]'::jsonb as permissions,
    false as is_custom
FROM (SELECT DISTINCT id as org_id FROM organizations) orgs
ON CONFLICT (org_id, role_name) DO NOTHING;
