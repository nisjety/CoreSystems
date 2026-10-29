ALTER TABLE "oauth_application" ADD COLUMN IF NOT EXISTS "icon" text;
ALTER TABLE "oauth_application" ADD COLUMN IF NOT EXISTS "redirect_ur_ls" text;
ALTER TABLE "oauth_application" ADD COLUMN IF NOT EXISTS "authentication_scheme" text;

DO $$
BEGIN
  IF EXISTS (
    SELECT 1
    FROM information_schema.columns
    WHERE table_schema = 'public'
      AND table_name = 'oauth_application'
      AND column_name = 'redirect_u_r_ls'
  ) THEN
    EXECUTE 'UPDATE "oauth_application"
      SET "redirect_ur_ls" = COALESCE("redirect_ur_ls", "redirect_u_r_ls")
      WHERE "redirect_ur_ls" IS NULL';
  END IF;
END $$;

ALTER TABLE "oauth_application" ALTER COLUMN "disabled" SET DEFAULT false;

UPDATE "oauth_application"
SET "authentication_scheme" = 'client_secret_basic'
WHERE "authentication_scheme" IS NULL;
