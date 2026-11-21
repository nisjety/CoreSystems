-- 0135: validate the four agent_memory scope constraints where no row violates them (EP-8 S6 and S10, finding F-f08-02).
--
-- 0103 added agent_memory_scope_chk, agent_memory_scope_id_chk, agent_memory_space_audience_chk and
-- agent_memory_source_label_chk NOT VALID: each holds for every row written since, and none scanned the rows that
-- existed then. Each one is validated here on its own, and only when no row breaks it. The check counts the rows
-- where the constraint's own predicate is FALSE: a CHECK passes on NULL, so an unknown value is not a violation.
-- It reads no value; the count is the only thing it reports.
--
-- A deployment that still holds a violating legacy row keeps that constraint NOT VALID. A NOTICE says so, with the
-- constraint name and the row count, and the migration does not fail. Nothing is deleted, rewritten or moved, and
-- 0103 is not edited (applied migrations are checksummed).
--
-- Validating a constraint takes SHARE UPDATE EXCLUSIVE, which does not block reads or writes of agent_memory. A
-- constraint validated by an earlier run is skipped, so applying this file twice changes nothing.
--
-- SWITCH: none. It runs once, changes no row and cannot fail a deployment: a constraint is validated only when its
-- check finds nothing to refuse, and any other stays NOT VALID, which is enforced for every new row, as it is today.
-- ZDR: no content is read or returned. The agent_memory row of docs/ZDR.md ("Agent memory (agent_memory scope='agent',
-- EP-8 S10)") records what these rows hold.
-- The reversal is in revert/0135_validate_agent_memory_scope_constraints.down.sql.

-- agent_memory_scope_chk: the scope is a closed set of kinds.
DO $$
BEGIN
    IF EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conname = 'agent_memory_scope_chk' AND conrelid = 'agent_memory'::regclass AND NOT convalidated
    ) THEN
        IF NOT EXISTS (
            SELECT 1 FROM agent_memory
            WHERE (scope IN ('run', 'thread', 'session', 'workspace', 'user', 'org', 'global', 'space', 'agent')) IS FALSE
        ) THEN
            ALTER TABLE agent_memory VALIDATE CONSTRAINT agent_memory_scope_chk;
        ELSE
            RAISE NOTICE 'agent_memory_scope_chk left NOT VALID: % violating rows', (
                SELECT count(*) FROM agent_memory
                WHERE (scope IN ('run', 'thread', 'session', 'workspace', 'user', 'org', 'global', 'space', 'agent')) IS FALSE
            );
        END IF;
    END IF;
END $$;

-- agent_memory_scope_id_chk: only the two instance kinds carry a scope_id.
DO $$
BEGIN
    IF EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conname = 'agent_memory_scope_id_chk' AND conrelid = 'agent_memory'::regclass AND NOT convalidated
    ) THEN
        IF NOT EXISTS (
            SELECT 1 FROM agent_memory
            WHERE ((scope IN ('space', 'agent')) = (scope_id IS NOT NULL AND scope_id <> '')) IS FALSE
        ) THEN
            ALTER TABLE agent_memory VALIDATE CONSTRAINT agent_memory_scope_id_chk;
        ELSE
            RAISE NOTICE 'agent_memory_scope_id_chk left NOT VALID: % violating rows', (
                SELECT count(*) FROM agent_memory
                WHERE ((scope IN ('space', 'agent')) = (scope_id IS NOT NULL AND scope_id <> '')) IS FALSE
            );
        END IF;
    END IF;
END $$;

-- agent_memory_space_audience_chk: a Space row carries the audience it was said to.
DO $$
BEGIN
    IF EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conname = 'agent_memory_space_audience_chk' AND conrelid = 'agent_memory'::regclass AND NOT convalidated
    ) THEN
        IF NOT EXISTS (
            SELECT 1 FROM agent_memory
            WHERE (scope <> 'space' OR (audience_hash IS NOT NULL AND audience_hash <> '' AND audience_revision IS NOT NULL)) IS FALSE
        ) THEN
            ALTER TABLE agent_memory VALIDATE CONSTRAINT agent_memory_space_audience_chk;
        ELSE
            RAISE NOTICE 'agent_memory_space_audience_chk left NOT VALID: % violating rows', (
                SELECT count(*) FROM agent_memory
                WHERE (scope <> 'space' OR (audience_hash IS NOT NULL AND audience_hash <> '' AND audience_revision IS NOT NULL)) IS FALSE
            );
        END IF;
    END IF;
END $$;

-- agent_memory_source_label_chk: source_label is a closed vocabulary.
DO $$
BEGIN
    IF EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conname = 'agent_memory_source_label_chk' AND conrelid = 'agent_memory'::regclass AND NOT convalidated
    ) THEN
        IF NOT EXISTS (
            SELECT 1 FROM agent_memory
            WHERE (source_label IS NULL OR source_label IN ('room_statement', 'member_save', 'agent_memory')) IS FALSE
        ) THEN
            ALTER TABLE agent_memory VALIDATE CONSTRAINT agent_memory_source_label_chk;
        ELSE
            RAISE NOTICE 'agent_memory_source_label_chk left NOT VALID: % violating rows', (
                SELECT count(*) FROM agent_memory
                WHERE (source_label IS NULL OR source_label IN ('room_statement', 'member_save', 'agent_memory')) IS FALSE
            );
        END IF;
    END IF;
END $$;
