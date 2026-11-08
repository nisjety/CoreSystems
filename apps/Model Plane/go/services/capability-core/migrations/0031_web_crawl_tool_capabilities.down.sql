-- Reverses 0031_web_crawl_tool_capabilities.up.sql: removes only the three web capabilities this
-- migration created and the seeded rows of their tools.
--
-- capability_scopes, capability_health, capability_versions and skill_packages carry foreign keys
-- onto capabilities(id), so any rows attached to these ids are removed first. Every delete is
-- scoped to these ids, so cap.tool.http (0008), the 0016 tool classes and the rest of the
-- catalogue are untouched.

DELETE FROM capability_tools
WHERE org_id = 'global'
  AND capability_id IN ('cap.web.crawl', 'cap.web.map', 'cap.web.batch')
  AND source = 'seed';

DELETE FROM capability_scopes WHERE capability_id IN ('cap.web.crawl', 'cap.web.map', 'cap.web.batch');
DELETE FROM capability_health WHERE capability_id IN ('cap.web.crawl', 'cap.web.map', 'cap.web.batch');
DELETE FROM capability_versions WHERE capability_id IN ('cap.web.crawl', 'cap.web.map', 'cap.web.batch');
DELETE FROM skill_packages WHERE capability_id IN ('cap.web.crawl', 'cap.web.map', 'cap.web.batch');

DELETE FROM capabilities
WHERE id IN ('cap.web.crawl', 'cap.web.map', 'cap.web.batch')
  AND created_by = 'migration:0031_web_crawl_tool_capabilities';
