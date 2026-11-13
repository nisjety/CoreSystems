-- Reverses 0040_org_permission_rules.up.sql.
--
-- Dropping the table loses every org permission rule. Turn CAPABILITY_PERMISSION_RULES_ENABLED off on capability-core
-- first: with the switch on and the table gone, every read fails, and the gate then asks for each effect-class call
-- (applyPermissionRules fails closed).

DROP INDEX IF EXISTS org_permission_rules_org_idx;
DROP TABLE IF EXISTS org_permission_rules;
