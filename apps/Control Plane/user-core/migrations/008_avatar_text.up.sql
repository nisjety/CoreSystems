-- Up migration: Change avatar column from VARCHAR(500) to TEXT
ALTER TABLE users ALTER COLUMN avatar TYPE TEXT;
