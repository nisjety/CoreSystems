BEGIN;

DROP INDEX IF EXISTS capability_scopes_active_grant_uq;
DROP INDEX IF EXISTS capability_scopes_org_scope_idx;
DROP INDEX IF EXISTS capability_scopes_org_cap_idx;

ALTER TABLE capability_scopes
    DROP CONSTRAINT IF EXISTS capability_scopes_org_value_check,
    DROP CONSTRAINT IF EXISTS capability_scopes_scope_kind_check,
    DROP CONSTRAINT IF EXISTS capability_scopes_org_id_nonempty_check,
    DROP COLUMN IF EXISTS org_id;

ALTER TABLE models
    DROP CONSTRAINT IF EXISTS models_risk_level_check;

ALTER TABLE capabilities
    DROP CONSTRAINT IF EXISTS capabilities_risk_level_check;

COMMIT;
