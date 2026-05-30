-- Revert hook type expansion to the original hook set.

ALTER TABLE hook_configs
DROP CONSTRAINT IF EXISTS hook_configs_hook_type_check;

ALTER TABLE hook_configs
ADD CONSTRAINT hook_configs_hook_type_check
CHECK (
    hook_type IN (
        'pre_tool_use',
        'post_tool_use',
        'stop'
    )
);