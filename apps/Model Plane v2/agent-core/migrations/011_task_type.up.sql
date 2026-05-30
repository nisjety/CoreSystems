-- 011_task_type.up.sql
-- Add task_type column to agent_tasks for executor dispatch.

ALTER TABLE agent_tasks
    ADD COLUMN IF NOT EXISTS task_type TEXT NOT NULL DEFAULT 'general'
        CHECK (task_type IN ('general','local_bash','sub_agent','code_review','file_edit'));

CREATE INDEX IF NOT EXISTS idx_tasks_type ON agent_tasks(task_type);
