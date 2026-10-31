-- WS16-S4: the attention notifications capability-core announces for a thread that waits on its owner.
-- In-app is on and push is on (inert until a web push subscription exists, S6). Email is off: these are
-- short prompts to return to a chat, not records the user must keep.
INSERT INTO notification_channel_configs
    (org_id, event_type, channel, enabled, default_for_subscribers, label, description)
VALUES
    ('_default', 'modelplane.needs_approval', 'in_app', TRUE,  TRUE,
     'Approval needed', 'A run in one of your chats is waiting for your approval.'),
    ('_default', 'modelplane.needs_approval', 'push',   TRUE,  TRUE,
     'Approval needed', 'Push when a run in one of your chats is waiting for your approval.'),
    ('_default', 'modelplane.needs_approval', 'email',  FALSE, FALSE,
     'Approval needed', 'Email when a run in one of your chats is waiting for your approval.'),

    ('_default', 'modelplane.needs_input',   'in_app', TRUE,  TRUE,
     'Input needed', 'A run in one of your chats has a question for you.'),
    ('_default', 'modelplane.needs_input',   'push',   TRUE,  TRUE,
     'Input needed', 'Push when a run in one of your chats has a question for you.'),
    ('_default', 'modelplane.needs_input',   'email',  FALSE, FALSE,
     'Input needed', 'Email when a run in one of your chats has a question for you.'),

    ('_default', 'modelplane.plan_ready',    'in_app', TRUE,  TRUE,
     'Plan ready', 'A plan in one of your chats is ready for your review.'),
    ('_default', 'modelplane.plan_ready',    'push',   TRUE,  TRUE,
     'Plan ready', 'Push when a plan in one of your chats is ready for your review.'),
    ('_default', 'modelplane.plan_ready',    'email',  FALSE, FALSE,
     'Plan ready', 'Email when a plan in one of your chats is ready for your review.')
ON CONFLICT (org_id, event_type, channel) DO NOTHING;
