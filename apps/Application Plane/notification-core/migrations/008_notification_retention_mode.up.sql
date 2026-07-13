-- Explicit content-retention posture for notification requests.
--
-- `zdr` rows retain only the control ledger (scope, type, fingerprint and
-- delivery status). Application code never stores their payload or creates a
-- feed projection. Existing rows are classified as standard because their
-- payloads were already persisted before this contract existed.

ALTER TABLE notification_requests
    ADD COLUMN IF NOT EXISTS retention_mode TEXT NOT NULL DEFAULT 'standard';

ALTER TABLE notification_requests
    ADD CONSTRAINT notification_requests_retention_mode_check
        CHECK (retention_mode IN ('standard', 'zdr'));
