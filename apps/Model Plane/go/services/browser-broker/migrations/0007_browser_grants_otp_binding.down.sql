-- Reverts 0007_browser_grants_otp_binding.up.sql. Idempotent: every statement
-- checks what it drops, and the whole revert is a no-op when the table does not
-- exist.
--
-- A fill_otp approval means nothing without its binding, and a build that predates
-- this migration would read one back with no origin and no mail connection. So
-- those approvals are deleted first, as 0006's down deletes its extension rows: no
-- fill_otp approval outlives its binding.
DO $$
BEGIN
    IF to_regclass('browser_grants') IS NULL THEN
        RETURN;
    END IF;

    DELETE FROM browser_grants
    WHERE kind = 'sensitive_approval' AND 'fill_otp' = ANY (allowed_actions);

    ALTER TABLE browser_grants DROP CONSTRAINT IF EXISTS browser_grants_grant_origin_check;
    ALTER TABLE browser_grants DROP CONSTRAINT IF EXISTS browser_grants_mail_connection_id_check;
    ALTER TABLE browser_grants DROP COLUMN IF EXISTS grant_origin;
    ALTER TABLE browser_grants DROP COLUMN IF EXISTS mail_connection_id;
END
$$;
