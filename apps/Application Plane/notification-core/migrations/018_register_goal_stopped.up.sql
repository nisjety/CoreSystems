-- EP-7 S5: the notice capability-core sends a thread goal's owner when the goal stops (met, budget
-- reached, turn cap reached or judged blocked). In-app is on and push is on (inert until a web push
-- subscription exists, WS16-S6). Email is off. The request carries fixed copy, the thread deep link and
-- opaque ids only: never the objective, the criteria or any run content.
INSERT INTO notification_channel_configs
    (org_id, event_type, channel, enabled, default_for_subscribers, label, description)
VALUES
    ('_default', 'modelplane.goal_stopped', 'in_app', TRUE,  TRUE,
     'Goal stopped', 'A goal in one of your chats stopped and may need you.'),
    ('_default', 'modelplane.goal_stopped', 'push',   TRUE,  TRUE,
     'Goal stopped', 'Push when a goal in one of your chats stops and may need you.'),
    ('_default', 'modelplane.goal_stopped', 'email',  FALSE, FALSE,
     'Goal stopped', 'Email when a goal in one of your chats stops and may need you.')
ON CONFLICT (org_id, event_type, channel) DO NOTHING;
