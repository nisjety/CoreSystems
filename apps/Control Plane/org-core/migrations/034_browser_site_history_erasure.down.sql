-- Reverses 034. The redacted history rows stay redacted: the tombstone is what the
-- erasure left, and restoring an identity is not possible. Rolling back removes only
-- the trigger, the redaction function and the column grant, so the runtime role goes
-- back to INSERT and SELECT on the history, and append-only again.
DROP TRIGGER IF EXISTS org_browser_site_history_erasure_only ON org_browser_site_history;
DROP FUNCTION IF EXISTS org_browser_site_history_redact_only();
REVOKE UPDATE (subject_user_id, actor_id, before_set, after_set) ON org_browser_site_history FROM org_core_app;
