ALTER TABLE capabilities
    ADD COLUMN IF NOT EXISTS availability_state TEXT NOT NULL DEFAULT 'unavailable',
    ADD COLUMN IF NOT EXISTS availability_reason_code TEXT NOT NULL DEFAULT 'health_not_attested',
    ADD COLUMN IF NOT EXISTS availability_reason TEXT NOT NULL DEFAULT '',
    ADD COLUMN IF NOT EXISTS execution_mode TEXT NOT NULL DEFAULT 'unavailable',
    ADD COLUMN IF NOT EXISTS cost_class TEXT NOT NULL DEFAULT 'unknown',
    ADD COLUMN IF NOT EXISTS health_checked_at TIMESTAMPTZ;

ALTER TABLE capabilities
    DROP CONSTRAINT IF EXISTS capabilities_availability_state_check,
    ADD CONSTRAINT capabilities_availability_state_check CHECK (
        availability_state IN (
            'available',
            'disabled',
            'unhealthy',
            'approval_required',
            'unavailable',
            'not_configured'
        )
    ),
    DROP CONSTRAINT IF EXISTS capabilities_execution_mode_check,
    ADD CONSTRAINT capabilities_execution_mode_check CHECK (
        execution_mode IN ('direct_read', 'agentic', 'unavailable')
    ),
    DROP CONSTRAINT IF EXISTS capabilities_cost_class_check,
    ADD CONSTRAINT capabilities_cost_class_check CHECK (
        cost_class IN ('bounded', 'variable', 'unknown')
    );

CREATE INDEX IF NOT EXISTS capabilities_availability_state_idx
    ON capabilities (availability_state)
    WHERE deleted_at IS NULL;

-- Existing enabled rows are registry declarations, not runtime health proof.
-- Keep them explicitly unavailable until an authenticated workload attests a
-- health state after the source rollout.
UPDATE capabilities
SET availability_state = 'unavailable',
    availability_reason_code = 'health_not_attested',
    availability_reason = 'Capability runtime health has not been attested.',
    execution_mode = 'unavailable',
    cost_class = 'unknown',
    health_checked_at = NULL
WHERE health_checked_at IS NULL;
