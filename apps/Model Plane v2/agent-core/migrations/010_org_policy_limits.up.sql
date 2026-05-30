-- 010_org_policy_limits.up.sql
-- Per-organisation resource policy limits.

CREATE TABLE IF NOT EXISTS org_policy_limits (
    org_id                        TEXT PRIMARY KEY,
    max_concurrent_runs           INTEGER     NOT NULL DEFAULT 10,
    max_concurrent_runs_per_user  INTEGER     NOT NULL DEFAULT 5,
    max_tokens_per_run            INTEGER     NOT NULL DEFAULT 200000,
    max_cost_usd_per_run          NUMERIC(10,4) NOT NULL DEFAULT 5.0,
    max_cost_usd_monthly          NUMERIC(12,2) NOT NULL DEFAULT 500.0,
    max_actions_per_run           INTEGER     NOT NULL DEFAULT 200,
    allowed_tools                 JSONB       NOT NULL DEFAULT '[]',
    cost_limit_action             TEXT        NOT NULL DEFAULT 'block'
                                    CHECK (cost_limit_action IN ('block','warn','truncate')),
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

COMMENT ON TABLE org_policy_limits IS
    'Per-organisation resource and cost limits enforced by the policy service.';
