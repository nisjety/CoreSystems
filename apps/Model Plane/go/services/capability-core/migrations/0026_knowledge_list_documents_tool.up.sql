-- 0026_knowledge_list_documents_tool.up.sql
--
-- The declared class of execution-core's `knowledge_list_documents` (WS24-S8): the owner's document
-- inventory, read from the same Data Plane corpus as `knowledge_search` under the run's own bearer,
-- no approval floor. The capability it is governed by, cap.retrieval.query, is already seeded by
-- 0016 for `knowledge_search`, so only the tool's row is added here, in the 0016 seed format so the
-- registry's seed reader can see it. It is re-asserted only while the row is still the seed's, so an
-- operator override is never rewritten.
--
-- Without this row the signed gate fails closed for the tool: execution-core resolves its capability
-- from a seeded row, and an unseeded name evaluates against nothing.
INSERT INTO capability_tools
    (org_id, capability_id, tool_name, side_effect_class, approval_floor, call_refinement, source)
VALUES
    ('global', 'cap.retrieval.query', 'knowledge_list_documents', 'read', 'none', NULL, 'seed')
ON CONFLICT (org_id, capability_id, tool_name) DO UPDATE SET
    side_effect_class = EXCLUDED.side_effect_class,
    approval_floor = EXCLUDED.approval_floor,
    call_refinement = EXCLUDED.call_refinement,
    updated_at = now()
WHERE capability_tools.source = 'seed';
