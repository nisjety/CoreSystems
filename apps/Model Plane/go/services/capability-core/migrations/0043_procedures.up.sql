-- 0039_procedures.up.sql
--
-- Saved browser recipes ("procedures"): the durable, versioned, owner-scoped store behind
-- Quarry's BrowserProcedure v2 (plan 3.4 E4.2 step 3, Q4-E4.2-3a; decision B2(b): capability-core
-- owns the saved-recipe store with its own migration). EP-11's agent_skill_versions is not the
-- store (prompt-text kinds, trigger-matched injection, no tags); only its immutability and digest
-- pattern (session-core 0108) is reused here.
--
-- Three tables:
--   procedures          one row per recipe: the name, the owner, and which version is the newest.
--   procedure_versions  one immutable row per version: the validated BrowserProcedure v2 body, its
--                       sha256 digest, and the step count and slot names read from it.
--   procedure_tags      a mutable name -> version pointer per recipe (for example "stable").
--
-- Semantics:
--   * Scope is 'user' only. A recipe is visible to its owner and nobody else; sharing is ADR 8 and
--     is not built, so the CHECK admits no other value yet.
--   * A version row is never updated. A delete is allowed only (a) under
--     mp.version_erasure = 'on' (the setting session-core's gdpr.rs sets, same as 0108) or (b) as
--     the cascade of deleting the recipe itself. The cascade is recognised by the parent row no
--     longer being visible to the trigger: the referential action runs after the parent delete has
--     been made visible to the statement, a direct delete of one version does not.
--   * The digest is sha256 (lowercase hex) of the body re-marshalled with sorted object keys. The
--     Go store (internal/procedures) computes and checks it; the CHECK here pins the shape.
--   * The body holds actions and locators only. Typed values are never stored: every typed step
--     names a slot and carries empty text, and the store refuses a body with an approval_grant_id
--     or a [redacted marker (internal/procedures Validate). ZDR: a recipe is org-owned
--     configuration, not conversation content; the refusal for ZDR callers sits at the gateway.
--   * Content-bearing (step locators and URLs). Erased with its owner or organization through the
--     erasure setting above; the per-user erasure path is not built in this migration.
--
-- RE-RUN SAFETY. capability-core re-applies every *.up.sql at every boot (docker-entrypoint.sh,
-- psql, ON_ERROR_STOP), so everything here is idempotent: IF NOT EXISTS for tables and indexes,
-- CREATE OR REPLACE for functions and triggers. No existing row is read or changed.
--
-- Dark: nothing reads these tables until the PROCEDURES_API switch (Q4-E4.2-3b) is turned on.
-- Roll back with 0039_procedures.down.sql (destroys every saved recipe).

CREATE TABLE IF NOT EXISTS procedures (
    id             TEXT PRIMARY KEY DEFAULT ('proc_' || gen_random_uuid()::text)
        CHECK (id ~ '^proc_[0-9a-f-]{36}$'),
    org_id         TEXT NOT NULL CHECK (org_id <> ''),
    owner_user_id  TEXT NOT NULL CHECK (owner_user_id <> ''),
    scope          TEXT NOT NULL DEFAULT 'user' CHECK (scope = 'user'),
    name           TEXT NOT NULL CHECK (char_length(name) BETWEEN 1 AND 120),
    description    TEXT NOT NULL DEFAULT '' CHECK (char_length(description) <= 500),
    source_run_id  TEXT NOT NULL DEFAULT '' CHECK (char_length(source_run_id) <= 128),
    latest_version INTEGER NOT NULL DEFAULT 1 CHECK (latest_version >= 1),
    created_at     TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    updated_at     TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp()
);

-- Every read is "my recipes": owner-scoped, newest first, keyset on (created_at, id).
CREATE INDEX IF NOT EXISTS procedures_owner_idx
    ON procedures (org_id, owner_user_id, created_at DESC, id DESC);

CREATE TABLE IF NOT EXISTS procedure_versions (
    procedure_id TEXT NOT NULL REFERENCES procedures (id) ON DELETE CASCADE,
    org_id       TEXT NOT NULL CHECK (org_id <> ''),
    version      INTEGER NOT NULL CHECK (version >= 1),
    digest       CHAR(64) NOT NULL CHECK (digest ~ '^[0-9a-f]{64}$'),
    body         JSONB NOT NULL CHECK (jsonb_typeof(body) = 'object'),
    step_count   INTEGER NOT NULL CHECK (step_count BETWEEN 1 AND 100),
    slot_names   TEXT[] NOT NULL DEFAULT '{}',
    created_by   TEXT NOT NULL DEFAULT '',
    created_at   TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY (procedure_id, version)
);

-- Erasure by organization reads this.
CREATE INDEX IF NOT EXISTS procedure_versions_org_idx
    ON procedure_versions (org_id, procedure_id);

CREATE TABLE IF NOT EXISTS procedure_tags (
    procedure_id TEXT NOT NULL REFERENCES procedures (id) ON DELETE CASCADE,
    tag          TEXT NOT NULL CHECK (tag ~ '^[a-z0-9._-]{1,32}$'),
    version      INTEGER NOT NULL CHECK (version >= 1),
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY (procedure_id, tag),
    -- CASCADE, not NO ACTION: deleting a recipe cascades to its versions and its tags in either
    -- order, and a NO ACTION check could see the tags still there when the versions go first.
    FOREIGN KEY (procedure_id, version) REFERENCES procedure_versions (procedure_id, version)
        ON DELETE CASCADE
);

-- A version row is never updated, and deleted only in erasure or with its recipe.
CREATE OR REPLACE FUNCTION procedure_versions_immutable_fn() RETURNS TRIGGER AS $$
BEGIN
    IF TG_OP = 'DELETE' THEN
        IF current_setting('mp.version_erasure', true) = 'on' THEN
            RETURN OLD;
        END IF;
        -- The cascade of deleting the recipe: the parent row is already gone.
        IF NOT EXISTS (SELECT 1 FROM procedures WHERE id = OLD.procedure_id) THEN
            RETURN OLD;
        END IF;
    END IF;
    RAISE EXCEPTION 'procedure_version_immutable: a procedure version is never changed, and deleted only with its procedure or in erasure'
        USING ERRCODE = 'object_not_in_prerequisite_state';
END
$$ LANGUAGE plpgsql;

CREATE OR REPLACE TRIGGER procedure_versions_immutable_trg
    BEFORE UPDATE OR DELETE ON procedure_versions
    FOR EACH ROW EXECUTE FUNCTION procedure_versions_immutable_fn();
