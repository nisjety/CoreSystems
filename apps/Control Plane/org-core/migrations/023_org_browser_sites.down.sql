-- Revert WS21-S3. Nothing reads these tables until the broker's site-policy client
-- is switched on (WS21 S4), so dropping them loses only organisation configuration
-- and members' remembered choices, which an admin and a member can re-enter.
DROP TABLE IF EXISTS org_browser_site_history;
DROP TABLE IF EXISTS user_browser_site_prefs;
DROP TABLE IF EXISTS org_browser_site_rules;
DROP TABLE IF EXISTS org_browser_site_policy;
