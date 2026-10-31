-- Least privilege for org_core_app on the tables migrations 021-023 create.
--
-- Migration 009 runs ALTER DEFAULT PRIVILEGES ... GRANT SELECT, INSERT, UPDATE,
-- DELETE ON TABLES TO org_core_app, so every table a later migration creates gets
-- all four privileges, whatever the GRANT line of that migration says: a GRANT only
-- adds. The grants stated in 021-023 were never the effective ones. The two
-- append-only history tables could be UPDATEd and DELETEd by the runtime role, the
-- SIEM audit outbox could be rewritten by it, and the revision rows that give each
-- write its compare-and-set could be deleted.
--
-- This migration makes the effective set explicit. Each table below is revoked in
-- full and then granted exactly what the org-core code runs as org_core_app (the
-- scoped transactions of DB.WithOrgScope, internal/database/database.go):
--
--   org_model_policies              SELECT, INSERT, DELETE  (a PUT replaces the set)
--   org_model_policy_revisions      SELECT, INSERT, UPDATE  (UPDATE and FOR UPDATE,
--                                                            never deleted)
--   org_model_policy_history        SELECT, INSERT          (append-only)
--   siem_destinations               SELECT, INSERT, UPDATE, DELETE
--   organization_siem_audit_outbox  SELECT, INSERT          (claimed and marked by
--                                                            the publisher on the
--                                                            owner pool, never here)
--   org_browser_site_policy         SELECT, INSERT, UPDATE  (never deleted: the
--                                                            revision must not reset)
--   org_browser_site_rules          SELECT, INSERT, DELETE  (a PUT replaces the rules)
--   user_browser_site_prefs         SELECT, INSERT, UPDATE, DELETE  (UPDATE for the
--                                                            ON CONFLICT upsert)
--   org_browser_site_history        SELECT, INSERT          (append-only)
--
-- SELECT stays on the history and outbox tables because the migrations that create
-- them state it as their read path. The default privileges are not changed, so a
-- table created by a later migration still starts with all four until that
-- migration narrows it. The down migration restores the all-four state.

REVOKE ALL ON org_model_policies FROM org_core_app;
GRANT SELECT, INSERT, DELETE ON org_model_policies TO org_core_app;

REVOKE ALL ON org_model_policy_revisions FROM org_core_app;
GRANT SELECT, INSERT, UPDATE ON org_model_policy_revisions TO org_core_app;

REVOKE ALL ON org_model_policy_history FROM org_core_app;
GRANT SELECT, INSERT ON org_model_policy_history TO org_core_app;

REVOKE ALL ON siem_destinations FROM org_core_app;
GRANT SELECT, INSERT, UPDATE, DELETE ON siem_destinations TO org_core_app;

REVOKE ALL ON organization_siem_audit_outbox FROM org_core_app;
GRANT SELECT, INSERT ON organization_siem_audit_outbox TO org_core_app;

REVOKE ALL ON org_browser_site_policy FROM org_core_app;
GRANT SELECT, INSERT, UPDATE ON org_browser_site_policy TO org_core_app;

REVOKE ALL ON org_browser_site_rules FROM org_core_app;
GRANT SELECT, INSERT, DELETE ON org_browser_site_rules TO org_core_app;

REVOKE ALL ON user_browser_site_prefs FROM org_core_app;
GRANT SELECT, INSERT, UPDATE, DELETE ON user_browser_site_prefs TO org_core_app;

REVOKE ALL ON org_browser_site_history FROM org_core_app;
GRANT SELECT, INSERT ON org_browser_site_history TO org_core_app;
