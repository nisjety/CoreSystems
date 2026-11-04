-- Reverses 0027_browser_tool_capabilities.up.sql: removes only the two browser capabilities this
-- migration created and the seeded rows of their tools.
--
-- capability_scopes, capability_health, capability_versions and skill_packages carry foreign keys
-- onto capabilities(id), so any rows attached to these two ids are removed first. Every delete is
-- scoped to these ids, so cap.browser.open (0008), the 0016 tool classes of the legacy browser_agent
-- and the rest of the catalogue are untouched.

DELETE FROM capability_tools
WHERE org_id = 'global'
  AND capability_id IN ('cap.browser.observe', 'cap.browser.act')
  AND source = 'seed';

-- Gives the two moved tools back to cap.browser.open, with the 0016 rows they had there.
INSERT INTO capability_tools
    (org_id, capability_id, tool_name, side_effect_class, approval_floor, call_refinement, source)
VALUES
    ('global', 'cap.browser.open', 'browser.observe', 'read', 'none', NULL, 'seed'),
    ('global', 'cap.browser.open', 'browser.act', 'external_write', 'none', NULL, 'seed')
ON CONFLICT (org_id, capability_id, tool_name) DO NOTHING;

DELETE FROM capability_scopes WHERE capability_id IN ('cap.browser.observe', 'cap.browser.act');
DELETE FROM capability_health WHERE capability_id IN ('cap.browser.observe', 'cap.browser.act');
DELETE FROM capability_versions WHERE capability_id IN ('cap.browser.observe', 'cap.browser.act');
DELETE FROM skill_packages WHERE capability_id IN ('cap.browser.observe', 'cap.browser.act');

DELETE FROM capabilities
WHERE id IN ('cap.browser.observe', 'cap.browser.act')
  AND created_by = 'migration:0027_browser_tool_capabilities';
