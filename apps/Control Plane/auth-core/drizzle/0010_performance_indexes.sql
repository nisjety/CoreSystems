-- Migration: 0010_performance_indexes.sql
-- Adds missing indexes on Better Auth managed tables
-- Run via: psql $DATABASE_URL -f this_file
-- All statements are idempotent (IF NOT EXISTS)

-- ============================================================
-- member table — queried on every org-scoped request
-- ============================================================
CREATE INDEX IF NOT EXISTS idx_member_user_id
  ON member(user_id);

CREATE INDEX IF NOT EXISTS idx_member_org_id
  ON member("organization_id");

-- Composite: common pattern is (user_id, organization_id) for membership check
CREATE INDEX IF NOT EXISTS idx_member_user_org
  ON member(user_id, "organization_id");

-- ============================================================
-- session table — token lookup on every authenticated request
-- UNIQUE(token) already ensures a B-tree index, but adding
-- a covering index for user_id → session lookups (e.g. revoke all sessions)
-- ============================================================
CREATE INDEX IF NOT EXISTS idx_session_user_id
  ON session(user_id);

-- Partial index: only non-expired sessions (reduces index size significantly)
CREATE INDEX IF NOT EXISTS idx_session_active
  ON session(expires_at)
  WHERE expires_at > NOW();

-- ============================================================
-- apikey table — rate-limit checks query by userId on every API call
-- ============================================================
CREATE INDEX IF NOT EXISTS idx_apikey_user_id
  ON apikey(user_id);

-- Partial: only active/enabled keys for rate limit lookups
CREATE INDEX IF NOT EXISTS idx_apikey_user_enabled
  ON apikey(user_id, enabled)
  WHERE enabled = true;

-- ============================================================
-- rate_limit table — key lookup on every request with rate limiting
-- ============================================================
CREATE INDEX IF NOT EXISTS idx_rate_limit_key
  ON rate_limit(key);
