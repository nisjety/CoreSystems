-- F8: reliability history/score. A booking becomes scoreable once both an
-- estimated_delivery (set at booking time, from the quote the user saw) and
-- an actual_delivered_at (set once tracking observes delivery) are present.
-- Bookings created before this migration — or via a caller that omits
-- estimated_delivery — simply never enter the scored population; there is
-- no backfill, since we have no honest estimated_delivery for them.

ALTER TABLE bookings ADD COLUMN estimated_delivery date;
ALTER TABLE bookings ADD COLUMN actual_delivered_at timestamptz;

CREATE INDEX idx_bookings_reliability
    ON bookings (carrier_code, booked_at)
    WHERE estimated_delivery IS NOT NULL;
