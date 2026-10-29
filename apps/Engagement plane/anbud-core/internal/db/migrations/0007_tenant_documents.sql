-- 0007_tenant_documents.sql
-- Tender documents a company uploaded for a notice: what was in the archive, and the passages of
-- the documents that name a certificate or a registration the gate knows (docs/ARCHITECTURE.md,
-- "Tender documents"). The text of the documents is not stored: it is read while the archive is
-- uploaded and then dropped, and what is kept is the manifest and the quoted sentences.
--
-- This is tenant data, and it lives in a schema of its own, for what it is not: it is not evidence.
-- The tables of schema public are the public corpus and the sync's ledger, append-only, and the
-- role model (docs/ROLES.md) is built on that. Here the application inserts, reads and deletes rows
-- that belong to one organisation, and each row has an expiry.
--
-- Isolation is in two places. Every query of the store names the organisation. And row level
-- security makes the database refuse a row of another organisation, whatever a query forgets: a row
-- is visible only when its org_id is the one the session set with set_config('anbud.org_id', ...,
-- true), and with none set, no row is. That setting is the session's own, though: a session that can
-- run SQL can name any organisation, so this protects against a query that forgets its condition and
-- not against a stolen connection or an injection. Row level security binds the roles that own
-- nothing, which is the application's once the role model is in use. It does not bind the owner or a
-- superuser, which a database run with one DSN is (docs/ROLES.md). It is not FORCEd, so the purge
-- below, which runs as the owner, is not held to it; if it is ever FORCEd, the purge needs a policy
-- of its own or it will delete nothing.
--
-- Expired rows are removed by tenant.purge_expired_uploads(), the one thing the application may do
-- across organisations, and all it does is delete what has expired. It is SECURITY DEFINER so that it
-- is not held to the policy, and it is pinned to pg_catalog so a schema in the caller's search_path
-- cannot stand in for anything it uses.
--
-- Privileges for the application role are stated in tenant_privileges.sql, which runs after every
-- migration and so cannot be undone by the order the roles and the migrations came in. Nobody else is
-- granted anything: not PUBLIC, and not the DBA group, which has no business with a tenant's documents.
-- The application may insert every column but id and created_at: the identity and the clock are the
-- database's, so a session cannot squat a future id or date a row into the future.
--
-- Forward-only. Runs inside the caller's transaction (migrate.go, applyOne), so no BEGIN or COMMIT.

CREATE SCHEMA tenant;

CREATE TABLE tenant.tender_uploads (
    id             BIGINT      GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    -- Who it belongs to. The same shape the API accepts in X-Org-ID.
    org_id         TEXT        NOT NULL CHECK (org_id ~ '^[A-Za-z0-9][A-Za-z0-9_.:-]{0,127}$'),
    -- The notice it was uploaded for: a Doffin notice id.
    notice_id      TEXT        NOT NULL CHECK (notice_id ~ '^[0-9]{4}-[0-9]{4,9}$'),
    -- The user who uploaded it, as the gateway names them (an id, never a name).
    uploaded_by    TEXT        CHECK (uploaded_by IS NULL OR uploaded_by ~ '^[A-Za-z0-9][A-Za-z0-9_.:-]{0,127}$'),
    -- The archive as it was sent: its name with control characters removed, its hash and its size.
    -- The bytes are not kept.
    archive_name   TEXT        NOT NULL DEFAULT '' CHECK (char_length(archive_name) <= 200),
    archive_sha256 TEXT        NOT NULL CHECK (archive_sha256 ~ '^[0-9a-f]{64}$'),
    archive_bytes  BIGINT      NOT NULL CHECK (archive_bytes > 0),
    -- What was in it, file by file: path, kind, size, hash, and whether it was read. An array of
    -- objects, written by the store, which validates them; the database bounds the array and its size.
    documents      JSONB       NOT NULL CHECK (jsonb_typeof(documents) = 'array'
                                                AND jsonb_array_length(documents) <= 1000
                                                AND octet_length(documents::text) <= 1048576),
    -- The passages that name a capability, each with its file, its page or paragraph and its sentence.
    findings       JSONB       NOT NULL CHECK (jsonb_typeof(findings) = 'array'
                                                AND jsonb_array_length(findings) <= 1000
                                                AND octet_length(findings::text) <= 1048576),
    read_count     INTEGER     NOT NULL CHECK (read_count >= 0),
    unread_count   INTEGER     NOT NULL CHECK (unread_count >= 0),
    -- How many passages were left out of findings because an upload keeps only so many of them.
    -- Nothing is dropped without a count.
    findings_dropped INTEGER   NOT NULL DEFAULT 0 CHECK (findings_dropped >= 0),
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at     TIMESTAMPTZ NOT NULL,
    CONSTRAINT tender_uploads_expires_after_created CHECK (expires_at > created_at),
    -- Nothing is held for longer than two years and two days, whatever the application asks for. The
    -- bound is in hours, not in years or days: a year is not a fixed length, and a day is not 24 hours in a
    -- session whose clock changes, so a retention the service is allowed to ask for (730 days) must not
    -- fail the check in a leap year or across a change of clock.
    CONSTRAINT tender_uploads_expires_within_two_years CHECK (expires_at <= created_at + interval '17568 hours'),
    -- The same archive for the same notice by the same organisation is one upload.
    CONSTRAINT tender_uploads_once UNIQUE (org_id, notice_id, archive_sha256)
);

-- Lookups by organisation and notice are served by the unique index above, whose first two columns they are.
CREATE INDEX tender_uploads_by_expiry ON tenant.tender_uploads (expires_at);

ALTER TABLE tenant.tender_uploads ENABLE ROW LEVEL SECURITY;

-- One policy for every command: a row is the session's own organisation's, or it is nobody's. With no
-- organisation set current_setting returns NULL, the comparison is NULL, and no row is visible.
CREATE POLICY tender_uploads_own_organisation ON tenant.tender_uploads
    USING (org_id = (SELECT current_setting('anbud.org_id', true)))
    WITH CHECK (org_id = (SELECT current_setting('anbud.org_id', true)));

CREATE FUNCTION tenant.purge_expired_uploads() RETURNS bigint
    LANGUAGE plpgsql
    SECURITY DEFINER
    SET search_path = pg_catalog, pg_temp
AS $purge$
DECLARE
    removed bigint;
BEGIN
    DELETE FROM tenant.tender_uploads WHERE expires_at <= pg_catalog.now();
    GET DIAGNOSTICS removed = ROW_COUNT;
    RETURN removed;
END
$purge$;

REVOKE ALL ON FUNCTION tenant.purge_expired_uploads() FROM PUBLIC;

COMMENT ON SCHEMA tenant IS 'Tenant working data: not evidence. Row level security by organisation; each row expires. The application role is granted what it needs in tenant_privileges.sql; PUBLIC and the DBA group are granted nothing.';
COMMENT ON TABLE tenant.tender_uploads IS 'One archive of tender documents that an organisation uploaded for a notice: the manifest and the quoted passages, not the text and not the bytes. Visible only to the organisation named in the session setting anbud.org_id. Each row has an expiry; tenant.purge_expired_uploads() removes the expired.';
COMMENT ON FUNCTION tenant.purge_expired_uploads() IS 'Deletes every upload whose expiry has passed, for every organisation, and returns how many. SECURITY DEFINER, so it is not held to the row level policy; it does nothing else.';
