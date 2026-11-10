-- 0115: allow 'browser_help_timed_out' as a managed-run terminal failure code (build plan EP-1 S5).
--
-- A takeover pause is a `browser_human_assist` approval: the person holds the run's browser, or the agent asked
-- them for help in it. When that approval times out (the person never gave the browser back) and the approval-expiry
-- switch SESSION_CORE_RECOVERY_APPROVAL_EXPIRY_ENABLED is on, the recovery worker ends the run as
-- browser_help_timed_out, a known cause, instead of as outcome_unknown. Only recovery writes the code, the same way
-- it writes 'question_expired' (migration 0088): a caller may not submit it, so session-core keeps it out of its
-- caller allowlist, though the column accepts it.
--
-- contract: the failure_code CHECK is dropped and re-added with one more value. Every value accepted before is still
-- accepted, so existing rows stay valid and no row is rewritten. The reversal is in
-- revert/0115_browser_help_timed_out_failure_code.down.sql.
--
-- Zero Data Retention: nothing new is stored. The code is a closed token on a terminal row that already names only ids.

ALTER TABLE managed_run_terminalization_outbox
    DROP CONSTRAINT IF EXISTS managed_run_terminalization_outbox_failure_code_check;

ALTER TABLE managed_run_terminalization_outbox
    ADD CONSTRAINT managed_run_terminalization_outbox_failure_code_check
    CHECK (failure_code IS NULL OR failure_code IN (
        'browser_failed',
        'browser_help_timed_out',
        'browser_permission_denied',
        'browser_resource_exhausted',
        'browser_timed_out',
        'dispatch_rejected',
        'dispatch_unreachable',
        'execution_failed',
        'inference_failed',
        'outcome_unknown',
        'provider_timeout',
        'provider_unavailable',
        'question_expired'
    ));
