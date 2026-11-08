-- Booking, tracking, manifests, and the booking audit log (built WITH
-- booking, not retrofitted — docs/ARCHITECTURE.md security rules).

CREATE TABLE bookings (
    id                  uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    shipment_id         uuid REFERENCES shipments(id),
    -- Two-step server-side confirmation gate: a booking is created as
    -- pending_confirmation with a single-use token; only confirm-with-token
    -- transitions it to booked (and actually calls the carrier). No client —
    -- UI or agent — can skip the gate.
    status              text NOT NULL DEFAULT 'pending_confirmation',
    confirmation_token  text NOT NULL,
    confirmed_by        text,
    confirmed_at        timestamptz,
    -- Quote snapshot: the exact option the user saw and approved.
    quote_ref           text NOT NULL,
    carrier_code        text NOT NULL,
    carrier_name        text NOT NULL,
    service_name        text NOT NULL,
    price_amount_cents  bigint NOT NULL,
    price_currency      text NOT NULL,
    -- Shipment snapshot: the booking is fully self-describing (the carrier
    -- Book call at confirm time reads these, and audits stay meaningful
    -- independent of later model changes).
    from_address        jsonb NOT NULL,
    to_address          jsonb NOT NULL,
    package             jsonb NOT NULL,
    -- Carrier results (set on successful Book call).
    booking_ref         text,
    tracking_no         text,
    booked_at           timestamptz,
    -- Label bytes stored at booking time so re-downloads never depend on the
    -- carrier being up. ZPL (when offered) is stored alongside.
    label_content_type  text,
    label_data          bytea,
    label_zpl           text,
    -- Cross-border customs declaration (jsonb mirror of carrier.CustomsInfo).
    customs             jsonb,
    customs_doc_pdf     bytea,
    -- Pickup confirmation (set when a pickup is scheduled for this booking).
    pickup_ref          text,
    pickup_date         date,
    pickup_window       text,
    -- End-of-day manifest membership.
    manifest_id         uuid,
    booked_by           text NOT NULL,
    error_message       text,
    created_at          timestamptz NOT NULL DEFAULT now(),
    updated_at          timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX idx_bookings_status ON bookings(status);
CREATE INDEX idx_bookings_carrier ON bookings(carrier_code);
CREATE INDEX idx_bookings_tracking_no ON bookings(tracking_no);
CREATE INDEX idx_bookings_manifest ON bookings(manifest_id);

CREATE TABLE tracking_events (
    id           uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    booking_id   uuid NOT NULL REFERENCES bookings(id) ON DELETE CASCADE,
    status       text NOT NULL,
    description  text NOT NULL DEFAULT '',
    occurred_at  timestamptz NOT NULL,
    fetched_at   timestamptz NOT NULL DEFAULT now(),
    -- One row per distinct carrier event; refresh-on-read upserts on this key.
    UNIQUE (booking_id, status, occurred_at)
);

CREATE INDEX idx_tracking_events_booking ON tracking_events(booking_id);

CREATE TABLE manifests (
    id            uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    carrier_code  text NOT NULL,
    manifest_date date NOT NULL,
    booking_count integer NOT NULL DEFAULT 0,
    document_pdf  bytea,
    created_by    text NOT NULL,
    created_at    timestamptz NOT NULL DEFAULT now(),
    UNIQUE (carrier_code, manifest_date)
);

-- Append-only audit trail for every booking state change. `details` carries
-- the acting context (token match, carrier response refs, error text).
CREATE TABLE booking_audit_log (
    id          uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    booking_id  uuid NOT NULL REFERENCES bookings(id) ON DELETE CASCADE,
    action      text NOT NULL,
    actor       text NOT NULL,
    details     jsonb NOT NULL DEFAULT '{}',
    created_at  timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX idx_booking_audit_booking ON booking_audit_log(booking_id);
