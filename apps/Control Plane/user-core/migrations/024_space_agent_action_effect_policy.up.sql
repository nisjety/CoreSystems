-- A Model run requesting an owner-plane effect needs an explicit, fresh
-- Control entitlement. This remains false until an owner-approved policy says
-- otherwise; a Space/thread decision is never sufficient by itself.
ALTER TABLE space_effect_policies
    ADD COLUMN IF NOT EXISTS agent_action_entitled BOOLEAN NOT NULL DEFAULT FALSE;
