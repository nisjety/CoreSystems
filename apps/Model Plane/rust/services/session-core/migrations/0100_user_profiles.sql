-- 0100_user_profiles.sql
--
-- The standing user profile (build plan EP-8 S4, decisions D7 and D8): one short text per person,
-- built deterministically from their own accepted, current `scope = 'user'` rows in agent_memory, or
-- as they last edited it. It is a new durable store of user-derived content, so:
--
--   * nothing writes it for a ZDR or waived caller (session-core refuses the write RPCs);
--   * ResetMemory deletes the row, a memory delete drops the lines that came from that memory, and the
--     organization erasure purges every row of the org (gdpr.rs);
--   * capability-core never reads it (it is not agent_memory).
--
--   content            at most 1400 characters, one line per remembered fact (also checked in code).
--   source_memory_ids  provenance: the agent_memory id behind each built line, in line order. An edit
--                      keeps the list, so "N sources deleted since you edited" can be counted.
--   origin             'built' by the builder or 'edited' by the person; a rebuild keeps an edited
--                      profile unless the person asks for it.
--   revision           increases on every change; an edit names the revision it was made against.
--   built_at           when the builder last wrote the text; NULL for a profile only ever edited.
--
-- Numbered 0100: round A reserved 0072, which 0072_space_event_log holds on this branch; round B moves
-- round A's five session-core migrations to 0100-0104 (EP-8 S4 is the first). The reversal is in
-- revert/0100_user_profiles.down.sql.

CREATE TABLE IF NOT EXISTS user_profiles (
    org_id            TEXT NOT NULL,
    user_id           TEXT NOT NULL,
    content           TEXT NOT NULL DEFAULT '',
    source_memory_ids TEXT[] NOT NULL DEFAULT '{}',
    origin            TEXT NOT NULL DEFAULT 'built' CHECK (origin IN ('built', 'edited')),
    revision          BIGINT NOT NULL DEFAULT 1,
    built_at          TIMESTAMPTZ,
    updated_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (org_id, user_id),
    CONSTRAINT user_profiles_content_len_chk CHECK (char_length(content) <= 1400)
);

-- The audit record of a profile change rides memory_control_events, content-free like every other row
-- there: 'profile_edited' (the person's own edit) and 'profile_rebuilt' (a written rebuild), with the
-- revision and a line count in `detail`, never the text. The event CHECK is a closed set, so it is
-- replaced; NOT VALID then VALIDATE, as 0051 does, so the swap does not hold a long exclusive lock.
ALTER TABLE memory_control_events DROP CONSTRAINT IF EXISTS memory_control_events_event_check;
ALTER TABLE memory_control_events
    ADD CONSTRAINT memory_control_events_event_check
    CHECK (event IN ('state_changed', 'reset', 'profile_edited', 'profile_rebuilt')) NOT VALID;
ALTER TABLE memory_control_events VALIDATE CONSTRAINT memory_control_events_event_check;
