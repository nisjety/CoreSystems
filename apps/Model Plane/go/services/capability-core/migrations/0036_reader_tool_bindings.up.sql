-- 0036_reader_tool_bindings.up.sql
--
-- The declared class of execution-core's long-page reader (EP-2-S10): `read_page` and `find_in_page`
-- read a page this run already fetched and holds, so they are governed by the capability of the
-- fetch that produced it (cap.tool.http). Only the tools' rows are added here, in the 0016 seed
-- format, and they are re-asserted only while a row is still the seed's, so an operator override
-- is never rewritten.
--
-- Without these rows the signed gate denies both names (tool not bound). Apply before
-- EXECUTION_WEB_READER is ever set to true. 0035 is reserved for EP-5 S6.
INSERT INTO capability_tools
    (org_id, capability_id, tool_name, side_effect_class, approval_floor, call_refinement, source)
VALUES
    ('global', 'cap.tool.http', 'read_page', 'read', 'none', NULL, 'seed'),
    ('global', 'cap.tool.http', 'find_in_page', 'read', 'none', NULL, 'seed')
ON CONFLICT (org_id, capability_id, tool_name) DO UPDATE SET
    side_effect_class = EXCLUDED.side_effect_class,
    approval_floor = EXCLUDED.approval_floor,
    call_refinement = EXCLUDED.call_refinement,
    updated_at = now()
WHERE capability_tools.source = 'seed';
