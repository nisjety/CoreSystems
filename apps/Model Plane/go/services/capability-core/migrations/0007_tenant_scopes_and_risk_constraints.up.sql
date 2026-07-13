BEGIN;

-- Unknown legacy risk values previously inherited low-risk policy behavior.
-- Preserve the original value for investigation, then quarantine the row and
-- normalize it to the strictest supported risk before installing the write
-- constraint.
UPDATE capabilities
SET config_json = jsonb_set(
        CASE WHEN jsonb_typeof(config_json) = 'object' THEN config_json ELSE '{}'::jsonb END,
        '{quarantined_legacy_risk_level}',
        to_jsonb(risk_level),
        true
    ),
    risk_level = 'high',
    enabled = FALSE,
    rollout_state = 'quarantine',
    availability_state = 'unavailable',
    availability_reason_code = 'invalid_risk_level',
    availability_reason = 'Legacy capability risk metadata was invalid and has been quarantined.',
    execution_mode = 'unavailable',
    cost_class = 'unknown',
    health_checked_at = NULL,
    updated_at = now()
WHERE risk_level NOT IN ('low', 'medium', 'high');

ALTER TABLE capabilities
    DROP CONSTRAINT IF EXISTS capabilities_risk_level_check,
    ADD CONSTRAINT capabilities_risk_level_check
        CHECK (risk_level IN ('low', 'medium', 'high'));

UPDATE models
SET config_json = jsonb_set(
        CASE WHEN jsonb_typeof(config_json) = 'object' THEN config_json ELSE '{}'::jsonb END,
        '{quarantined_legacy_risk_level}',
        to_jsonb(risk_level),
        true
    ),
    risk_level = 'high',
    enabled = FALSE,
    updated_at = now()
WHERE risk_level NOT IN ('low', 'medium', 'high');

ALTER TABLE models
    DROP CONSTRAINT IF EXISTS models_risk_level_check,
    ADD CONSTRAINT models_risk_level_check
        CHECK (risk_level IN ('low', 'medium', 'high'));

-- Every durable scope grant is owned by one tenant, including grants against
-- a global catalog entry. Tenant-owned capability grants can be attributed to
-- the capability owner. A global capability's exact org grant can be safely
-- attributed to that org. Other legacy global grants are ambiguous and are
-- revoked rather than guessed.
ALTER TABLE capability_scopes
    ADD COLUMN IF NOT EXISTS org_id TEXT;

UPDATE capability_scopes AS cs
SET org_id = CASE
        WHEN c.org_id <> 'global' THEN c.org_id
        WHEN cs.scope_kind = 'org' AND cs.scope_value <> '*' THEN cs.scope_value
        ELSE NULL
    END
FROM capabilities AS c
WHERE c.id = cs.capability_id
  AND cs.org_id IS NULL;

UPDATE capability_scopes
SET revoked_at = COALESCE(revoked_at, now())
WHERE org_id IS NULL
   OR org_id = ''
   OR scope_kind NOT IN ('run', 'thread', 'workspace', 'user', 'org', 'global', 'agent')
   OR (scope_kind = 'org' AND scope_value <> org_id);

UPDATE capability_scopes
SET org_id = '__quarantined_legacy__'
WHERE org_id IS NULL OR org_id = '';

-- The constraints below intentionally cover revoked rows as well as active
-- grants, so quarantined legacy values must be structurally valid. Use the
-- non-authorizing global kind as a sentinel while retaining the original
-- scope_value for investigation. The revoked_at boundary keeps these rows
-- ineligible for every grant lookup and partial active-grant index.
UPDATE capability_scopes
SET scope_kind = 'global'
WHERE revoked_at IS NOT NULL
  AND (
      scope_kind NOT IN ('run', 'thread', 'workspace', 'user', 'org', 'global', 'agent')
      OR (scope_kind = 'org' AND scope_value <> org_id)
  );

WITH duplicate_grants AS (
    SELECT id,
           row_number() OVER (
               PARTITION BY org_id, capability_id, scope_kind, scope_value
               ORDER BY granted_at DESC, id DESC
           ) AS duplicate_rank
    FROM capability_scopes
    WHERE revoked_at IS NULL
)
UPDATE capability_scopes AS cs
SET revoked_at = COALESCE(cs.revoked_at, now())
FROM duplicate_grants AS duplicate
WHERE duplicate.id = cs.id
  AND duplicate.duplicate_rank > 1;

ALTER TABLE capability_scopes
    ALTER COLUMN org_id SET NOT NULL,
    DROP CONSTRAINT IF EXISTS capability_scopes_org_id_nonempty_check,
    ADD CONSTRAINT capability_scopes_org_id_nonempty_check CHECK (org_id <> ''),
    DROP CONSTRAINT IF EXISTS capability_scopes_scope_kind_check,
    ADD CONSTRAINT capability_scopes_scope_kind_check CHECK (
        scope_kind IN ('run', 'thread', 'workspace', 'user', 'org', 'global', 'agent')
    ),
    DROP CONSTRAINT IF EXISTS capability_scopes_org_value_check,
    ADD CONSTRAINT capability_scopes_org_value_check CHECK (
        scope_kind <> 'org' OR scope_value = org_id
    );

CREATE INDEX IF NOT EXISTS capability_scopes_org_cap_idx
    ON capability_scopes (org_id, capability_id)
    WHERE revoked_at IS NULL;

CREATE INDEX IF NOT EXISTS capability_scopes_org_scope_idx
    ON capability_scopes (org_id, scope_kind, scope_value)
    WHERE revoked_at IS NULL;

CREATE UNIQUE INDEX IF NOT EXISTS capability_scopes_active_grant_uq
    ON capability_scopes (org_id, capability_id, scope_kind, scope_value)
    WHERE revoked_at IS NULL;

COMMIT;
