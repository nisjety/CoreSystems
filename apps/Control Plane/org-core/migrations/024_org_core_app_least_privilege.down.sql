-- Revert 024. Gives org_core_app the four privileges that migration 009's default
-- privileges gave the 021-023 tables, which is the effective state before this
-- migration. This restores the over-grant so the chain can be rolled back; it is
-- not a state to keep.

GRANT SELECT, INSERT, UPDATE, DELETE ON org_model_policies TO org_core_app;
GRANT SELECT, INSERT, UPDATE, DELETE ON org_model_policy_revisions TO org_core_app;
GRANT SELECT, INSERT, UPDATE, DELETE ON org_model_policy_history TO org_core_app;
GRANT SELECT, INSERT, UPDATE, DELETE ON siem_destinations TO org_core_app;
GRANT SELECT, INSERT, UPDATE, DELETE ON organization_siem_audit_outbox TO org_core_app;
GRANT SELECT, INSERT, UPDATE, DELETE ON org_browser_site_policy TO org_core_app;
GRANT SELECT, INSERT, UPDATE, DELETE ON org_browser_site_rules TO org_core_app;
GRANT SELECT, INSERT, UPDATE, DELETE ON user_browser_site_prefs TO org_core_app;
GRANT SELECT, INSERT, UPDATE, DELETE ON org_browser_site_history TO org_core_app;
