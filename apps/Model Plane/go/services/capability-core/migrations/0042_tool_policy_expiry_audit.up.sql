-- Expiry audit for a lapsed "Always" rule (build plan WS13 S7, K3-10).
--
-- The policy engine reads an allow past expires_at as an ask and says nothing, so the audit trail showed a
-- rule created and confirmed but never ending. A sweeper in capability-core now writes one content-free
-- tool_policy_rule_expired row per lapsed allow and stamps expiry_audited_at on the policy so it is
-- written once. Renewing the rule (a re-confirm) clears the stamp, so a rule that lapses again is audited
-- again. The sweeper runs only while TOOL_POLICY_RULE_AUDIT_ENABLED is "true"; with it off nothing reads
-- the column and no decision anywhere changes. Lapsed rows are never revoked or deleted here.
--
-- The column is nullable and every existing row keeps NULL. The partial index holds only the rows the
-- sweeper still has to visit, so it stays small.
--
-- RE-RUN SAFETY. capability-core re-applies every *.up.sql at every boot, so everything here is
-- idempotent: the column and the index are created only when missing.

ALTER TABLE tool_policies
    ADD COLUMN IF NOT EXISTS expiry_audited_at TIMESTAMPTZ;

CREATE INDEX IF NOT EXISTS tool_policies_expiry_due_idx
    ON tool_policies (expires_at)
    WHERE decision = 'allow'
      AND revoked_at IS NULL
      AND expires_at IS NOT NULL
      AND expiry_audited_at IS NULL;
