-- Reverses 0040_web_crawl_plan_tool.up.sql: removes the seeded row that declares knowledge.crawl_plan
-- under cap.web.map, and restores that capability's tool_family to the single name 0031 seeds.
--
-- The capability row is 0031's. This file only restores its tool_family, so cap.web.map, its
-- availability and the knowledge.map row stay as they were. The DELETE is scoped to the seed source, so
-- an operator override is left alone, as 0031's down leaves its own overrides.

DELETE FROM capability_tools
WHERE org_id = 'global'
  AND capability_id = 'cap.web.map'
  AND tool_name = 'knowledge.crawl_plan'
  AND source = 'seed';

UPDATE capabilities
SET config_json = jsonb_set(config_json, '{tool_family}', to_jsonb(ARRAY['knowledge.map'])),
    updated_at = now()
WHERE id = 'cap.web.map' AND org_id = 'global';
