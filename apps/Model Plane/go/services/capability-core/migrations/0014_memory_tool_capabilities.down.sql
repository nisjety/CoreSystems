-- 0014_memory_tool_capabilities.down.sql
DELETE FROM capabilities WHERE id IN ('cap.memory.search', 'cap.memory.index');
