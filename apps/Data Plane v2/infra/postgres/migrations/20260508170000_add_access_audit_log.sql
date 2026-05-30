-- Wave 3 §15-E — admin/user access audit log.
--
-- One row per authenticated request that hits a Data Plane v2 endpoint.
-- Populated by the `access_audit` middleware AFTER the response so we can
-- record status + latency + the document IDs the caller actually saw.
--
-- This is the forensics surface: who saw which doc at what time, was the
-- request authorized, did it succeed. Retained 90 days by default
-- (configurable); compliance teams may export to long-term storage.

CREATE TABLE IF NOT EXISTS access_audit_log (
    id                BIGSERIAL    PRIMARY KEY,
    request_id        TEXT         NOT NULL,
    user_id           TEXT,                                -- nullable: API-key calls
    org_id            TEXT         NOT NULL,
    endpoint          TEXT         NOT NULL,               -- e.g. "POST /v1/retrieve"
    http_status       INTEGER      NOT NULL,
    latency_ms        INTEGER      NOT NULL DEFAULT 0,
    auth_method       TEXT         NOT NULL,               -- "api_key" | "jwt" | "anonymous"
    document_ids      TEXT[]       NOT NULL DEFAULT '{}',  -- the docs the caller saw
    cause             TEXT,                                -- "ok" | "denied:no_membership" | "denied:no_acl" | "error:..."
    created_at        TIMESTAMPTZ  NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_access_audit_org_created
    ON access_audit_log (org_id, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_access_audit_user_created
    ON access_audit_log (user_id, created_at DESC)
    WHERE user_id IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_access_audit_cause
    ON access_audit_log (cause)
    WHERE cause <> 'ok';
