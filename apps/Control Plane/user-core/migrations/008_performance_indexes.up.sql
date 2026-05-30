-- Migration: 008_performance_indexes.up.sql
-- Adds missing indexes on tables from migrations 005-007
-- All CREATE INDEX statements are idempotent (IF NOT EXISTS)

-- ============================================================
-- user_activity_log (added in 006_add_activity_log.up.sql)
-- ListActivities: WHERE user_id=$1 ORDER BY created_at DESC LIMIT/OFFSET
-- ============================================================
CREATE INDEX IF NOT EXISTS idx_activity_log_user_created
  ON user_activity_log(user_id, created_at DESC);

-- ============================================================
-- user_org_memberships (added in 005_enterprise_identity_memberships.up.sql)
-- GetPrimaryUserOrgMembership: WHERE user_id=$1 AND status='active'
-- Partial index on active records only — keeps index small and fast
-- ============================================================
CREATE INDEX IF NOT EXISTS idx_uom_user_active
  ON user_org_memberships(user_id, status)
  WHERE status = 'active';

-- NOTE: user_api_keys already has idx_user_api_keys_user_id and
-- idx_user_api_keys_prefix created in 007_api_keys.up.sql — no duplicates needed.
