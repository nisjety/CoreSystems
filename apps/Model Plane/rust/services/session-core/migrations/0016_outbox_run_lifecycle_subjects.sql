-- Allow run-lifecycle projections in the audit outbox.
--
-- session_audit_outbox was created for audit events only, so its subject CHECK
-- accepted just `velion.audit.v2.model.session-core.%`. When RUN_COMPLETED
-- learning events started being enqueued through the same outbox (so they commit
-- in the SAME transaction as the terminal event, which is the entire point of an
-- outbox), every insert was rejected:
--
--   new row for relation "session_audit_outbox" violates check constraint
--   "session_audit_outbox_subject_check"
--
-- The insert is wrapped in a SAVEPOINT and its error is deliberately swallowed,
-- so terminalization still succeeded and the run stayed terminal -- the only
-- casualty was the learning event, reported as a WARN plus
-- mp_session_learning_run_events_dropped_total. That is why this was invisible
-- until the counter was checked: nothing user-facing ever broke.
--
-- The new predicate stays deliberately narrow rather than dropping the
-- constraint. `mp.v1.run.%.event` mirrors the NATS publish grant added for
-- session-core-runtime, and the drainer applies its own per-route authority
-- check (audit_publisher.rs::check_outbox_authority) requiring the subject's run
-- id to equal the payload's resource_ref -- so one run cannot publish under
-- another's subject. Keeping the constraint tight means a typo'd subject still
-- fails loudly here instead of reaching the bus.

ALTER TABLE session_audit_outbox
    DROP CONSTRAINT IF EXISTS session_audit_outbox_subject_check;

ALTER TABLE session_audit_outbox
    ADD CONSTRAINT session_audit_outbox_subject_check
    CHECK (
        subject LIKE 'velion.audit.v2.model.session-core.%'
        OR subject LIKE 'mp.v1.run.%.event'
    );
