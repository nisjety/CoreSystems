-- Down migration: Revert avatar column back to VARCHAR(500)
ALTER TABLE users ALTER COLUMN avatar TYPE VARCHAR(500);
