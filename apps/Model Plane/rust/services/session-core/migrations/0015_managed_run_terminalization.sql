-- Durable managed-run terminalization obligation/outbox.
--
-- A managed run is born with exactly one row in this table, in the same
-- transaction as runs + RUN_STARTED. Producers submit only a fixed outcome
-- classification; Session Core writes the terminal event and receipt
-- atomically. A lease-backed recovery worker may resolve an abandoned row as
-- an observable unknown-outcome failure, never as success.
--
-- Zero Data Retention posture: this table intentionally has no prompt,
-- response, tool argument/result, provider error, URL, arbitrary metadata, or
-- content-derived hash column. It stores only opaque identifiers, server-owned
-- source/step mappings, fixed classification enums, and recovery bookkeeping.

CREATE TABLE IF NOT EXISTS managed_run_terminalization_outbox (
    run_id TEXT PRIMARY KEY
        REFERENCES runs(id) ON DELETE CASCADE
        CHECK (length(run_id) BETWEEN 1 AND 128),
    org_id TEXT NOT NULL
        CHECK (length(org_id) BETWEEN 1 AND 128),
    user_id TEXT NOT NULL
        CHECK (length(user_id) BETWEEN 1 AND 128),
    -- Opaque caller retry identity. It is not a content digest and is scoped
    -- to the durable authenticated owner.
    start_key TEXT NOT NULL
        CHECK (start_key ~ '^[A-Za-z0-9][A-Za-z0-9_-]{0,127}$'),
    configured_source TEXT NOT NULL
        CHECK (configured_source IN (
            'gateway_direct',
            'execution_agent',
            'execution_browser',
            'gateway_browser'
        )),
    configured_terminal_step_id TEXT NOT NULL
        CHECK (configured_terminal_step_id IN (
            'model-gateway-direct-inference-final',
            'execution-core-agent-final',
            'execution-core-browser-final',
            'model-gateway-browser-agent-final'
        )),
    zdr BOOLEAN NOT NULL,
    state TEXT NOT NULL DEFAULT 'open'
        CHECK (state IN (
            'open',
            'processing',
            'applied',
            'reconciliation_required',
            'superseded'
        )),
    outcome TEXT
        CHECK (outcome IS NULL OR outcome IN ('completed', 'failed', 'unknown')),
    failure_code TEXT
        CHECK (failure_code IS NULL OR failure_code IN (
            'browser_failed',
            'dispatch_rejected',
            'execution_failed',
            'inference_failed',
            'outcome_unknown',
            'provider_timeout',
            'provider_unavailable'
        )),
    applied_source TEXT
        CHECK (applied_source IS NULL OR applied_source IN (
            'gateway_direct',
            'execution_agent',
            'execution_browser',
            'gateway_agent_dispatch_rejected',
            'gateway_browser'
        )),
    applied_terminal_step_id TEXT
        CHECK (applied_terminal_step_id IS NULL OR length(applied_terminal_step_id) BETWEEN 1 AND 128),
    step_event_id TEXT
        CHECK (step_event_id IS NULL OR length(step_event_id) BETWEEN 1 AND 128),
    receipt_id TEXT
        CHECK (receipt_id IS NULL OR length(receipt_id) BETWEEN 1 AND 128),
    step_index BIGINT
        CHECK (step_index IS NULL OR step_index >= 1),
    applied_at TIMESTAMPTZ,
    -- The deadline is only renewed by Session Core's fixed server policy.
    deadline_at TIMESTAMPTZ NOT NULL DEFAULT (now() + interval '15 minutes'),
    attempts INTEGER NOT NULL DEFAULT 0
        CHECK (attempts >= 0),
    next_attempt_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    lease_owner TEXT
        CHECK (lease_owner IS NULL OR length(lease_owner) BETWEEN 1 AND 256),
    lease_token_hash TEXT
        CHECK (lease_token_hash IS NULL OR length(lease_token_hash) = 64),
    lease_expires_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (org_id, user_id, start_key),
    CHECK (
        (lease_owner IS NULL AND lease_token_hash IS NULL AND lease_expires_at IS NULL)
        OR
        (lease_owner IS NOT NULL AND lease_token_hash IS NOT NULL AND lease_expires_at IS NOT NULL)
    ),
    CHECK (
        (configured_source = 'gateway_direct'
            AND configured_terminal_step_id = 'model-gateway-direct-inference-final')
        OR (configured_source = 'execution_agent'
            AND configured_terminal_step_id = 'execution-core-agent-final')
        OR (configured_source = 'execution_browser'
            AND configured_terminal_step_id = 'execution-core-browser-final')
        OR (configured_source = 'gateway_browser'
            AND configured_terminal_step_id = 'model-gateway-browser-agent-final')
    ),
    CHECK (
        (state IN ('open', 'processing', 'superseded')
            AND outcome IS NULL
            AND failure_code IS NULL
            AND applied_source IS NULL
            AND applied_terminal_step_id IS NULL
            AND step_event_id IS NULL
            AND receipt_id IS NULL
            AND step_index IS NULL
            AND applied_at IS NULL)
        OR (state = 'applied'
            AND outcome IN ('completed', 'failed')
            AND applied_source IS NOT NULL
            AND applied_terminal_step_id IS NOT NULL
            AND step_event_id IS NOT NULL
            AND receipt_id IS NOT NULL
            AND step_index IS NOT NULL
            AND applied_at IS NOT NULL
            AND ((outcome = 'completed' AND failure_code IS NULL)
                OR (outcome = 'failed' AND failure_code IS NOT NULL)))
        OR (state = 'reconciliation_required'
            AND outcome = 'unknown'
            AND failure_code = 'outcome_unknown'
            AND applied_source IS NOT NULL
            AND applied_terminal_step_id IS NOT NULL
            AND step_event_id IS NOT NULL
            AND receipt_id IS NOT NULL
            AND step_index IS NOT NULL
            AND applied_at IS NOT NULL)
    )
);

CREATE INDEX IF NOT EXISTS managed_run_terminalization_due_idx
    ON managed_run_terminalization_outbox (deadline_at, next_attempt_at, created_at)
    WHERE state = 'open';

CREATE INDEX IF NOT EXISTS managed_run_terminalization_lease_idx
    ON managed_run_terminalization_outbox (lease_expires_at, created_at)
    WHERE state = 'processing';
