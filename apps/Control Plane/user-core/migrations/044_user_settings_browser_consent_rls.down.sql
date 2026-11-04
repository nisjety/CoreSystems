-- 044_user_settings_browser_consent_rls (down): remove the user-scope policy
-- and turn RLS back off on user_settings. Stored browser_consent rows stay.

BEGIN;

DROP POLICY IF EXISTS user_settings_user_scope ON user_settings;
ALTER TABLE user_settings NO FORCE ROW LEVEL SECURITY;
ALTER TABLE user_settings DISABLE ROW LEVEL SECURITY;

COMMIT;
