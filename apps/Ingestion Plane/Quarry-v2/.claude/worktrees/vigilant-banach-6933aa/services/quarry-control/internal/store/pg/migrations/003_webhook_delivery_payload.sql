-- 003_webhook_delivery_payload.sql
-- Add payload column for webhook delivery dispatcher (HMAC body source).
ALTER TABLE webhook_deliveries ADD COLUMN IF NOT EXISTS payload TEXT NOT NULL DEFAULT '';
