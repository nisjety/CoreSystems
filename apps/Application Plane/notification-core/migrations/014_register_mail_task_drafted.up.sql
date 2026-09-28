-- Mail-task spec §3.5 (chat-wave2, owner ruling 2026-09-28: Postmark inbound,
-- self-forward only). A single recipient (the alias owner), never a follower
-- fan-out. The event carries no email content -- see
-- mail_task_drafted.go's mailTaskDraftedEvent doc comment.
INSERT INTO notification_channel_configs
    (org_id, event_type, channel, enabled, default_for_subscribers, label, description)
VALUES
    ('_default', 'chat.mail_task_drafted', 'in_app', TRUE, TRUE,
     'Email task drafted', 'An email you forwarded to your task address produced a drafted answer.'),
    ('_default', 'chat.mail_task_drafted', 'email',  FALSE, FALSE,
     'Email task drafted', 'Email when a forwarded task produces a drafted answer.')
ON CONFLICT (org_id, event_type, channel) DO NOTHING;
