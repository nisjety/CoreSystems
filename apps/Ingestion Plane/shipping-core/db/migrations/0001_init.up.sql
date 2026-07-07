CREATE EXTENSION IF NOT EXISTS pgcrypto;

CREATE TABLE users (
    id            uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    entra_subject text NOT NULL UNIQUE,
    email         text NOT NULL,
    role          text NOT NULL DEFAULT 'staff',
    created_at    timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE shipments (
    id                 uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    created_by         uuid REFERENCES users(id),
    status             text NOT NULL DEFAULT 'draft',
    from_address       jsonb NOT NULL,
    to_address         jsonb NOT NULL,
    package_weight_kg  numeric(10, 3) NOT NULL,
    package_length_cm  numeric(10, 2) NOT NULL,
    package_width_cm   numeric(10, 2) NOT NULL,
    package_height_cm  numeric(10, 2) NOT NULL,
    segment            text NOT NULL,
    visma_order_ref    text,
    created_at         timestamptz NOT NULL DEFAULT now(),
    updated_at         timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE quotes (
    id                   uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    shipment_id          uuid NOT NULL REFERENCES shipments(id) ON DELETE CASCADE,
    carrier_code         text NOT NULL,
    carrier_name         text NOT NULL,
    service_name         text NOT NULL,
    price_amount_cents   bigint NOT NULL,
    price_currency       text NOT NULL,
    estimated_delivery   date NOT NULL,
    transit_days         integer NOT NULL,
    features             jsonb NOT NULL DEFAULT '[]',
    reliability_score    numeric(4, 3),
    created_at           timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX idx_quotes_shipment_id ON quotes(shipment_id);
CREATE INDEX idx_shipments_created_by ON shipments(created_by);
