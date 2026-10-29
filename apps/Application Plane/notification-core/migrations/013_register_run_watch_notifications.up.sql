-- AUTO-2: capability-core (Model Plane) notifies a user when a run they are
-- watching finishes. In-app is on by default; email is opt-in since these
-- are transient run-status pings, not durable records the user must see.
INSERT INTO notification_channel_configs
    (org_id, event_type, channel, enabled, default_for_subscribers, label, description)
VALUES
    ('_default', 'modelplane.run_completed', 'in_app', TRUE, TRUE,
     'Run completed', 'A run you asked to be notified about finished successfully.'),
    ('_default', 'modelplane.run_completed', 'email',  FALSE, FALSE,
     'Run completed', 'Email when a watched run finishes successfully.'),

    ('_default', 'modelplane.run_failed',    'in_app', TRUE, TRUE,
     'Run failed', 'A run you asked to be notified about failed.'),
    ('_default', 'modelplane.run_failed',    'email',  FALSE, FALSE,
     'Run failed', 'Email when a watched run fails.')
ON CONFLICT (org_id, event_type, channel) DO NOTHING;
