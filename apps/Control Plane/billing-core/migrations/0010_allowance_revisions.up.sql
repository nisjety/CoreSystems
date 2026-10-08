CREATE TABLE billing_allowance_revisions (
  org_id TEXT NOT NULL,
  metric TEXT NOT NULL,
  version TEXT NOT NULL,
  effective_at TIMESTAMPTZ NOT NULL,
  revision JSONB NOT NULL,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  PRIMARY KEY(org_id,metric,version),
  UNIQUE(org_id,metric,effective_at),
  FOREIGN KEY(org_id,metric) REFERENCES billing_allowance_policies(org_id,metric)
);
