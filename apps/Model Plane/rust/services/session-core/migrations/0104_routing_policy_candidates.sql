-- 0104_routing_policy_candidates.sql
--
-- RoutingPolicy candidates (build plan EP-14 S10). A policy change is proposed as a candidate, reviewed
-- against an offline diff, evaluated live, and only then promoted to the live singleton row.
--
--   routing_policy_candidates     one row per proposed policy. `base_version` is the live version the
--                                 candidate was proposed against; promotion writes the live row only while
--                                 that version is still live. `state` moves proposed -> evaluated (or
--                                 rejected), then evaluated -> promoted, and any pending state can become
--                                 superseded. The config, the diff and the live evaluation are platform
--                                 configuration: no org or user content is stored. `proposed_by` and
--                                 `decided_by` are identities, as `routing_policy.updated_by` already is.
--   routing_policy.candidate_id   the candidate a live version was promoted from; empty for a direct write.
--
-- Additive and idempotent: an existing singleton row keeps its candidate_id ''. The reversal is in
-- revert/0104_routing_policy_candidates.down.sql.

CREATE TABLE IF NOT EXISTS routing_policy_candidates (
    id            TEXT PRIMARY KEY CHECK (length(id) > 0),
    config        JSONB NOT NULL,
    base_version  BIGINT NOT NULL CHECK (base_version >= 0),
    proposed_by   TEXT NOT NULL DEFAULT '',
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    state         TEXT NOT NULL CHECK (state IN ('proposed', 'evaluated', 'rejected', 'promoted', 'superseded')),
    diff          JSONB NOT NULL DEFAULT '{}',
    live_eval     JSONB NOT NULL DEFAULT '{}',
    waiver_reason TEXT NOT NULL DEFAULT '',
    decided_by    TEXT NOT NULL DEFAULT '',
    decided_at    TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS routing_policy_candidates_created_idx
    ON routing_policy_candidates (created_at DESC, id DESC);

CREATE INDEX IF NOT EXISTS routing_policy_candidates_pending_idx
    ON routing_policy_candidates (state)
    WHERE state IN ('proposed', 'evaluated');

ALTER TABLE routing_policy
    ADD COLUMN IF NOT EXISTS candidate_id TEXT NOT NULL DEFAULT '';
