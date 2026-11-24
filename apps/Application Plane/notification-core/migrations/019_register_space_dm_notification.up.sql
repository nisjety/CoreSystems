-- D22r (DEC-3D-DM-ATTN): every DM message notifies every other member of the DM
-- by default; each member can silence one DM with its room level (Convex,
-- spaceRoomNotifyPrefs) or this type's email preference. Convex keeps the in-app
-- copy (a reply item in its inbox), so in_app stays off here, as for the other
-- space.* types (migration 015). Email is on by default. The request is
-- refs-only: ids and closed words, no title, name or message text. Delivery
-- stays off until NOTIFICATION_DELIVERY_MODE=novu and a Novu workflow with the
-- id space.dm exists. Idempotent: ON CONFLICT DO NOTHING.
INSERT INTO notification_channel_configs
    (org_id, event_type, channel, enabled, default_for_subscribers, label, description)
VALUES
    ('_default', 'space.dm', 'in_app', FALSE, FALSE,
     'Direct message', 'Someone sent you a direct message in a Space.'),
    ('_default', 'space.dm', 'email',  TRUE,  TRUE,
     'Direct message', 'Email when someone sends you a direct message in a Space.')
ON CONFLICT (org_id, event_type, channel) DO NOTHING;
