-- U5-2: org-level channel configuration.
--
-- Each row is an org-admin-controlled "is this (event_type, channel) combo
-- enabled for the org, and is it the default for new subscribers?". The
-- preference rows in 004 are the per-user overrides on top of these
-- defaults.
--
-- The seed below covers the events verevon-core publishes today (see
-- verevon-gap.md §8.30 G44 and §11). Add a new row whenever a new workflow
-- ships in notification-core or in an upstream service.
CREATE TABLE IF NOT EXISTS notification_channel_configs (
    org_id                  TEXT        NOT NULL,
    event_type              TEXT        NOT NULL,
    channel                 TEXT        NOT NULL,
    enabled                 BOOLEAN     NOT NULL DEFAULT TRUE,
    default_for_subscribers BOOLEAN     NOT NULL DEFAULT TRUE,
    -- Human-readable label for the /profile/notifications UI. Falls back
    -- to a constant in the handler if empty.
    label                   TEXT        NOT NULL DEFAULT '',
    description             TEXT        NOT NULL DEFAULT '',
    created_at              TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at              TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (org_id, event_type, channel)
);

CREATE INDEX IF NOT EXISTS notification_channel_configs_org_idx
    ON notification_channel_configs (org_id);

-- Seed a "_default" org-wide policy that every org inherits if it has no
-- explicit row. The Channel service reads this when the requested
-- (org_id, event_type, channel) combo doesn't exist.
INSERT INTO notification_channel_configs
    (org_id, event_type, channel, enabled, default_for_subscribers, label, description)
VALUES
    ('_default', 'control_session.entitlements_changed', 'in_app', TRUE, TRUE,
     'Plan & entitlement updates', 'Real-time notice when your plan or feature access changes.'),
    ('_default', 'control_session.entitlements_changed', 'email',  FALSE, FALSE,
     'Plan & entitlement updates', 'Email summary of plan and entitlement changes.'),

    ('_default', 'auth.user.invited',                   'in_app', TRUE, TRUE,
     'Team invitations',           'When someone invites you to a workspace.'),
    ('_default', 'auth.user.invited',                   'email',  TRUE, TRUE,
     'Team invitations',           'Email link to accept a workspace invitation.'),

    ('_default', 'auth.user.security_alert',            'in_app', TRUE, TRUE,
     'Security alerts',            'New sign-in from an unrecognised device or IP.'),
    ('_default', 'auth.user.security_alert',            'email',  TRUE, TRUE,
     'Security alerts',            'Email alert for sign-ins from new devices.'),

    ('_default', 'billing.invoice_finalized',           'in_app', TRUE, TRUE,
     'Billing updates',            'New invoice, failed payment, or renewal notice.'),
    ('_default', 'billing.invoice_finalized',           'email',  TRUE, TRUE,
     'Billing updates',            'Email receipts and billing summaries.'),

    ('_default', 'org.member.added',                    'in_app', TRUE, TRUE,
     'Team activity',              'When a new member joins your workspace.'),
    ('_default', 'org.member.added',                    'email',  FALSE, FALSE,
     'Team activity',              'Daily email digest of team activity.'),

    ('_default', 'mention.received',                    'in_app', TRUE, TRUE,
     'Mentions',                   '@mentions in chat, comments, or shared documents.'),
    ('_default', 'mention.received',                    'email',  FALSE, FALSE,
     'Mentions',                   'Email summary of mentions.'),

    ('_default', 'product.update',                      'in_app', TRUE, FALSE,
     'Product updates',            'Release notes and new feature announcements.'),
    ('_default', 'product.update',                      'email',  FALSE, FALSE,
     'Product updates',            'Monthly product newsletter.')
ON CONFLICT (org_id, event_type, channel) DO NOTHING;
