CREATE TABLE IF NOT EXISTS "privacy_consent" (
  "id" text PRIMARY KEY,
  "user_id" text REFERENCES "user"("id") ON DELETE cascade,
  "session_id" text,
  "analytics" boolean NOT NULL DEFAULT false,
  "marketing" boolean NOT NULL DEFAULT false,
  "necessary" boolean NOT NULL DEFAULT true,
  "created_at" timestamp NOT NULL DEFAULT now(),
  "updated_at" timestamp NOT NULL DEFAULT now()
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_privacy_consent_user_id
  ON "privacy_consent"("user_id")
  WHERE "user_id" IS NOT NULL;

CREATE UNIQUE INDEX IF NOT EXISTS idx_privacy_consent_session_id
  ON "privacy_consent"("session_id")
  WHERE "session_id" IS NOT NULL;
