-- 040_space_tool_policies (WS10-S5a): Control-held per-tool policy for a shared
-- Space. Two layers only, and both can only TIGHTEN:
--
--   * space_ceiling: what a Space manager sets for every agent of the Space.
--   * agent:         what a Space manager sets for one agent of the Space.
--
-- A tool key is a tool name, an action key (`execute_provider_action:<operation>`)
-- or '*' (every tool of that layer without a specific row). The level is
-- allow < ask < block. An absent row means 'allow', which never loosens a default
-- gate (risk, posture, never-auto set): only a standing rule or a once decision
-- waives a prompt. Execution Core folds the merged view with its own sources by
-- max_restrictive, so no row here can loosen anything. The organisation floor
-- layer of the first design is dropped (plan 2.6 ruling: it is read from
-- capability-core when that plane delivers it).
--
-- Rows hold keys and levels only: never tool arguments or content, so they are
-- safe next to a Zero Data Retention org.
--
-- space_tool_policy_sets carries the monotonic revision of one layer (one row per
-- Space ceiling, one per Space agent). A PUT replaces the whole layer in one
-- transaction under a compare-and-set on that revision, so emptying a layer
-- still advances it and an old editor view can never be written back.
CREATE TABLE IF NOT EXISTS space_tool_policy_sets (
    space_ref       TEXT NOT NULL REFERENCES registered_spaces(space_ref) ON DELETE RESTRICT,
    layer           TEXT NOT NULL CHECK (layer IN ('space_ceiling', 'agent')),
    agent_key       TEXT NOT NULL DEFAULT '',
    org_id          TEXT NOT NULL,
    revision        BIGINT NOT NULL CHECK (revision > 0),
    updated_by      TEXT NOT NULL CHECK (char_length(btrim(updated_by)) > 0),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (space_ref, layer, agent_key),
    CHECK ((layer = 'space_ceiling' AND agent_key = '')
        OR (layer = 'agent' AND char_length(btrim(agent_key)) > 0))
);

CREATE TABLE IF NOT EXISTS space_tool_policies (
    policy_id           TEXT PRIMARY KEY,
    org_id              TEXT NOT NULL,
    layer               TEXT NOT NULL CHECK (layer IN ('space_ceiling', 'agent')),
    space_ref           TEXT NOT NULL REFERENCES registered_spaces(space_ref) ON DELETE RESTRICT,
    agent_subject_id    TEXT,
    tool_key            TEXT NOT NULL CHECK (char_length(btrim(tool_key)) BETWEEN 1 AND 200),
    level               TEXT NOT NULL CHECK (level IN ('allow', 'ask', 'block')),
    set_by_subject_id   TEXT NOT NULL CHECK (char_length(btrim(set_by_subject_id)) > 0),
    revision            BIGINT NOT NULL CHECK (revision > 0),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CHECK ((layer = 'space_ceiling' AND agent_subject_id IS NULL)
        OR (layer = 'agent' AND agent_subject_id IS NOT NULL AND char_length(btrim(agent_subject_id)) > 0))
);

CREATE UNIQUE INDEX IF NOT EXISTS space_tool_policies_key_idx
    ON space_tool_policies (space_ref, layer, COALESCE(agent_subject_id, ''), tool_key);

CREATE INDEX IF NOT EXISTS space_tool_policies_space_idx
    ON space_tool_policies (org_id, space_ref);
