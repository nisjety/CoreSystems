-- Reverses 0016_capability_tool_classes.up.sql.
--
-- Nothing reads the class columns or capability_tools yet, so dropping them
-- loses only the declarations this migration seeded. Once WS05-S3 and later
-- consume them, rolling back means turning TOOL_CLASS_GATE off first.

DROP TABLE IF EXISTS capability_tools;

ALTER TABLE capabilities
    DROP CONSTRAINT IF EXISTS capabilities_owner_effect_floor_check,
    DROP CONSTRAINT IF EXISTS capabilities_approval_floor_check,
    DROP CONSTRAINT IF EXISTS capabilities_side_effect_class_check,
    DROP COLUMN IF EXISTS approval_floor,
    DROP COLUMN IF EXISTS side_effect_class;
