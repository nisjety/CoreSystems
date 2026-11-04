-- 044_user_settings_browser_consent_rls: user-scoped RLS on user_settings for
-- the browser_consent category (W3 E3.2-e3, OD13 / DEC-19 per-origin consent).
--
-- A browser_consent row holds one person's per-origin consent decisions. The
-- HTTP layer reads and writes it only inside a transaction that sets
--     SELECT set_config('app.current_user_id', '<user id>', true);
-- (users.Repository.withUserScope). The policy below then admits only that
-- person's rows. The policy covers the whole table; today only the
-- browser_consent paths set the GUC.
--
-- Shape follows org-core 008: a row is admitted when EITHER the GUC matches the
-- row's user_id OR the GUC is unset/empty. The unset branch keeps every
-- existing unscoped path working unchanged: the other settings categories,
-- GDPR erasure (anonymize deletes user_settings by user_id; hard delete
-- cascades from users), DSAR and migrations. A scoped request gains a hard
-- database-level user filter. A stricter posture (deny when unset) can follow
-- once every user_settings path sets the GUC.
--
-- FORCE is set so the policy also binds the table owner (user-core connects as
-- the owner that ran the migrations). A superuser connection still bypasses
-- RLS; the application-level user filter stays the primary guard.
--
-- Idempotent: every statement tolerates a re-run.

BEGIN;

ALTER TABLE user_settings ENABLE ROW LEVEL SECURITY;
ALTER TABLE user_settings FORCE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS user_settings_user_scope ON user_settings;
CREATE POLICY user_settings_user_scope ON user_settings
    USING (
        COALESCE(current_setting('app.current_user_id', true), '') = ''
        OR user_id = current_setting('app.current_user_id', true)
    )
    WITH CHECK (
        COALESCE(current_setting('app.current_user_id', true), '') = ''
        OR user_id = current_setting('app.current_user_id', true)
    );

COMMIT;
