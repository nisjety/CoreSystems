-- Application owns the product participant set; Control records only an
-- independently verified, versioned commitment after every recipient passes
-- current Space and organization membership checks. Historical rows support
-- visibility-safe replay/fork authorization without treating all Space
-- members as conversation recipients.

BEGIN;

CREATE TABLE IF NOT EXISTS space_recipient_audiences (
    space_ref       TEXT NOT NULL REFERENCES registered_spaces(space_ref) ON DELETE CASCADE,
    revision        BIGINT NOT NULL,
    audience_ref    TEXT NOT NULL,
    audience_hash   TEXT NOT NULL,
    recipient_count INTEGER NOT NULL,
    registered_at   TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (space_ref, revision),
    UNIQUE (audience_ref),
    CONSTRAINT space_recipient_audiences_revision_chk CHECK (revision > 0),
    CONSTRAINT space_recipient_audiences_count_chk CHECK (recipient_count > 0),
    CONSTRAINT space_recipient_audiences_hash_chk CHECK (audience_hash LIKE 'sha256:%')
);

CREATE TABLE IF NOT EXISTS space_recipient_audience_members (
    space_ref  TEXT NOT NULL,
    revision   BIGINT NOT NULL,
    subject_id TEXT NOT NULL,
    PRIMARY KEY (space_ref, revision, subject_id),
    FOREIGN KEY (space_ref, revision)
        REFERENCES space_recipient_audiences(space_ref, revision) ON DELETE CASCADE,
    CONSTRAINT space_recipient_audience_members_subject_chk CHECK (char_length(btrim(subject_id)) > 0)
);

CREATE INDEX IF NOT EXISTS idx_space_recipient_audiences_current
    ON space_recipient_audiences (space_ref, revision DESC);

COMMIT;
