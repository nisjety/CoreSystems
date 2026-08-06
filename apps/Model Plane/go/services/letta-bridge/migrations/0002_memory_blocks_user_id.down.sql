DROP INDEX IF EXISTS idx_letta_memory_org_user;
ALTER TABLE letta_memory_blocks DROP COLUMN IF EXISTS user_id;
