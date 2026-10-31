-- Build plan WS03 S2: a granted approval raised by the durable Do loop is
-- consumed exactly once by that loop, and the cold delivery worker must not also
-- run it.
--
--   continuation_mode   '' = the cold delivery worker (every existing row, unchanged)
--                       'segment' | 'loop' = the loop resumes it; no delivery row is made
--   consumed_at         set once, by ConsumeApproval
--   consumed_claim_hash a hash of the claim token, so the same segment replaying is
--                       told apart from a second consumer
--
-- Additive: defaults keep every existing row and writer valid.
ALTER TABLE approvals
    ADD COLUMN IF NOT EXISTS continuation_mode TEXT NOT NULL DEFAULT '',
    ADD COLUMN IF NOT EXISTS consumed_at TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS consumed_claim_hash TEXT;
