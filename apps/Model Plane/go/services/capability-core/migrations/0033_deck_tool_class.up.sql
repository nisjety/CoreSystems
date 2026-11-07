-- 0033_deck_tool_class.up.sql
--
-- The declared class of model-gateway's `deck` tool (EP-5 S2): typed slide operations on a
-- manifest the gateway owns, and a build the gateway runs as a server-authored code_interpreter
-- step (fixed command, manifest staged through files_in). It computes in the networkless sandbox
-- and writes nothing outside it, so it carries code_interpreter's capability and class
-- (cap.command.sandbox, sandbox_compute, no approval floor; 0016). Only the tool's row is added,
-- in the 0016 seed format so the registry's seed reader can see it. It is re-asserted only while
-- the row is still the seed's, so an operator override is never rewritten.
--
-- execution-core never sees the name `deck`: the step it runs is `code_interpreter`, gated by that
-- tool's own row. This row classes the gateway tool for every reader of declared classes.
INSERT INTO capability_tools
    (org_id, capability_id, tool_name, side_effect_class, approval_floor, call_refinement, source)
VALUES
    ('global', 'cap.command.sandbox', 'deck', 'sandbox_compute', 'none', NULL, 'seed')
ON CONFLICT (org_id, capability_id, tool_name) DO UPDATE SET
    side_effect_class = EXCLUDED.side_effect_class,
    approval_floor = EXCLUDED.approval_floor,
    call_refinement = EXCLUDED.call_refinement,
    updated_at = now()
WHERE capability_tools.source = 'seed';
