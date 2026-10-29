-- 0069: allow the classified browser-agent failure codes as managed-run
-- terminal failure codes.
--
-- Gateway used to collapse every failed standalone browser-agent run to
-- 'browser_failed' (model-gateway's session_flow.rs), although Execution Core
-- had already classified the step: a capability-policy denial, a timeout and
-- an exhausted resource each need a different operational response (fix the
-- policy or attestation, retry, raise the budget), and SC1 leg C showed a
-- permission denial reading as a generic browser failure. Gateway now keeps
-- the class:
--
--   * permission_denied   -> 'browser_permission_denied'
--   * timed_out           -> 'browser_timed_out'
--   * resource_exhausted  -> 'browser_resource_exhausted'
--   * anything else       -> 'browser_failed' (unchanged)
--
-- The codes carry no URL, content or error text. They must match
-- terminalization::ALLOWED_FAILURE_CODES in session-core; a code missing from
-- the allowlist is rejected before any database access, and Gateway then
-- retries once with 'browser_failed', so the deploy order is not critical.
--
-- The column CHECK is the inline one from 0015 (generated name
-- <table>_<column>_check), last widened by 0033; dropped and re-added with the
-- extra values, following 0016's and 0033's precedent. Additive only: every
-- value previously accepted is still accepted, so existing rows stay valid and
-- this needs no backfill. Older receipts keep saying 'browser_failed' -- the
-- split is not retroactive.

ALTER TABLE managed_run_terminalization_outbox
    DROP CONSTRAINT IF EXISTS managed_run_terminalization_outbox_failure_code_check;

ALTER TABLE managed_run_terminalization_outbox
    ADD CONSTRAINT managed_run_terminalization_outbox_failure_code_check
    CHECK (failure_code IS NULL OR failure_code IN (
        'browser_failed',
        'browser_permission_denied',
        'browser_resource_exhausted',
        'browser_timed_out',
        'dispatch_rejected',
        'dispatch_unreachable',
        'execution_failed',
        'inference_failed',
        'outcome_unknown',
        'provider_timeout',
        'provider_unavailable'
    ));
