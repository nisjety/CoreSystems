-- PR-5 (E): register the `daily_brief` workflow.
--
-- insight-core's daily-brief scheduler POSTs a notification request with
-- type `daily_brief` (the Novu WorkflowID the adapter triggers). This seeds the
-- org-wide `_default` channel policy so the brief is delivered on in_app + email
-- — the same (event_type, channel) registry every other workflow uses (see 005).
-- The Novu workflow TEMPLATE itself lives in the Novu dashboard; this registers
-- the channel routing only (do NOT build Novu).
INSERT INTO notification_channel_configs
    (org_id, event_type, channel, enabled, default_for_subscribers, label, description)
VALUES
    ('_default', 'daily_brief', 'in_app', TRUE, TRUE,
     'Daily brief', 'A daily summary of recorded activity across your surfaces.'),
    ('_default', 'daily_brief', 'email',  TRUE, TRUE,
     'Daily brief', 'Email digest of your daily activity summary.')
ON CONFLICT (org_id, event_type, channel) DO NOTHING;
