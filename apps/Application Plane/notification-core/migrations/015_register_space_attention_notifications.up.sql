-- WS11-S9: out-of-app Space attention (refs-only). Convex is the in-app source
-- (its inbox), so in_app is disabled for every space.* type: enabling it would
-- duplicate the inbox. Email is on for the three signals a person must not miss
-- (mention, invite, approval needed) and off for the rest; per-user preferences
-- override these rows through PUT /api/v1/notifications/preferences. Requests
-- carry ids and closed words only, so no title or text is stored here. Delivery
-- stays off until NOTIFICATION_DELIVERY_MODE=novu and the matching Novu
-- workflows exist. Idempotent: ON CONFLICT DO NOTHING.
INSERT INTO notification_channel_configs
    (org_id, event_type, channel, enabled, default_for_subscribers, label, description)
VALUES
    ('_default', 'space.mention', 'in_app', FALSE, FALSE,
     'Space mention', 'Someone mentioned you in a Space.'),
    ('_default', 'space.mention', 'email',  TRUE,  TRUE,
     'Space mention', 'Email when someone mentions you in a Space.'),

    ('_default', 'space.reply', 'in_app', FALSE, FALSE,
     'Space reply', 'Someone replied to you in a Space.'),
    ('_default', 'space.reply', 'email',  FALSE, FALSE,
     'Space reply', 'Email when someone replies to you in a Space.'),

    ('_default', 'space.invite', 'in_app', FALSE, FALSE,
     'Space invite', 'You were added to a Space.'),
    ('_default', 'space.invite', 'email',  TRUE,  TRUE,
     'Space invite', 'Email when you are added to a Space.'),

    ('_default', 'space.agent_done', 'in_app', FALSE, FALSE,
     'Agent finished', 'An agent run you started in a Space finished.'),
    ('_default', 'space.agent_done', 'email',  FALSE, FALSE,
     'Agent finished', 'Email when an agent run you started in a Space finishes.'),

    ('_default', 'space.agent_failed', 'in_app', FALSE, FALSE,
     'Agent failed', 'An agent run you started in a Space failed.'),
    ('_default', 'space.agent_failed', 'email',  FALSE, FALSE,
     'Agent failed', 'Email when an agent run you started in a Space fails.'),

    ('_default', 'space.approval_needed', 'in_app', FALSE, FALSE,
     'Approval needed', 'An agent in a Space is waiting for your approval.'),
    ('_default', 'space.approval_needed', 'email',  TRUE,  TRUE,
     'Approval needed', 'Email when an agent in a Space is waiting for your approval.'),

    ('_default', 'space.agent_awaiting', 'in_app', FALSE, FALSE,
     'Agent waiting', 'An agent in a Space is waiting for you.'),
    ('_default', 'space.agent_awaiting', 'email',  FALSE, FALSE,
     'Agent waiting', 'Email when an agent in a Space is waiting for you.'),

    ('_default', 'space.reminder', 'in_app', FALSE, FALSE,
     'Space reminder', 'A reminder you set in a Space came due.'),
    ('_default', 'space.reminder', 'email',  FALSE, FALSE,
     'Space reminder', 'Email when a reminder you set in a Space comes due.')
ON CONFLICT (org_id, event_type, channel) DO NOTHING;
