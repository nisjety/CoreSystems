DROP INDEX IF EXISTS idx_bookings_reliability;
ALTER TABLE bookings DROP COLUMN IF EXISTS actual_delivered_at;
ALTER TABLE bookings DROP COLUMN IF EXISTS estimated_delivery;
