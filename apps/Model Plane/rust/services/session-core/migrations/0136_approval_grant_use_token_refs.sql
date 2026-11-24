-- Single-use standing-rule proofs (build plan WS10-S3, C-PRIO-06).
--
-- Control's approval-grant-use-v1 proof lives two minutes and names a run, not a step. Without a
-- record of the proofs already spent, one proof could pass several steps of the same run inside
-- that window. Each approval a standing rule passed now stores the proof's opaque `token_ref`
-- (a Control random id, content-free), and a unique index makes a second use of the same proof
-- fail. An idempotent retry of the SAME step still collapses onto its row through the
-- (org_id, user_id, idempotency_key) arbiter, which Postgres checks first.
--
-- Additive: a nullable column and a partial unique index. Rows written before this migration,
-- and every approval no rule passed, carry NULL and are not indexed.
ALTER TABLE approvals
    ADD COLUMN IF NOT EXISTS grant_use_token_ref TEXT;

CREATE UNIQUE INDEX IF NOT EXISTS approvals_grant_use_token_ref_key
    ON approvals (grant_use_token_ref)
    WHERE grant_use_token_ref IS NOT NULL;
