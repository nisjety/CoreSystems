-- D-A billing-consolidation grant, mirrored from auth-core.
--
-- billing-core has its own database and cannot read auth-core's org_group /
-- org_group_grant tables, so the relationship arrives as an event
-- (organization.billing_group.changed) and is mirrored here. Same shape as the
-- existing plan mirror: auth-core is the authority, this is a local projection.
--
-- Scope, per the product decision: INHERITANCE ONLY. A member organization
-- inherits the host's plan/entitlements. No usage, invoice, or payment-provider
-- customer is consolidated -- billing_accounts.org_id remains the billing unit
-- and every organization keeps its own invoices and its own provider customer.
CREATE TABLE IF NOT EXISTS billing_group_memberships (
    org_id TEXT PRIMARY KEY,
    host_org_id TEXT NOT NULL,
    org_group_id TEXT NOT NULL,
    -- Mirrored so a revocation that arrives as `false` is recorded rather than
    -- inferred from a missing row; the consumer deletes on revoke, but an
    -- explicit column keeps a partial/legacy payload unambiguous.
    billing_consolidation BOOLEAN NOT NULL DEFAULT TRUE,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- An org inheriting from itself would be a no-op that silently looks active.
    CONSTRAINT billing_group_memberships_not_self CHECK (org_id <> host_org_id)
);

-- Resolving "who inherits from this host" for cache invalidation on the host's
-- own plan change.
CREATE INDEX IF NOT EXISTS idx_billing_group_memberships_host
    ON billing_group_memberships (host_org_id);
