-- 049_space_memory_personal_turn_entitlement — WS17-S8 (decision D12): the per-turn
-- personal-memory opt-in in a shared Space gets its own deny-by-default entitlement.
-- The per-turn decision (model.memory.personal_context) used to reuse
-- memory_declassify_entitled, so an org that allowed declassifying a memory also
-- allowed personal memory in rooms. The column defaults FALSE for every organisation
-- and no preset carries it. Control refuses the decision under Space zero data
-- retention, so the bit is ZDR inert (catalogue row, entitlement_catalog.go).
-- Number: plan 4.6.B reserves 048 for WS20-S9, so 049 is the first spare number after it.
ALTER TABLE space_effect_policies
    ADD COLUMN IF NOT EXISTS memory_personal_turn_entitled BOOLEAN NOT NULL DEFAULT FALSE;
