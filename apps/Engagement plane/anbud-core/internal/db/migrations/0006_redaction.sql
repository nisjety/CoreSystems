-- 0006_redaction.sql
-- Erasing a notice body (open owner decision O1) as a recorded DBA procedure, not as
-- application code. raw_documents stays append-only. The one change it admits is a
-- redaction: the body becomes empty and redacted_at is set, and every other column stays,
-- the hash of the original bytes included. The design treats a hash as not personal data
-- (open question 9 of the runbook asks compliance to confirm it), and keeping it keeps the
-- identity (source, doffin_id, body_sha256): the same bytes downloaded again find the
-- redacted row, INSERT ... ON CONFLICT DO NOTHING does nothing, and the erased body is
-- never stored a second time. The runbook is docs/PERSONAL-DATA.md.
--
-- 1. raw_documents.redacted_at is NULL while the body is held. The two column checks that
--    forbid an erased body, the body is not empty (raw_documents_body_check) and the hash is
--    the body's (raw_documents_check, which PostgreSQL named so because it reads two
--    columns), are replaced under their own names and hold only while it is NULL. The format
--    of the hash is still checked on every row. A third check makes a redacted body empty.
--    All three are added NOT VALID: they are enforced for every row written from now on,
--    and the existing rows are not scanned. Validating would hash every stored body, for
--    minutes on a full corpus, under the exclusive lock this migration holds, and it could
--    find nothing: every row that exists already satisfies the stronger checks of 0002, and
--    redacted_at is NULL on all of them. ALTER TABLE raw_documents VALIDATE CONSTRAINT <name>
--    marks them validated later, under a lock that does not block the sync.
-- 2. raw_redactions is the tombstone of each redaction: one row per document, with the
--    original hash and size, a reason from a fixed list, a request reference of a fixed
--    shape (a ticket id: no free text, so no personal data), who and when. Append-only.
-- 3. anbud_redact_raw_document(id, reason, request) is the only way to redact. It is
--    SECURITY DEFINER and belongs to the migrator, so the DBA group needs no privilege to
--    change evidence, only EXECUTE. It pins its search path with pg_temp last and names
--    every table and function with its schema, because every role may create temporary
--    tables, and a temporary table named like a table of the function would otherwise be
--    written instead of it. In one transaction it locks the row, refuses an unknown or
--    already redacted id, validates the reason and the request, writes the tombstone,
--    empties the body, and logs one line with ids only.
-- 4. raw_documents_change_guard() replaces raw_append_only_guard() on UPDATE and DELETE of
--    raw_documents, under the same trigger name. DELETE is refused always. UPDATE is refused
--    except for exactly the transformation of the function: while a door that names this row
--    and this transaction is open (a transaction-local setting the function sets), the row
--    goes from not redacted to redacted with an empty body and nothing else changed, and the
--    tombstone for the row, with its hash, its size and the same time, is already written.
--    The door is not a secret and gives no privilege. Setting it by hand opens that one
--    transformation of that one row, for a role that may UPDATE the table, which only the
--    owner may.
-- 5. raw_documents_insert_guard() refuses an INSERT of a row that already carries
--    redacted_at, so the relaxed checks cannot be used to store a row nobody checked.
-- 6. raw_documents_live is the rows that still hold their body: the contract for a reader
--    such as the parser. A reader that honours the redaction list reads this view.
--
-- Privileges are stated here, because this migration may run before or after
-- scripts/roles.sql. The function is revoked from PUBLIC in the open. Anything that names a
-- role of the role model runs only when the role exists, so the migration works on a
-- database that has none. The DBA group gets EXECUTE if it exists when this runs, and
-- scripts/roles.sql grants it when it runs after. The default privileges of that script
-- hand the application INSERT on a new table, and nobody but the function may insert a
-- tombstone, so INSERT is revoked from the application on the tombstones and on the view.
--
-- Forward-only: 0002 to 0005 are not edited. Runs inside the caller's transaction
-- (migrate.go, applyOne), so no BEGIN, COMMIT or ROLLBACK. migrate_concurrency_test.go drops
-- the tombstone table and the three functions before each round, and the view and the new
-- column with the table raw_documents.

ALTER TABLE raw_documents ADD COLUMN redacted_at TIMESTAMPTZ;

ALTER TABLE raw_documents DROP CONSTRAINT raw_documents_body_check;
ALTER TABLE raw_documents ADD CONSTRAINT raw_documents_body_check CHECK (
    redacted_at IS NOT NULL OR octet_length(body) > 0) NOT VALID;

ALTER TABLE raw_documents DROP CONSTRAINT raw_documents_check;
ALTER TABLE raw_documents ADD CONSTRAINT raw_documents_check CHECK (
    body_sha256 ~ '^[0-9a-f]{64}$'
    AND (redacted_at IS NOT NULL OR body_sha256 = encode(sha256(body), 'hex'))) NOT VALID;

ALTER TABLE raw_documents ADD CONSTRAINT raw_documents_redacted_is_empty CHECK (
    redacted_at IS NULL OR octet_length(body) = 0) NOT VALID;

CREATE TABLE raw_redactions (
    id              BIGINT      GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    raw_document_id BIGINT      NOT NULL REFERENCES raw_documents (id),
    doffin_id       TEXT        NOT NULL CHECK (doffin_id ~ '^[0-9]{4}-[0-9]{4,9}$'),
    original_sha256 TEXT        NOT NULL CHECK (original_sha256 ~ '^[0-9a-f]{64}$'),
    original_size   BIGINT      NOT NULL CHECK (original_size > 0),
    reason          TEXT        NOT NULL CHECK (reason IN (
                        'erasure_request', 'objection', 'source_withdrawn',
                        'authority_order', 'controller_decision')),
    request_ref     TEXT        NOT NULL CHECK (request_ref ~ '^[A-Za-z0-9._:/-]{1,120}$'),
    redacted_by     TEXT        NOT NULL CHECK (char_length(redacted_by) BETWEEN 1 AND 128),
    redacted_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT raw_redactions_one_per_document UNIQUE (raw_document_id)
);

CREATE TRIGGER raw_redactions_append_only BEFORE UPDATE OR DELETE ON raw_redactions
    FOR EACH ROW EXECUTE FUNCTION raw_append_only_guard();
CREATE TRIGGER raw_redactions_no_truncate BEFORE TRUNCATE ON raw_redactions
    FOR EACH STATEMENT EXECUTE FUNCTION raw_append_only_guard();

DO $redaction$
BEGIN
    IF EXISTS (SELECT 1 FROM pg_catalog.pg_roles WHERE rolname = 'anbud_app') THEN
        REVOKE INSERT ON raw_redactions FROM anbud_app;
    END IF;
END
$redaction$;

CREATE FUNCTION raw_documents_change_guard() RETURNS trigger
LANGUAGE plpgsql
SET search_path = pg_catalog, public, pg_temp
AS $guard$
BEGIN
    IF TG_OP = 'UPDATE' THEN
        IF pg_catalog.current_setting('anbud.redacting_raw_document', true)
               = OLD.id::text || ':' || pg_catalog.txid_current()::text
           AND OLD.redacted_at IS NULL
           AND NEW.redacted_at IS NOT NULL
           AND pg_catalog.octet_length(NEW.body) = 0
           AND NEW.id = OLD.id
           AND NEW.source = OLD.source
           AND NEW.doffin_id = OLD.doffin_id
           AND NEW.data_class = OLD.data_class
           AND NEW.content_type = OLD.content_type
           AND NEW.body_sha256 = OLD.body_sha256
           AND NEW.fetched_at = OLD.fetched_at
           AND NEW.stored_at = OLD.stored_at
           AND NEW.run_id = OLD.run_id
           AND EXISTS (SELECT 1
                         FROM public.raw_redactions r
                        WHERE r.raw_document_id = OLD.id
                          AND r.original_sha256 = OLD.body_sha256
                          AND r.original_size = pg_catalog.octet_length(OLD.body)
                          AND r.redacted_at = NEW.redacted_at)
        THEN
            RETURN NEW;
        END IF;
        RAISE EXCEPTION 'UPDATE on % refused: table is append-only, and the one change it admits is made by anbud_redact_raw_document()', TG_TABLE_NAME
            USING ERRCODE = 'restrict_violation';
    END IF;
    RAISE EXCEPTION '% on % refused: table is append-only', TG_OP, TG_TABLE_NAME
        USING ERRCODE = 'restrict_violation';
END;
$guard$;

DROP TRIGGER raw_documents_append_only ON raw_documents;
CREATE TRIGGER raw_documents_append_only BEFORE UPDATE OR DELETE ON raw_documents
    FOR EACH ROW EXECUTE FUNCTION raw_documents_change_guard();

CREATE FUNCTION raw_documents_insert_guard() RETURNS trigger
LANGUAGE plpgsql
SET search_path = pg_catalog, public, pg_temp
AS $guard$
BEGIN
    IF NEW.redacted_at IS NOT NULL THEN
        RAISE EXCEPTION 'INSERT into % refused: a new row cannot arrive redacted', TG_TABLE_NAME
            USING ERRCODE = 'restrict_violation';
    END IF;
    RETURN NEW;
END;
$guard$;

CREATE TRIGGER raw_documents_no_redacted_insert BEFORE INSERT ON raw_documents
    FOR EACH ROW EXECUTE FUNCTION raw_documents_insert_guard();

CREATE FUNCTION anbud_redact_raw_document(p_id BIGINT, p_reason TEXT, p_request TEXT)
RETURNS BIGINT
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public, pg_temp
AS $redact$
DECLARE
    v_doffin_id TEXT;
    v_sha256    TEXT;
    v_size      BIGINT;
    v_redacted  TIMESTAMPTZ;
    v_tombstone BIGINT;
    v_rows      BIGINT;
    v_at        TIMESTAMPTZ := pg_catalog.transaction_timestamp();
BEGIN
    IF p_id IS NULL OR p_reason IS NULL OR p_request IS NULL THEN
        RAISE EXCEPTION 'redaction refused: the document id, the reason and the request reference are all required'
            USING ERRCODE = 'null_value_not_allowed';
    END IF;
    IF p_reason NOT IN ('erasure_request', 'objection', 'source_withdrawn', 'authority_order', 'controller_decision') THEN
        RAISE EXCEPTION 'redaction refused: the reason is not one of the allowed values'
            USING ERRCODE = 'invalid_parameter_value';
    END IF;
    IF p_request !~ '^[A-Za-z0-9._:/-]{1,120}$' THEN
        RAISE EXCEPTION 'redaction refused: the request reference does not have the required shape'
            USING ERRCODE = 'invalid_parameter_value';
    END IF;

    SELECT d.doffin_id, d.body_sha256, pg_catalog.octet_length(d.body), d.redacted_at
      INTO v_doffin_id, v_sha256, v_size, v_redacted
      FROM public.raw_documents d
     WHERE d.id = p_id
       FOR UPDATE;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'redaction refused: no document has the id %', p_id
            USING ERRCODE = 'no_data_found';
    END IF;
    IF v_redacted IS NOT NULL THEN
        RAISE EXCEPTION 'redaction refused: document % is already redacted', p_id
            USING ERRCODE = 'object_not_in_prerequisite_state';
    END IF;

    INSERT INTO public.raw_redactions
           (raw_document_id, doffin_id, original_sha256, original_size, reason, request_ref, redacted_by, redacted_at)
    VALUES (p_id, v_doffin_id, v_sha256, v_size, p_reason, p_request, session_user, v_at)
    RETURNING id INTO v_tombstone;

    PERFORM pg_catalog.set_config('anbud.redacting_raw_document', p_id::text || ':' || pg_catalog.txid_current()::text, true);
    UPDATE public.raw_documents SET body = ''::bytea, redacted_at = v_at WHERE id = p_id;
    GET DIAGNOSTICS v_rows = ROW_COUNT;
    PERFORM pg_catalog.set_config('anbud.redacting_raw_document', '', true);
    -- Locked above, so there is exactly one row to change. Anything else, a trigger that
    -- swallowed the update for one, means the body is still there: fail, and the whole
    -- call, tombstone included, rolls back, rather than return a tombstone for nothing.
    IF v_rows <> 1 THEN
        RAISE EXCEPTION 'redaction failed: the update of document % changed % rows, not 1', p_id, v_rows
            USING ERRCODE = 'internal_error';
    END IF;

    RAISE LOG 'anbud redaction: document=% notice=% tombstone=% reason=%', p_id, v_doffin_id, v_tombstone, p_reason;
    RETURN v_tombstone;
END;
$redact$;

REVOKE ALL ON FUNCTION anbud_redact_raw_document(BIGINT, TEXT, TEXT) FROM PUBLIC;

CREATE VIEW raw_documents_live AS
    SELECT id, source, doffin_id, data_class, content_type, body, body_sha256, fetched_at, stored_at, run_id, redacted_at
      FROM raw_documents
     WHERE redacted_at IS NULL;

DO $redaction$
BEGIN
    IF EXISTS (SELECT 1 FROM pg_catalog.pg_roles WHERE rolname = 'anbud_app') THEN
        REVOKE INSERT ON raw_documents_live FROM anbud_app;
    END IF;
    IF EXISTS (SELECT 1 FROM pg_catalog.pg_roles WHERE rolname = 'anbud_dba') THEN
        GRANT EXECUTE ON FUNCTION anbud_redact_raw_document(BIGINT, TEXT, TEXT) TO anbud_dba;
    END IF;
END
$redaction$;

COMMENT ON TABLE raw_documents IS 'Doffin notice bodies exactly as downloaded. Append-only, except that anbud_redact_raw_document() can erase one body: the row stays with redacted_at set, an empty body and the hash of the original bytes. Each body may hold contact details of persons named in the notice. Public corpus, no tenant column.';
COMMENT ON COLUMN raw_documents.redacted_at IS 'NULL while the body is held. Set once, by anbud_redact_raw_document(), when the body is erased. The body is then empty and body_sha256 still names the original bytes.';
COMMENT ON TABLE raw_redactions IS 'One row per redacted notice body: the row, the hash and size of the bytes erased, the reason, the request reference, who and when. Append-only. Built to hold no personal data: the reference is a ticket-shaped id, and the one caveat is the login name of whoever ran the procedure (docs/PERSONAL-DATA.md). Only anbud_redact_raw_document() writes it.';
COMMENT ON VIEW raw_documents_live IS 'The notice bodies that are still held: raw_documents without the redacted rows. A reader such as the parser reads this view, which is how it honours the redaction list.';
COMMENT ON FUNCTION raw_documents_change_guard() IS 'Refuses every UPDATE and DELETE of raw_documents, except the one transformation anbud_redact_raw_document() makes with its tombstone written and its door open for this row and this transaction.';
COMMENT ON FUNCTION raw_documents_insert_guard() IS 'Refuses an INSERT into raw_documents of a row that already carries redacted_at.';
COMMENT ON FUNCTION anbud_redact_raw_document(BIGINT, TEXT, TEXT) IS 'Erases one notice body and records a tombstone, in one transaction. Arguments: the id of the raw_documents row, a reason from the list in raw_redactions, and a request reference of the shape letters, digits and . _ : / - (a ticket id, never a name). Returns the tombstone id. SECURITY DEFINER: executable by the DBA group only. docs/PERSONAL-DATA.md is the runbook.';
