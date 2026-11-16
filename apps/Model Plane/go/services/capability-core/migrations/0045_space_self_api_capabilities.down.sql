-- Reverses 0045_space_self_api_capabilities.up.sql. It removes the five Space run-action
-- capability rows and their five admission rows, and nothing else. A capability an operator
-- has since adopted under another creator is kept.

DELETE FROM agent_action_admissions
WHERE action_id IN (
    'space.thread.rename',
    'space.thread.pin',
    'space.thread.post_card',
    'space.agent.pause_self',
    'space.schedule.follow_up'
);

DELETE FROM capability_scopes
WHERE capability_id IN (
    'cap.tool.space.thread.rename',
    'cap.tool.space.thread.pin',
    'cap.tool.space.thread.post_card',
    'cap.tool.space.agent.pause_self',
    'cap.tool.space.schedule.follow_up'
);
DELETE FROM capability_health
WHERE capability_id IN (
    'cap.tool.space.thread.rename',
    'cap.tool.space.thread.pin',
    'cap.tool.space.thread.post_card',
    'cap.tool.space.agent.pause_self',
    'cap.tool.space.schedule.follow_up'
);
DELETE FROM capability_versions
WHERE capability_id IN (
    'cap.tool.space.thread.rename',
    'cap.tool.space.thread.pin',
    'cap.tool.space.thread.post_card',
    'cap.tool.space.agent.pause_self',
    'cap.tool.space.schedule.follow_up'
);
DELETE FROM skill_packages
WHERE capability_id IN (
    'cap.tool.space.thread.rename',
    'cap.tool.space.thread.pin',
    'cap.tool.space.thread.post_card',
    'cap.tool.space.agent.pause_self',
    'cap.tool.space.schedule.follow_up'
);

DELETE FROM capabilities
WHERE id IN (
    'cap.tool.space.thread.rename',
    'cap.tool.space.thread.pin',
    'cap.tool.space.thread.post_card',
    'cap.tool.space.agent.pause_self',
    'cap.tool.space.schedule.follow_up'
)
  AND created_by = 'migration:0045_space_self_api_capabilities';
