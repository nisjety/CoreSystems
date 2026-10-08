-- Azure-billed ActualCost rows are separate from per-request estimates. They
-- are attributable at a Foundry project/org level, not to a Verevon user.
CREATE TABLE IF NOT EXISTS azure_actual_costs (
    azure_scope text NOT NULL,
    project_tag text NOT NULL,
    meter text NOT NULL,
    usage_date date NOT NULL,
    amount numeric(20, 10) NOT NULL,
    currency char(3) NOT NULL,
    org_id text NOT NULL DEFAULT '',
    updated_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (azure_scope, project_tag, meter, usage_date, currency)
);

CREATE INDEX IF NOT EXISTS azure_actual_costs_org_date_idx
    ON azure_actual_costs (org_id, usage_date)
    WHERE org_id <> '';
