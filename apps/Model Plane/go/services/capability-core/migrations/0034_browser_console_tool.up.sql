-- 0034_browser_console_tool.up.sql
--
-- The declared class of execution-core's `browser.console` (WS9.E): a bounded, scrubbed, read-only
-- look at an existing Quarry browser run's console, under the run's broker grant. It is governed by
-- the capability its siblings `browser.observe` and `browser.act` already use (cap.browser.open, seeded
-- by 0016), so only the tool's row is added here, in the 0016 seed format so the registry's seed
-- reader can see it. It is re-asserted only while the row is still the seed's, so an operator
-- override is never rewritten.
--
-- Without this row the signed gate fails closed for the tool: execution-core resolves its capability
-- from a seeded row, and an unseeded name evaluates against nothing.
INSERT INTO capability_tools
    (org_id, capability_id, tool_name, side_effect_class, approval_floor, call_refinement, source)
VALUES
    ('global', 'cap.browser.open', 'browser.console', 'read', 'none', NULL, 'seed')
ON CONFLICT (org_id, capability_id, tool_name) DO UPDATE SET
    side_effect_class = EXCLUDED.side_effect_class,
    approval_floor = EXCLUDED.approval_floor,
    call_refinement = EXCLUDED.call_refinement,
    updated_at = now()
WHERE capability_tools.source = 'seed';
