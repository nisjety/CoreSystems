-- Who authored a run question (Wave 3 EP-4 S5).
--
-- Until now every stored question read as the model's. An MCP server may later send a form
-- through a run (EP-4 S7, elicitation), and the card must then say which server asked, never
-- dress a server's question as the assistant's. This migration adds the two facts that say so.
--
--   origin        'model' (the run's own model, every row before this one) or 'mcp_elicitation'
--                 (a form an MCP server sent). session-core accepts 'mcp_elicitation' only from
--                 the execution-core service principal.
--   origin_label  the asking server's display name, at most 80 characters; empty exactly when
--                 the origin is 'model' (the CHECK).
--
-- Additive: an existing row takes the defaults and reads 'model' with an empty label, which is
-- what it was. No new store and no new retention: the columns live on a table only persistent
-- runs write, and they are erased with their run (ON DELETE CASCADE from runs), as before.
-- The reversal is in revert/0105_run_question_origin.down.sql.

ALTER TABLE run_questions
    ADD COLUMN IF NOT EXISTS origin TEXT NOT NULL DEFAULT 'model'
        CHECK (origin IN ('model', 'mcp_elicitation'));

ALTER TABLE run_questions
    ADD COLUMN IF NOT EXISTS origin_label TEXT NOT NULL DEFAULT ''
        CHECK (length(origin_label) <= 80);

ALTER TABLE run_questions
    DROP CONSTRAINT IF EXISTS run_questions_origin_label_chk;

ALTER TABLE run_questions
    ADD CONSTRAINT run_questions_origin_label_chk
    CHECK ((origin = 'model') = (origin_label = ''));
