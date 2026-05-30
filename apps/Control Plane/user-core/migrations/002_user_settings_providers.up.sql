-- Migration: 002_user_settings_providers.up.sql
-- Adds user_settings (persistent per-category JSONB) and provider_accounts (OAuth social login links)

-- User settings table: one row per user per category (appearance, language, privacy, notifications)
CREATE TABLE IF NOT EXISTS user_settings (
    id          UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    user_id     UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    category    VARCHAR(50) NOT NULL,
    settings    JSONB NOT NULL DEFAULT '{}',
    created_at  TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    updated_at  TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW()
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_user_settings_user_category ON user_settings(user_id, category);
CREATE INDEX IF NOT EXISTS idx_user_settings_user_id ON user_settings(user_id);

-- Provider accounts table: links a local user to one or more OAuth provider identities
-- e.g. Microsoft AAD, Google, GitHub — whatever providers auth-core supports
CREATE TABLE IF NOT EXISTS provider_accounts (
    id               UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    user_id          UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    provider         VARCHAR(50)  NOT NULL,  -- 'microsoft', 'google', 'github', etc.
    provider_user_id VARCHAR(255) NOT NULL,  -- stable ID from the provider
    tenant_id        VARCHAR(255),           -- Microsoft AAD tenant / Google Workspace domain
    email            VARCHAR(255),           -- provider-side email (may differ from login email)
    display_name     VARCHAR(255),           -- name as returned by the provider
    metadata         JSONB,                  -- any extra provider-specific claims (job title, dept, photo URL, etc.)
    created_at       TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    updated_at       TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW()
);

-- A provider identity can only belong to one local user
CREATE UNIQUE INDEX IF NOT EXISTS idx_provider_accounts_provider_uid ON provider_accounts(provider, provider_user_id);
CREATE INDEX IF NOT EXISTS idx_provider_accounts_user_id ON provider_accounts(user_id);
CREATE INDEX IF NOT EXISTS idx_provider_accounts_provider ON provider_accounts(provider);
