-- Rollback migration for enterprise fields

DROP TABLE IF EXISTS org_plan_history;
DROP TABLE IF EXISTS org_role_mappings;
DROP TABLE IF EXISTS org_compliance;
DROP TABLE IF EXISTS org_billing;
DROP TABLE IF EXISTS org_quotas;
