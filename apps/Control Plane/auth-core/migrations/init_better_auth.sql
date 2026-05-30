-- Better Auth Database Schema Migration
-- Creates all required tables for Better Auth with plugins

-- User table
CREATE TABLE IF NOT EXISTS "user" (
  "id" TEXT PRIMARY KEY,
  "name" TEXT NOT NULL,
  "email" TEXT NOT NULL UNIQUE,
  "email_verified" BOOLEAN NOT NULL DEFAULT FALSE,
  "image" TEXT,
  "phone_number" TEXT UNIQUE,
  "phone_number_verified" BOOLEAN,
  "two_factor_enabled" BOOLEAN DEFAULT FALSE,
  "role" TEXT,
  "banned" BOOLEAN DEFAULT FALSE,
  "ban_reason" TEXT,
  "ban_expires" TIMESTAMP,
  "created_at" TIMESTAMP NOT NULL DEFAULT NOW(),
  "updated_at" TIMESTAMP NOT NULL DEFAULT NOW()
);

-- Session table
CREATE TABLE IF NOT EXISTS "session" (
  "id" TEXT PRIMARY KEY,
  "expires_at" TIMESTAMP NOT NULL,
  "token" TEXT NOT NULL UNIQUE,
  "created_at" TIMESTAMP NOT NULL,
  "updated_at" TIMESTAMP NOT NULL,
  "ip_address" TEXT,
  "user_agent" TEXT,
  "user_id" TEXT NOT NULL REFERENCES "user"("id") ON DELETE CASCADE,
  "active_organization_id" TEXT,
  "active_team_id" TEXT,
  "impersonated_by" TEXT
);

-- Account table (for OAuth providers)
CREATE TABLE IF NOT EXISTS "account" (
  "id" TEXT PRIMARY KEY,
  "account_id" TEXT NOT NULL,
  "provider_id" TEXT NOT NULL,
  "user_id" TEXT NOT NULL REFERENCES "user"("id") ON DELETE CASCADE,
  "access_token" TEXT,
  "refresh_token" TEXT,
  "id_token" TEXT,
  "access_token_expires_at" TIMESTAMP,
  "refresh_token_expires_at" TIMESTAMP,
  "scope" TEXT,
  "password" TEXT,
  "created_at" TIMESTAMP NOT NULL,
  "updated_at" TIMESTAMP NOT NULL
);

-- Verification table
CREATE TABLE IF NOT EXISTS "verification" (
  "id" TEXT PRIMARY KEY,
  "identifier" TEXT NOT NULL,
  "value" TEXT NOT NULL,
  "expires_at" TIMESTAMP NOT NULL,
  "created_at" TIMESTAMP DEFAULT NOW(),
  "updated_at" TIMESTAMP DEFAULT NOW()
);

-- Two-factor authentication table
CREATE TABLE IF NOT EXISTS "two_factor" (
  "id" TEXT PRIMARY KEY,
  "user_id" TEXT NOT NULL REFERENCES "user"("id") ON DELETE CASCADE,
  "secret" TEXT NOT NULL,
  "backup_codes" TEXT NOT NULL
);

-- Passkey authentication table
CREATE TABLE IF NOT EXISTS "passkey" (
  "id" TEXT PRIMARY KEY,
  "name" TEXT,
  "public_key" TEXT NOT NULL,
  "user_id" TEXT NOT NULL REFERENCES "user"("id") ON DELETE CASCADE,
  "credential_i_d" TEXT NOT NULL,
  "counter" INTEGER NOT NULL,
  "device_type" TEXT NOT NULL,
  "backed_up" BOOLEAN NOT NULL,
  "transports" TEXT,
  "created_at" TIMESTAMP NOT NULL DEFAULT NOW(),
  "aaguid" TEXT
);

-- Rate limiting table
CREATE TABLE IF NOT EXISTS "rate_limit" (
  "id" TEXT PRIMARY KEY,
  "key" TEXT,
  "count" INTEGER,
  "last_request" BIGINT
);

-- Organization table
CREATE TABLE IF NOT EXISTS "organization" (
  "id" TEXT PRIMARY KEY,
  "name" TEXT NOT NULL,
  "slug" TEXT UNIQUE,
  "logo" TEXT,
  "metadata" TEXT,
  "created_at" TIMESTAMP NOT NULL
);

-- Organization members table
CREATE TABLE IF NOT EXISTS "member" (
  "id" TEXT PRIMARY KEY,
  "organization_id" TEXT NOT NULL REFERENCES "organization"("id") ON DELETE CASCADE,
  "user_id" TEXT NOT NULL REFERENCES "user"("id") ON DELETE CASCADE,
  "role" TEXT NOT NULL DEFAULT 'member',
  "created_at" TIMESTAMP NOT NULL
);

-- Organization invitations table
CREATE TABLE IF NOT EXISTS "invitation" (
  "id" TEXT PRIMARY KEY,
  "organization_id" TEXT NOT NULL REFERENCES "organization"("id") ON DELETE CASCADE,
  "email" TEXT NOT NULL,
  "role" TEXT,
  "status" TEXT NOT NULL DEFAULT 'pending',
  "expires_at" TIMESTAMP NOT NULL,
  "inviter_id" TEXT NOT NULL REFERENCES "user"("id") ON DELETE CASCADE,
  "team_id" TEXT
);

-- Team management table
CREATE TABLE IF NOT EXISTS "team" (
  "id" TEXT PRIMARY KEY,
  "name" TEXT NOT NULL,
  "organization_id" TEXT NOT NULL REFERENCES "organization"("id") ON DELETE CASCADE,
  "created_at" TIMESTAMP NOT NULL,
  "updated_at" TIMESTAMP
);

-- Team members table
CREATE TABLE IF NOT EXISTS "team_member" (
  "id" TEXT PRIMARY KEY,
  "team_id" TEXT NOT NULL REFERENCES "team"("id") ON DELETE CASCADE,
  "user_id" TEXT NOT NULL REFERENCES "user"("id") ON DELETE CASCADE,
  "created_at" TIMESTAMP NOT NULL
);

-- SSO provider table
CREATE TABLE IF NOT EXISTS "sso_provider" (
  "id" TEXT PRIMARY KEY,
  "issuer" TEXT NOT NULL,
  "domain" TEXT NOT NULL,
  "provider_id" TEXT NOT NULL UNIQUE,
  "oidc_config" TEXT,
  "saml_config" TEXT,
  "user_id" TEXT REFERENCES "user"("id") ON DELETE CASCADE,
  "organization_id" TEXT
);

-- OAuth application table
CREATE TABLE IF NOT EXISTS "oauth_application" (
  "id" TEXT PRIMARY KEY,
  "client_id" TEXT UNIQUE,
  "client_secret" TEXT,
  "name" TEXT,
  "redirect_u_r_ls" TEXT,
  "metadata" TEXT,
  "type" TEXT,
  "disabled" BOOLEAN,
  "user_id" TEXT,
  "created_at" TIMESTAMP,
  "updated_at" TIMESTAMP
);

-- OAuth access token table
CREATE TABLE IF NOT EXISTS "oauth_access_token" (
  "id" TEXT PRIMARY KEY,
  "access_token" TEXT UNIQUE,
  "refresh_token" TEXT UNIQUE,
  "access_token_expires_at" TIMESTAMP,
  "refresh_token_expires_at" TIMESTAMP,
  "client_id" TEXT,
  "user_id" TEXT,
  "scopes" TEXT,
  "created_at" TIMESTAMP,
  "updated_at" TIMESTAMP
);

-- OAuth consent table
CREATE TABLE IF NOT EXISTS "oauth_consent" (
  "id" TEXT PRIMARY KEY,
  "user_id" TEXT,
  "client_id" TEXT,
  "scopes" TEXT,
  "consent_given" BOOLEAN,
  "created_at" TIMESTAMP,
  "updated_at" TIMESTAMP
);

-- API key table
CREATE TABLE IF NOT EXISTS "apikey" (
  "id" TEXT PRIMARY KEY,
  "name" TEXT,
  "start" TEXT,
  "prefix" TEXT,
  "key" TEXT NOT NULL,
  "user_id" TEXT NOT NULL REFERENCES "user"("id") ON DELETE CASCADE,
  "refill_interval" INTEGER,
  "refill_amount" INTEGER,
  "last_refill_at" TIMESTAMP,
  "enabled" BOOLEAN DEFAULT TRUE,
  "rate_limit_enabled" BOOLEAN DEFAULT TRUE,
  "rate_limit_time_window" INTEGER DEFAULT 86400000,
  "rate_limit_max" INTEGER DEFAULT 10,
  "request_count" INTEGER DEFAULT 0,
  "remaining" INTEGER,
  "last_request" TIMESTAMP,
  "expires_at" TIMESTAMP,
  "created_at" TIMESTAMP NOT NULL,
  "updated_at" TIMESTAMP NOT NULL,
  "permissions" TEXT,
  "metadata" TEXT
);

-- Create indexes for better performance
CREATE INDEX IF NOT EXISTS idx_session_user_id ON "session"("user_id");
CREATE INDEX IF NOT EXISTS idx_account_user_id ON "account"("user_id");
CREATE INDEX IF NOT EXISTS idx_member_org_id ON "member"("organization_id");
CREATE INDEX IF NOT EXISTS idx_member_user_id ON "member"("user_id");
CREATE INDEX IF NOT EXISTS idx_apikey_user_id ON "apikey"("user_id");
