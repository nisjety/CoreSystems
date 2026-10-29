-- Reverses 0026_knowledge_list_documents_tool.up.sql: removes only the seeded row of this one tool.
DELETE FROM capability_tools
WHERE org_id = 'global'
  AND capability_id = 'cap.retrieval.query'
  AND tool_name = 'knowledge_list_documents'
  AND source = 'seed';
