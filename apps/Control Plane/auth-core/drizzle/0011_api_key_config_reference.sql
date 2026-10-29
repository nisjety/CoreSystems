ALTER TABLE "apikey" ADD COLUMN IF NOT EXISTS "config_id" text;
ALTER TABLE "apikey" ADD COLUMN IF NOT EXISTS "reference_id" text;

UPDATE "apikey"
SET "config_id" = COALESCE(NULLIF("config_id", ''), 'user-keys');

UPDATE "apikey"
SET "reference_id" = COALESCE(NULLIF("reference_id", ''), "user_id");

ALTER TABLE "apikey" ALTER COLUMN "config_id" SET DEFAULT 'user-keys';
ALTER TABLE "apikey" ALTER COLUMN "config_id" SET NOT NULL;
ALTER TABLE "apikey" ALTER COLUMN "reference_id" SET NOT NULL;
ALTER TABLE "apikey" ALTER COLUMN "user_id" DROP NOT NULL;

CREATE INDEX IF NOT EXISTS idx_apikey_config_id
  ON apikey(config_id);

CREATE INDEX IF NOT EXISTS idx_apikey_reference_id
  ON apikey(reference_id);

CREATE INDEX IF NOT EXISTS idx_apikey_config_reference
  ON apikey(config_id, reference_id);

CREATE INDEX IF NOT EXISTS idx_apikey_key_hash
  ON apikey(key);

CREATE INDEX IF NOT EXISTS idx_apikey_reference_enabled
  ON apikey(reference_id, enabled)
  WHERE enabled = true;
