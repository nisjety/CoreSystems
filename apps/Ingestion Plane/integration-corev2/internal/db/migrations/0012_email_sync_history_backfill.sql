-- Durable user-requested history depth for inbox sources. The value stores
-- additional days beyond each fetcher's normal bootstrap window.
ALTER TABLE email_sync_state
    ADD COLUMN IF NOT EXISTS history_backfill_days INTEGER NOT NULL DEFAULT 0;

ALTER TABLE email_sync_state
    DROP CONSTRAINT IF EXISTS email_sync_state_history_backfill_days_check;

ALTER TABLE email_sync_state
    ADD CONSTRAINT email_sync_state_history_backfill_days_check
    CHECK (history_backfill_days >= 0 AND history_backfill_days <= 3650);
