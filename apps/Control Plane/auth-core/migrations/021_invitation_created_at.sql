-- Better Auth 1.6 requires invitation.createdAt in its adapter schema.
-- Backfill existing invitations before making the new column mandatory; the
-- bootstrap migration remains immutable for checksum-safe deployments.
ALTER TABLE invitation
  ADD COLUMN IF NOT EXISTS created_at TIMESTAMP;

UPDATE invitation
SET created_at = NOW()
WHERE created_at IS NULL;

ALTER TABLE invitation
  ALTER COLUMN created_at SET DEFAULT NOW(),
  ALTER COLUMN created_at SET NOT NULL;
