CREATE TABLE IF NOT EXISTS billing_accounts (
    org_id TEXT PRIMARY KEY,
    plan TEXT NOT NULL DEFAULT 'free',
    subscription_state TEXT NOT NULL DEFAULT 'active',
    credits BIGINT NOT NULL DEFAULT 0,
    products JSONB NOT NULL DEFAULT '{}'::jsonb,
    feature_flags JSONB NOT NULL DEFAULT '{}'::jsonb,
    entitlements JSONB NOT NULL DEFAULT '{}'::jsonb,
    quota_limits JSONB NOT NULL DEFAULT '{}'::jsonb,
    provider_customer_id JSONB NOT NULL DEFAULT '{}'::jsonb,
    metadata JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS billing_usage_events (
    id BIGSERIAL PRIMARY KEY,
    org_id TEXT NOT NULL,
    metric TEXT NOT NULL,
    quantity DOUBLE PRECISION NOT NULL,
    source TEXT NOT NULL DEFAULT 'unknown',
    occurred_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    metadata JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_billing_usage_events_org_metric
    ON billing_usage_events (org_id, metric);

CREATE TABLE IF NOT EXISTS billing_invoices (
    invoice_id TEXT PRIMARY KEY,
    org_id TEXT NOT NULL,
    provider TEXT NOT NULL,
    amount_cents BIGINT NOT NULL,
    currency TEXT NOT NULL,
    status TEXT NOT NULL,
    issued_at TIMESTAMPTZ NOT NULL,
    due_at TIMESTAMPTZ NOT NULL,
    metadata JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_modified TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_billing_invoices_org_id
    ON billing_invoices (org_id);
