DROP INDEX IF EXISTS capabilities_availability_state_idx;

ALTER TABLE capabilities
    DROP CONSTRAINT IF EXISTS capabilities_cost_class_check,
    DROP CONSTRAINT IF EXISTS capabilities_execution_mode_check,
    DROP CONSTRAINT IF EXISTS capabilities_availability_state_check,
    DROP COLUMN IF EXISTS health_checked_at,
    DROP COLUMN IF EXISTS cost_class,
    DROP COLUMN IF EXISTS execution_mode,
    DROP COLUMN IF EXISTS availability_reason,
    DROP COLUMN IF EXISTS availability_reason_code,
    DROP COLUMN IF EXISTS availability_state;
