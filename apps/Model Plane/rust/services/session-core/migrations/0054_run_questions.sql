-- Run questions (build plan WS14 S3).
--
-- A run may put a question to its owner. A question is its own pause kind with its
-- own table, not an approval: an approval is an authority grant, and an answer is
-- information that must never read as one.
--
--   state      requested -> answered | skipped | expired | cancelled. Only a
--              requested question moves, and only once (compare-and-set).
--   delivery   next_turn: the answer is the owner's next message (ships first).
--              resume: the answer continues the same run (a later slice).
--   spec       the question's items, as the shared mp-contracts user_question
--              shape. Model-authored prompt text, kept like a plan or a message.
--   answers    the owner's answer to each item; NULL until answered.
--   answers_digest   lets an identical retry be told from a different second answer.
--
-- Only a persistent run has questions: a zero-retention or waived run refuses to
-- create one. The rows are erased with their run (ON DELETE CASCADE), which is how
-- an organization purge reaches them. The reversal is in
-- revert/0054_run_questions.down.sql.

CREATE TABLE IF NOT EXISTS run_questions (
    id TEXT PRIMARY KEY
        CHECK (id LIKE 'q\_%'),
    run_id TEXT NOT NULL
        REFERENCES runs(id) ON DELETE CASCADE,
    thread_id TEXT NOT NULL
        CHECK (length(thread_id) BETWEEN 1 AND 256),
    step_id TEXT NOT NULL DEFAULT ''
        CHECK (length(step_id) <= 256),
    org_id TEXT NOT NULL
        CHECK (length(org_id) BETWEEN 1 AND 128),
    user_id TEXT NOT NULL
        CHECK (length(user_id) BETWEEN 1 AND 128),
    state TEXT NOT NULL DEFAULT 'requested'
        CHECK (state IN ('requested', 'answered', 'skipped', 'expired', 'cancelled')),
    delivery TEXT NOT NULL DEFAULT 'next_turn'
        CHECK (delivery IN ('next_turn', 'resume')),
    spec JSONB NOT NULL,
    answers JSONB,
    answers_digest TEXT
        CHECK (answers_digest IS NULL OR length(answers_digest) = 64),
    answered_message_id TEXT NOT NULL DEFAULT ''
        CHECK (length(answered_message_id) <= 256),
    idempotency_key TEXT NOT NULL DEFAULT ''
        CHECK (length(idempotency_key) <= 256),
    requested_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at TIMESTAMPTZ NOT NULL,
    answered_at TIMESTAMPTZ,
    -- An answer and its time come together, and only an answer has them.
    CONSTRAINT run_questions_answer_shape CHECK (
        (state IN ('answered', 'skipped'))
        = (answered_at IS NOT NULL AND answers IS NOT NULL AND answers_digest IS NOT NULL)
    )
);

CREATE INDEX IF NOT EXISTS run_questions_run_idx
    ON run_questions (run_id, requested_at, id);

-- What is waiting on a person: read by the attention surfaces and the expiry sweep.
CREATE INDEX IF NOT EXISTS run_questions_pending_idx
    ON run_questions (org_id, user_id, expires_at)
    WHERE state = 'requested';

-- A retry of the same ask (the caller keys it `{run}:{step}`) resolves to one row.
CREATE UNIQUE INDEX IF NOT EXISTS run_questions_idempotency_idx
    ON run_questions (org_id, idempotency_key)
    WHERE idempotency_key <> '';
