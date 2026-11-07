-- 0108: immutable skill versions with digests (Wave 3 EP-11 S2; renumbered from 0096 at the W3-20 close so it applies after 0107).
--
-- Every accepted change to a skill's text gets a numbered version row that is never changed
-- afterwards, so "an edit silently changes every run" becomes a visible new version, and a run can
-- later pin the exact text it used (EP-11 S3). skill_revisions (0064) is not that store: it keeps
-- only the newest 50 revisions, truncates large snapshots, and undo writes a snapshot back into the
-- live row.
--
-- Semantics (EP-11 D2, owner 2026-11-04):
--   * A version is cut when the digest of an ACCEPTED row changes. A proposal has version 0 until
--     its owner accepts it (accept = v1). Toggling `enabled`, sharing, or a rejected row's edit cuts
--     nothing; a rejected row that is accepted again with the same text keeps its version.
--   * The digest is sha256 (lowercase hex) of the UTF-8 text of agent_skill_version_body(row):
--     name, description, content, trigger_keywords, trigger_file_patterns, tool_restrictions,
--     kind, settings and bundle_sha256. kind and settings are the constants 'skill' and '{}' until
--     EP-11 S6 adds the columns and replaces the body function with the same defaults, so no
--     existing digest changes. jsonb's text output orders keys deterministically.
--   * Version rows are never updated. A delete is allowed only with the skill's own delete (this
--     file's AFTER DELETE path) and in erasure (gdpr.rs), both under mp.version_erasure = 'on'.
--   * A restore writes the old body as a NEW version; nothing re-points to an old one.
--
-- Writers (EP-11 D10): session-core's RPCs and capability-core's registry SQL both write
-- agent_skills on this database, so the triggers are the single writer of version, digest and
-- agent_skill_versions. Neither service names version or digest in a write; whatever a statement
-- sets for them is overwritten by the BEFORE trigger. The actor comes from the same
-- transaction-local settings the revision trigger reads (0064: mp.actor_kind, mp.actor_id).
--
-- Trigger order on agent_skills: BEFORE (bump) sets version and digest; the AFTER triggers
-- (revision, version) then see the final row. The revision snapshot names its own fields and
-- leaves out version, digest and bundle_sha256, so a bump does not write a second revision.
--
-- Content-bearing (skill text is in `body`), derived only from skills the org already keeps;
-- erased with the skill, with the organization (gdpr.rs), and by owner through owner_user_id
-- (register D1-643: the per-user path is not built here). Zero Data Retention: a skill is org
-- configuration, not conversation content; ZDR credentials cannot write agent_skills at either
-- writer.

ALTER TABLE agent_skills
    ADD COLUMN IF NOT EXISTS version INTEGER NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS digest TEXT NOT NULL DEFAULT '',
    ADD COLUMN IF NOT EXISTS bundle_sha256 TEXT NOT NULL DEFAULT '';

CREATE TABLE IF NOT EXISTS agent_skill_versions (
    org_id         TEXT NOT NULL,
    skill_id       TEXT NOT NULL,
    version        INTEGER NOT NULL CHECK (version >= 1),
    digest         TEXT NOT NULL CHECK (digest ~ '^[0-9a-f]{64}$'),
    body           JSONB NOT NULL,
    bundle_sha256  TEXT NOT NULL DEFAULT '',
    scope          TEXT NOT NULL,
    owner_user_id  TEXT NOT NULL DEFAULT '',
    actor_kind     TEXT NOT NULL
        CHECK (actor_kind IN ('user', 'service', 'system', 'unattributed')),
    actor_id       TEXT NOT NULL DEFAULT '',
    created_at     TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY (skill_id, version)
);

-- Erasure by owner (and the org purge) reads this.
CREATE INDEX IF NOT EXISTS agent_skill_versions_org_owner_idx
    ON agent_skill_versions (org_id, owner_user_id);

-- The versioned fields of a skill, as one JSON object. The field list is pinned by the
-- session-core test `digest_field_list_is_pinned`; change it only with a migration that says
-- which digests move.
CREATE OR REPLACE FUNCTION agent_skill_version_body(s agent_skills) RETURNS JSONB AS $$
    SELECT jsonb_build_object(
        'name', s.name,
        'description', s.description,
        'content', s.content,
        'trigger_keywords', s.trigger_keywords,
        'trigger_file_patterns', s.trigger_file_patterns,
        'tool_restrictions', s.tool_restrictions,
        'kind', 'skill'::text,
        'settings', '{}'::jsonb,
        'bundle_sha256', s.bundle_sha256
    )
$$ LANGUAGE sql STABLE;

CREATE OR REPLACE FUNCTION agent_skill_digest(s agent_skills) RETURNS TEXT AS $$
    SELECT encode(sha256(convert_to(agent_skill_version_body(s)::text, 'UTF8')), 'hex')
$$ LANGUAGE sql STABLE;

-- BEFORE: the only place version and digest are set. A writer's own values are discarded first,
-- so neither writer can forge a version or skip one.
CREATE OR REPLACE FUNCTION agent_skills_version_bump_fn() RETURNS TRIGGER AS $$
DECLARE
    next_digest TEXT;
BEGIN
    IF TG_OP = 'UPDATE' THEN
        NEW.version := OLD.version;
        NEW.digest := OLD.digest;
    ELSE
        NEW.version := 0;
        NEW.digest := '';
    END IF;
    IF NEW.review_state = 'accepted' THEN
        next_digest := agent_skill_digest(NEW);
        IF next_digest IS DISTINCT FROM NEW.digest THEN
            NEW.digest := next_digest;
            NEW.version := NEW.version + 1;
        END IF;
    END IF;
    RETURN NEW;
END
$$ LANGUAGE plpgsql;

CREATE OR REPLACE TRIGGER agent_skills_version_bump_trg
    BEFORE INSERT OR UPDATE ON agent_skills
    FOR EACH ROW EXECUTE FUNCTION agent_skills_version_bump_fn();

-- AFTER: write the version row the BEFORE trigger cut, or remove a deleted skill's versions.
CREATE OR REPLACE FUNCTION agent_skills_version_fn() RETURNS TRIGGER AS $$
DECLARE
    kind        TEXT;
    prior       TEXT;
BEGIN
    IF TG_OP = 'DELETE' THEN
        -- The skill's own delete is one of the two deletes D2 allows. The setting is put back at
        -- once, so the rest of the transaction cannot delete a version through it.
        prior := COALESCE(current_setting('mp.version_erasure', true), '');
        PERFORM set_config('mp.version_erasure', 'on', true);
        DELETE FROM agent_skill_versions WHERE skill_id = OLD.id;
        PERFORM set_config('mp.version_erasure', prior, true);
        RETURN NULL;
    END IF;

    IF NEW.version <= (CASE WHEN TG_OP = 'UPDATE' THEN OLD.version ELSE 0 END) THEN
        RETURN NULL;
    END IF;

    kind := nullif(current_setting('mp.actor_kind', true), '');
    IF kind IS NULL OR kind NOT IN ('user', 'service', 'system') THEN
        kind := 'unattributed';
    END IF;

    INSERT INTO agent_skill_versions
        (org_id, skill_id, version, digest, body, bundle_sha256, scope, owner_user_id,
         actor_kind, actor_id)
    VALUES
        (NEW.org_id, NEW.id, NEW.version, NEW.digest, agent_skill_version_body(NEW),
         NEW.bundle_sha256, NEW.scope, NEW.owner_user_id, kind,
         left(COALESCE(current_setting('mp.actor_id', true), ''), 128));
    RETURN NULL;
END
$$ LANGUAGE plpgsql;

CREATE OR REPLACE TRIGGER agent_skills_version_trg
    AFTER INSERT OR UPDATE OR DELETE ON agent_skills
    FOR EACH ROW EXECUTE FUNCTION agent_skills_version_fn();

-- A version row is never updated, and deleted only under mp.version_erasure = 'on'.
CREATE OR REPLACE FUNCTION agent_skill_versions_immutable_fn() RETURNS TRIGGER AS $$
BEGIN
    IF TG_OP = 'DELETE' AND current_setting('mp.version_erasure', true) = 'on' THEN
        RETURN OLD;
    END IF;
    RAISE EXCEPTION 'skill_version_immutable: a skill version is never changed, and deleted only with its skill or in erasure'
        USING ERRCODE = 'object_not_in_prerequisite_state';
END
$$ LANGUAGE plpgsql;

CREATE OR REPLACE TRIGGER agent_skill_versions_immutable_trg
    BEFORE UPDATE OR DELETE ON agent_skill_versions
    FOR EACH ROW EXECUTE FUNCTION agent_skill_versions_immutable_fn();

-- Backfill: every accepted skill gets v1, attributed to the system. Setting digest fires the BEFORE
-- trigger, which computes the real digest; the revision trigger sees an unchanged snapshot and
-- records nothing. sqlx runs this file once in one transaction; `version = 0` keeps a re-run inert.
SELECT set_config('mp.actor_kind', 'system', true), set_config('mp.actor_id', 'migration:0108', true);
UPDATE agent_skills SET digest = '' WHERE review_state = 'accepted' AND version = 0;
SELECT set_config('mp.actor_kind', '', true), set_config('mp.actor_id', '', true);
