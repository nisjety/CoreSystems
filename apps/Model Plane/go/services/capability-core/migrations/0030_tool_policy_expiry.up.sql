-- Expiry for a confirmed "Always" rule (build plan WS13 S7, K3-10).
--
-- A rule an "Always" choice proposed and the person confirmed is an `allow` that lapses: ninety days
-- by default, at most a year (tool_policies.go). When it lapses the policy engine reads the allow as
-- an ask (policy.DecideWithPolicies), exactly as it reads a stale digest, and the person renews the
-- rule by confirming it again. The constraint keeps every lapse on an allow: an ask or a never does
-- not lapse, so a tightening policy can never stop applying by the clock.
--
-- The column is nullable and every existing row keeps NULL, which never lapses, so applying this
-- changes no decision anywhere. Nothing reads it unless TOOL_POLICY_ENFORCED is "true" on
-- capability-core, as with the rest of the table.
--
-- RE-RUN SAFETY. capability-core re-applies every *.up.sql at every boot, so everything here is
-- idempotent: the column is added only when missing, and the constraint only when it is missing.

ALTER TABLE tool_policies
    ADD COLUMN IF NOT EXISTS expires_at TIMESTAMPTZ;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
        FROM pg_constraint
        WHERE conrelid = 'tool_policies'::regclass
          AND conname = 'tool_policies_expiry_allow_check'
    ) THEN
        ALTER TABLE tool_policies
            ADD CONSTRAINT tool_policies_expiry_allow_check
            CHECK (expires_at IS NULL OR decision = 'allow');
    END IF;
END;
$$;
