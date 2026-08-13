-- The protected Space-binding writer uses its successful transactional audit
-- record as the durable idempotency receipt. Keep the replay lookup bounded as
-- the admin audit log grows; this does not make the audit log a workflow
-- coordinator, it only indexes this Data-owned configuration effect.
CREATE INDEX IF NOT EXISTS idx_admin_audit_log_space_binding_idempotency
    ON admin_audit_log (
        org_id,
        target_id,
        (payload->>'idempotency_key'),
        created_at DESC
    )
    WHERE action = 'space_retrieval_binding_upsert' AND outcome = 'ok';
