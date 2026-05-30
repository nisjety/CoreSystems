-- Expand hook types to cover compact, permission, session, and subagent events.

ALTER TABLE hook_configs
DROP CONSTRAINT IF EXISTS hook_configs_hook_type_check;

ALTER TABLE hook_configs
ADD CONSTRAINT hook_configs_hook_type_check
CHECK (
    hook_type IN (
        'pre_tool_use',
        'post_tool_use',
        'stop',
        'pre_compact',
        'post_compact',
        'permission_check',
        'session_start',
        'session_end',
        'subagent_start',
        'subagent_stop'
    )
);