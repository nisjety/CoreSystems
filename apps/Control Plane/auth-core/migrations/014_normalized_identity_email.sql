-- Prevent concurrent OAuth callbacks from creating canonical users that differ
-- only by email case. This migration intentionally fails if historical
-- collisions exist; operators must audit and reconcile those identities first.
CREATE UNIQUE INDEX IF NOT EXISTS user_email_normalized_unique
  ON "user" (LOWER(BTRIM(email)));

CREATE UNIQUE INDEX IF NOT EXISTS account_provider_account_unique
  ON account (provider_id, account_id);
