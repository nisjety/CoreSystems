-- 0029_ask_user_tool_class.up.sql
--
-- The declared class of execution-core's `ask_user` (WS14-S4): the Do agent puts a question to the
-- run's owner, which is stored in run_questions and answered on the next turn. It writes only the
-- run's own context and grants nothing (an answer is never an approval, DEC-05), so it is
-- context_write with no approval floor, governed by cap.memory.index like plan.propose (0016).
-- Only the tool's row is added, in the 0016 seed format so the registry's seed reader can see it.
-- It is re-asserted only while the row is still the seed's, so an operator override is never
-- rewritten.
--
-- Without this row, TOOL_CLASS_GATE=enforce treats ask_user as unclassified, with an approval floor.
INSERT INTO capability_tools
    (org_id, capability_id, tool_name, side_effect_class, approval_floor, call_refinement, source)
VALUES
    ('global', 'cap.memory.index', 'ask_user', 'context_write', 'none', NULL, 'seed')
ON CONFLICT (org_id, capability_id, tool_name) DO UPDATE SET
    side_effect_class = EXCLUDED.side_effect_class,
    approval_floor = EXCLUDED.approval_floor,
    call_refinement = EXCLUDED.call_refinement,
    updated_at = now()
WHERE capability_tools.source = 'seed';
