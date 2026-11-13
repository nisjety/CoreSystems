-- 0040_web_crawl_plan_tool.up.sql
--
-- Q4-E4.3-055c2 (E4.3 Track B F-t2-055): declares the knowledge.crawl_plan tool class under
-- cap.web.map, in the 0016 seed format that 0031 uses for knowledge.map.
--
--   cap.web.map  knowledge.crawl_plan  read, low risk, no approval floor. The crawl planner maps a
--                                      site through the same /v1/map read as knowledge.map and drafts
--                                      a crawl scope for the person to approve in the page picker. It
--                                      starts no job and stores nothing, so it is a read and never a
--                                      context write. A zero-retention run keeps it.
--
-- WHY NO NEW CAPABILITY. The planner reads the same site map that knowledge.map lists, under the same
-- scope and egress, so it shares cap.web.map and adds no capability row to attest. A tool name is
-- bound to exactly one capability (execution-core resolves the capability from the name alone), so a
-- second capability for the same read would split one authority across two rows.
--
-- AVAILABILITY IS NOT CHANGED HERE. This migration writes no availability column and records no
-- attestation. cap.web.map keeps the unavailable state 0031 seeds until a health attestation for it is
-- recorded. The planner is offered to the model, and refused when called, only while the deployment has
-- switched the crawl tools on with EXECUTION_CORE_WEB_CRAWL_TOOLS.
--
-- THE NAME RESOLVES. execution-core binds knowledge.crawl_plan to cap.web.map in capability_policy.rs
-- (Q4-E4.3-055c1). The parity test (tests/tool_class_registry_parity.rs) refuses a bound name that has no
-- seed, and the row below is that seed.
--
-- RE-RUN SAFETY. capability-core re-applies every *.up.sql at every boot. 0031 resets the tool_family of
-- cap.web.map at each boot and this file runs after it, so the UPDATE below re-adds the planner name on
-- every boot. The seed row is re-asserted only while it is still the seed's (WHERE source = 'seed'), so
-- an operator override is never rewritten.

-- The declared class of the planner, one tuple per line: the registry seed reader and execution-core's
-- tool_class_registry_parity.rs read these rows as text.
INSERT INTO capability_tools
    (org_id, capability_id, tool_name, side_effect_class, approval_floor, call_refinement, source)
VALUES
    ('global', 'cap.web.map', 'knowledge.crawl_plan', 'read', 'none', NULL, 'seed')
ON CONFLICT (org_id, capability_id, tool_name) DO UPDATE SET
    side_effect_class = EXCLUDED.side_effect_class,
    approval_floor = EXCLUDED.approval_floor,
    call_refinement = EXCLUDED.call_refinement,
    updated_at = now()
WHERE capability_tools.source = 'seed';

-- tool_family lists the names the capability's family serves, as 0031 does for knowledge.map. The
-- UPDATE is keyed on the global row's id, so no other capability is touched.
UPDATE capabilities
SET config_json = jsonb_set(config_json, '{tool_family}', to_jsonb(ARRAY['knowledge.map', 'knowledge.crawl_plan'])),
    updated_at = now()
WHERE id = 'cap.web.map' AND org_id = 'global';
