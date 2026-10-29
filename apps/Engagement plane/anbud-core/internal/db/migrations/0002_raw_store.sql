-- 0002_raw_store.sql
-- Public Doffin corpus: search response bodies, notice bodies, one row per download
-- attempt, the run ledger and per-day sync state. No tenant column (constraint 4).
-- Runs inside the caller's transaction (migrate.go:167-175), so no BEGIN, COMMIT or ROLLBACK.
-- Applied once. migrate_concurrency_test.go drops these objects before each round.

CREATE FUNCTION raw_append_only_guard() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    RAISE EXCEPTION '% on % refused: table is append-only', TG_OP, TG_TABLE_NAME
        USING ERRCODE = 'restrict_violation';
END;
$$;

CREATE TABLE sync_runs (
    id                BIGINT      GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    mode              TEXT        NOT NULL CHECK (mode IN ('sync', 'census')),
    range_from        DATE        NOT NULL,
    range_to          DATE        NOT NULL,
    state             TEXT        NOT NULL DEFAULT 'running' CHECK (state IN (
                          'running', 'done', 'partial', 'paused', 'key_rejected',
                          'cancelled', 'interrupted', 'failed')),
    started_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    finished_at       TIMESTAMPTZ,
    retry_after_until TIMESTAMPTZ,
    last_error        TEXT        CHECK (char_length(last_error) <= 200),
    searches          INTEGER     NOT NULL DEFAULT 0 CHECK (searches >= 0),
    downloads         INTEGER     NOT NULL DEFAULT 0 CHECK (downloads >= 0),
    stored            INTEGER     NOT NULL DEFAULT 0 CHECK (stored >= 0),
    duplicates        INTEGER     NOT NULL DEFAULT 0 CHECK (duplicates >= 0),
    held_skipped      INTEGER     NOT NULL DEFAULT 0 CHECK (held_skipped >= 0),
    not_found         INTEGER     NOT NULL DEFAULT 0 CHECK (not_found >= 0),
    CHECK (range_from <= range_to),
    CHECK ((state = 'running') = (finished_at IS NULL))
);

CREATE TABLE raw_search_pages (
    id           BIGINT      GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    source       TEXT        NOT NULL CHECK (source = 'doffin'),
    query_key    TEXT        NOT NULL CHECK (char_length(query_key) BETWEEN 1 AND 300),
    page         INTEGER     NOT NULL CHECK (page >= 1),
    data_class   TEXT        NOT NULL DEFAULT 'public_notice_may_hold_contacts'
                             CHECK (data_class = 'public_notice_may_hold_contacts'),
    content_type TEXT        NOT NULL DEFAULT '' CHECK (char_length(content_type) <= 255),
    body         BYTEA       NOT NULL CHECK (octet_length(body) > 0),
    body_sha256  TEXT        NOT NULL CHECK (body_sha256 ~ '^[0-9a-f]{64}$'
                                         AND body_sha256 = encode(sha256(body), 'hex')),
    fetched_at   TIMESTAMPTZ NOT NULL,
    run_id       BIGINT      NOT NULL REFERENCES sync_runs (id),
    CONSTRAINT raw_search_pages_identity UNIQUE (source, query_key, page, body_sha256)
);

CREATE TABLE raw_documents (
    id           BIGINT      GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    source       TEXT        NOT NULL CHECK (source = 'doffin'),
    doffin_id    TEXT        NOT NULL CHECK (doffin_id ~ '^[0-9]{4}-[0-9]{4,9}$'),
    data_class   TEXT        NOT NULL DEFAULT 'public_notice_may_hold_contacts'
                             CHECK (data_class = 'public_notice_may_hold_contacts'),
    content_type TEXT        NOT NULL DEFAULT '' CHECK (char_length(content_type) <= 255),
    body         BYTEA       NOT NULL CHECK (octet_length(body) > 0),
    body_sha256  TEXT        NOT NULL CHECK (body_sha256 ~ '^[0-9a-f]{64}$'
                                         AND body_sha256 = encode(sha256(body), 'hex')),
    fetched_at   TIMESTAMPTZ NOT NULL,
    stored_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    run_id       BIGINT      NOT NULL REFERENCES sync_runs (id),
    CONSTRAINT raw_documents_identity UNIQUE (source, doffin_id, body_sha256)
);

CREATE TABLE download_attempts (
    id              BIGINT      GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    run_id          BIGINT      NOT NULL REFERENCES sync_runs (id),
    source          TEXT        NOT NULL CHECK (source = 'doffin'),
    doffin_id       TEXT        NOT NULL CHECK (doffin_id ~ '^[0-9]{4}-[0-9]{4,9}$'),
    attempted_at    TIMESTAMPTZ NOT NULL,
    outcome         TEXT        NOT NULL CHECK (outcome IN (
                        'stored', 'duplicate', 'not_found', 'invalid_id', 'http_error',
                        'empty_body', 'too_large', 'transport_error',
                        'rate_limited', 'unauthorized')),
    http_status     INTEGER     CHECK (http_status BETWEEN 100 AND 599),
    message         TEXT        CHECK (char_length(message) <= 256),
    raw_document_id BIGINT      REFERENCES raw_documents (id),
    CONSTRAINT download_attempts_raw_link CHECK (
        (outcome IN ('stored', 'duplicate')) = (raw_document_id IS NOT NULL))
);
CREATE INDEX download_attempts_by_notice ON download_attempts (source, doffin_id);

CREATE TABLE sync_days (
    source         TEXT        NOT NULL CHECK (source = 'doffin'),
    day            DATE        NOT NULL,
    state          TEXT        NOT NULL CHECK (state IN ('running', 'searched', 'done', 'capped',
                                                         'partial', 'failed', 'paused', 'interrupted')),
    error_code     TEXT        CHECK (error_code IN ('saturated', 'count_mismatch', 'count_changed',
                        'download_limit', 'not_found_grace', 'item_rejected', 'consecutive_errors',
                        'rate_limited', 'unauthorized', 'invalid_params', 'search_failed',
                        'decode_error', 'store_failed', 'cancelled', 'process_lost')),
    num_total      BIGINT      CHECK (num_total >= 0),
    num_accessible BIGINT      CHECK (num_accessible >= 0),
    held           INTEGER     NOT NULL DEFAULT 0 CHECK (held >= 0),
    stored         INTEGER     NOT NULL DEFAULT 0 CHECK (stored >= 0),
    not_found      INTEGER     NOT NULL DEFAULT 0 CHECK (not_found >= 0),
    unresolved     INTEGER     NOT NULL DEFAULT 0 CHECK (unresolved >= 0),
    attempts       INTEGER     NOT NULL DEFAULT 0 CHECK (attempts >= 0),
    last_run_id    BIGINT      NOT NULL REFERENCES sync_runs (id),
    updated_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (source, day),
    CONSTRAINT sync_days_done_adds_up CHECK (
        state <> 'done'
        OR (num_total IS NOT NULL AND num_accessible IS NOT NULL
            AND num_total = num_accessible AND error_code IS NULL AND unresolved = 0
            AND held + stored + not_found = num_accessible)),
    CONSTRAINT sync_days_capped_is_saturated CHECK (
        state <> 'capped'
        OR (num_total IS NOT NULL AND num_accessible IS NOT NULL
            AND num_total > num_accessible AND error_code = 'saturated'
            AND held = 0 AND stored = 0 AND not_found = 0 AND unresolved = 0)),
    CONSTRAINT sync_days_gap_has_code CHECK (
        state NOT IN ('partial', 'failed', 'paused', 'interrupted') OR error_code IS NOT NULL),
    CONSTRAINT sync_days_counted CHECK (
        state NOT IN ('searched', 'done', 'capped')
        OR (num_total IS NOT NULL AND num_accessible IS NOT NULL))
);

CREATE TRIGGER raw_search_pages_append_only BEFORE UPDATE OR DELETE ON raw_search_pages
    FOR EACH ROW EXECUTE FUNCTION raw_append_only_guard();
CREATE TRIGGER raw_search_pages_no_truncate BEFORE TRUNCATE ON raw_search_pages
    FOR EACH STATEMENT EXECUTE FUNCTION raw_append_only_guard();
CREATE TRIGGER raw_documents_append_only BEFORE UPDATE OR DELETE ON raw_documents
    FOR EACH ROW EXECUTE FUNCTION raw_append_only_guard();
CREATE TRIGGER raw_documents_no_truncate BEFORE TRUNCATE ON raw_documents
    FOR EACH STATEMENT EXECUTE FUNCTION raw_append_only_guard();
CREATE TRIGGER download_attempts_append_only BEFORE UPDATE OR DELETE ON download_attempts
    FOR EACH ROW EXECUTE FUNCTION raw_append_only_guard();
CREATE TRIGGER download_attempts_no_truncate BEFORE TRUNCATE ON download_attempts
    FOR EACH STATEMENT EXECUTE FUNCTION raw_append_only_guard();

COMMENT ON TABLE raw_documents IS 'Doffin notice bodies exactly as downloaded. Append-only. Each body may hold contact details of persons named in the notice. Public corpus, no tenant column.';
COMMENT ON COLUMN raw_documents.doffin_id IS 'Lookup alias, not notice identity. The parser assigns notice identifier and version later.';
COMMENT ON TABLE raw_search_pages IS 'Search response bodies exactly as returned, with content type and fetch time. Append-only.';
COMMENT ON TABLE download_attempts IS 'One row per notice download outcome. Append-only. Holds no bodies.';
COMMENT ON TABLE sync_days IS 'Mutable per-day sync state. Counts describe the latest attempt.';
