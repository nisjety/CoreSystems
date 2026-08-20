-- 0033: allow 'dispatch_unreachable' as a managed-run terminal failure code.
--
-- Gateway distinguishes two ways a prepared agent run can be closed without
-- ever running (see model-gateway's sse.rs):
--
--   * Execution Core answered with a deterministic refusal
--     (InvalidArgument/Unauthenticated/PermissionDenied) -> 'dispatch_rejected'.
--   * The request never left the gateway because the CONNECT phase failed
--     (ECONNREFUSED and friends) -> 'dispatch_unreachable'.
--
-- Both produce the same source ('gateway_agent_dispatch_rejected': the gateway's
-- dispatch is still the producer that failed) and the same outcome ('failed'),
-- so only the failure code distinguishes them. Until now both collapsed to
-- 'dispatch_rejected', because the gateway helper ignored its failure-code
-- argument and hardcoded that value -- which made a transient runner outage
-- indistinguishable, in the durable receipt, from a request the runner
-- deliberately refused. They need different operational responses (retry the
-- run vs fix the request), so they need different codes.
--
-- The column CHECK is an inline one from 0015, hence Postgres' generated name
-- <table>_<column>_check; dropped and re-added with the extra value, following
-- 0016's precedent for widening an inline CHECK. Additive only: every value
-- previously accepted is still accepted, so existing rows stay valid and this
-- needs no backfill. Older receipts keep saying 'dispatch_rejected' for both
-- causes -- the split is not retroactive.

ALTER TABLE managed_run_terminalization_outbox
    DROP CONSTRAINT IF EXISTS managed_run_terminalization_outbox_failure_code_check;

ALTER TABLE managed_run_terminalization_outbox
    ADD CONSTRAINT managed_run_terminalization_outbox_failure_code_check
    CHECK (failure_code IS NULL OR failure_code IN (
        'browser_failed',
        'dispatch_rejected',
        'dispatch_unreachable',
        'execution_failed',
        'inference_failed',
        'outcome_unknown',
        'provider_timeout',
        'provider_unavailable'
    ));
