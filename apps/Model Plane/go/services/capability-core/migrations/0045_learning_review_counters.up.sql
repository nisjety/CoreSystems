-- The learning review's day counters (WS07-S10 cost controls; audit unit F-f01-22).
--
-- One row per key the review counts against: learning-review:user:<org>:<user>:<day> for an owner and
-- learning-review:org:<org>:<day> for an organization, both on the UTC day. A row holds the key, the organization it
-- belongs to, the count and the time that count expires. It holds ids and a number, never a transcript, a skill, a
-- reason or a note.
--
-- sessionreview.PgReviewBudget.Incr increments a key in one statement, so replicas share one count, and a count whose
-- window has passed starts again at one. The sweeper deletes a row a day after it expires. The org_id index serves the
-- organization erasure: its purge (internal/erasure, orgPurgeSteps) deletes an organization's rows by org_id when the
-- erasure consumer is on (CAPABILITY_CORE_ORG_ERASURE_ENABLED=true). The sweep removes every row a day after it
-- expires, whether or not an erasure has run.
--
-- Nothing reads or writes this table unless LEARNING_REVIEW_COST_CONTROLS is the literal true on capability-core, except
-- the organization erasure's purge, which deletes one organization's rows and nothing else.
--
-- RE-RUN SAFETY. capability-core re-applies every *.up.sql at every boot, so everything here is idempotent.

CREATE TABLE IF NOT EXISTS learning_review_counters (
    key        TEXT PRIMARY KEY,
    org_id     TEXT NOT NULL,
    count      BIGINT NOT NULL CHECK (count >= 0),
    expires_at TIMESTAMPTZ NOT NULL
);

CREATE INDEX IF NOT EXISTS learning_review_counters_expires_idx
    ON learning_review_counters (expires_at);

CREATE INDEX IF NOT EXISTS learning_review_counters_org_idx
    ON learning_review_counters (org_id);
