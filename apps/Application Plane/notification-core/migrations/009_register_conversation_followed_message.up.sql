-- Inbox follow notifications are intentionally in-app first. The generic
-- notice has no customer text, sender identity, provider identifiers, or
-- message body; opening Inbox remains subject to its normal authorization.
INSERT INTO notification_channel_configs
    (org_id, event_type, channel, enabled, default_for_subscribers, label, description)
VALUES
    ('_default', 'inbox.conversation_followed_message', 'in_app', TRUE, TRUE,
     'Followed conversations', 'A customer reply arrives in a conversation you follow.'),
    ('_default', 'inbox.conversation_followed_message', 'email',  FALSE, FALSE,
     'Followed conversations', 'Email summary of updates in conversations you follow.')
ON CONFLICT (org_id, event_type, channel) DO NOTHING;
