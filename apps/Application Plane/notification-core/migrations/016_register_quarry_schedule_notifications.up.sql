-- WS15.S4 (D17): notices for quarry schedules. quarry-control tells a schedule's
-- creator when a scheduled run is skipped (skipped_zdr also pauses the schedule)
-- or fails. The types stay inert until quarry-control's schedule notifier sends
-- them. In-app is on by default and email stays off. Nothing here stores content:
-- a ZDR notice carries a title only, and a standard notice carries ids and a
-- closed reason, never a URL (see quarryNoticeRefusal in internal/http/handlers.go).
-- Idempotent: ON CONFLICT DO NOTHING.
INSERT INTO notification_channel_configs
    (org_id, event_type, channel, enabled, default_for_subscribers, label, description)
VALUES
    ('_default', 'quarry.schedule_run_skipped', 'in_app', TRUE, TRUE,
     'Schedule run skipped', 'A scheduled Innhenting run was skipped, for example because ZDR is on.'),
    ('_default', 'quarry.schedule_run_skipped', 'email',  FALSE, FALSE,
     'Schedule run skipped', 'Email when a scheduled Innhenting run is skipped.'),

    ('_default', 'quarry.schedule_run_failed', 'in_app', TRUE, TRUE,
     'Schedule run failed', 'A scheduled Innhenting run failed.'),
    ('_default', 'quarry.schedule_run_failed', 'email',  FALSE, FALSE,
     'Schedule run failed', 'Email when a scheduled Innhenting run fails.')
ON CONFLICT (org_id, event_type, channel) DO NOTHING;
