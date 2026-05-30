-- 011_task_type.down.sql
ALTER TABLE agent_tasks DROP COLUMN IF EXISTS task_type;
