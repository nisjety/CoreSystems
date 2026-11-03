-- WS20-S7: the notice capability-core sends a scheduled run's owner when the run terminalizes.
-- In-app is on and push is on (inert until a web push subscription exists, WS16-S6). Email is off: the
-- notice points back to a chat and carries no run content.
INSERT INTO notification_channel_configs
    (org_id, event_type, channel, enabled, default_for_subscribers, label, description)
VALUES
    ('_default', 'modelplane.scheduled_run_outcome', 'in_app', TRUE,  TRUE,
     'Scheduled run outcome', 'A scheduled run in one of your chats finished, needs your review, failed or paused.'),
    ('_default', 'modelplane.scheduled_run_outcome', 'push',   TRUE,  TRUE,
     'Scheduled run outcome', 'Push when a scheduled run in one of your chats finishes, needs your review, fails or pauses.'),
    ('_default', 'modelplane.scheduled_run_outcome', 'email',  FALSE, FALSE,
     'Scheduled run outcome', 'Email when a scheduled run in one of your chats finishes, needs your review, fails or pauses.')
ON CONFLICT (org_id, event_type, channel) DO NOTHING;
