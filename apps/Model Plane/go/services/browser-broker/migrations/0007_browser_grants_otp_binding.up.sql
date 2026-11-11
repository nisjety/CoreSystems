-- browser-broker 0007: the fill_otp binding (Q3-E3.1-b2-c, ADR-2a).
--
-- 0002 stays reserved for WS7 (R-3), 0006 is the origin-extension kind and 0008
-- is the web-policy snapshot. 0008's header reserves this number for the fill_otp
-- binding, and the container entrypoint applies every *.up.sql in lexical order,
-- so this file runs before 0008 on every start.
--
-- The container entrypoint re-applies every *.up.sql on each start, so every
-- statement here is idempotent: ADD COLUMN IF NOT EXISTS, and each constraint is
-- added only when no constraint of that name exists yet.
--
-- What changes:
--   * mail_connection_id   the one mail connection a fill_otp approval reads its
--                          one-time code from, 1 to 128 characters. NULL for every
--                          other grant, and for every row written before this
--                          column.
--   * grant_origin         the one canonical https origin a fill_otp approval types
--                          the code into. NULL for every other grant.
--
-- The broker checks both before it writes (checkFillOtpBinding in
-- internal/grant/fill_otp.go). The two CHECKs are this table's own fence, as
-- migration 0006's are. The values are stored verbatim and never logged.
--
-- Still no zdr column: a ZDR fill_otp approval is refused before it reaches this
-- table (ErrFillOtpZdrRefused, ErrZdrPostureChanged), so no ZDR binding is stored
-- here.

ALTER TABLE browser_grants
    ADD COLUMN IF NOT EXISTS mail_connection_id TEXT NULL,
    ADD COLUMN IF NOT EXISTS grant_origin TEXT NULL;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conname = 'browser_grants_mail_connection_id_check'
          AND conrelid = 'browser_grants'::regclass
    ) THEN
        ALTER TABLE browser_grants ADD CONSTRAINT browser_grants_mail_connection_id_check
            CHECK (mail_connection_id IS NULL OR char_length(mail_connection_id) BETWEEN 1 AND 128);
    END IF;

    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conname = 'browser_grants_grant_origin_check'
          AND conrelid = 'browser_grants'::regclass
    ) THEN
        ALTER TABLE browser_grants ADD CONSTRAINT browser_grants_grant_origin_check
            CHECK (grant_origin IS NULL OR grant_origin LIKE 'https://%');
    END IF;
END
$$;
