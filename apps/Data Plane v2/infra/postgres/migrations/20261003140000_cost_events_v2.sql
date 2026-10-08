-- Usage ledger v2: the columns of an exact, feature-attributed, forwardable usage row
-- (design task CR-T10).
--
-- ── Why this exists ──────────────────────────────────────────────────────────
--
-- cost_events held four numbers per event (event_type, model, count, estimated_tokens)
-- and a user. That was enough for a count of embeddings and reranks; it cannot say what
-- a paid call cost. A contextual-retrieval or graph-extraction call is a chat completion
-- whose exact prompt, completion and prompt-cache tokens the provider reports, and the
-- feature that caused it is the dimension the product's cost page, the organization's
-- background budget and cost-core's reports are built on. Until this migration those
-- calls were recorded nowhere, and the events that did reach this table never left it:
-- nothing forwarded them to cost-core.
--
-- Version 2 events (usage-ledger-rs, schema_version 2) fill the new columns; version 1
-- events (the count-only event of the producers that predate it) leave them NULL, which
-- reads as "unknown", never as a zero that would read as free. `schema_version`
-- records which a row is.
--
-- ── Zero retention ───────────────────────────────────────────────────────────
--
-- A zero-retention organization's usage is recorded: the earlier convention dropped
-- those events and made its spend invisible. A usage event holds counters only (no
-- document, no chunk, no text, no user), and the CHECK `cost_events_zdr_counters_only_chk`
-- makes the table refuse a zero-retention row that names a user, so the property does not
-- rest on every writer remembering it.
--
-- ── Forwarding ───────────────────────────────────────────────────────────────
--
-- data-orchestrator's forwarder posts each version 2 row to cost-core
-- (POST /api/v1/cost/record) and records that it did. The forward_* columns are its
-- bookkeeping, in the same row so the row is the single truth of what was forwarded:
--
--   forwarded_at         set when cost-core answered 202. NULL: not forwarded yet.
--   forward_attempts     failed attempts so far; drives the retry pause.
--   forward_next_at      earliest time to try again.
--   forward_lease_until  a worker holds the row until then (FOR UPDATE SKIP LOCKED plus
--                        this deadline), so two replicas never post one row at once.
--   forward_error        a closed, content-free class of the last failure, never free text.
--
-- Version 1 rows are never forwarded: they carry no exact numbers and no feature, and
-- cost-core would have to price them from a guess.
--
-- ── Compatibility ────────────────────────────────────────────────────────────
--
-- Every new column is nullable or has a constant default, so the ALTER takes no rewrite.
-- The running consumer writes the old seven columns and nothing else; it keeps working
-- against the migrated table, and its rows are version 1 by default. The new consumer
-- writes all of them. Deploy order: this migration, then the consumer, then producers.
--
-- cost_events DDL lives in init.sql too (the baseline for a fresh database); this file is
-- what an existing one gets. Both say the same, and tests/contract/postgres checks it.

-- ALTER TABLE needs an exclusive lock on cost_events. Do not queue behind a long
-- transaction and block every other reader and writer meanwhile: give up after 10 s and
-- let the operator retry.
SET LOCAL lock_timeout = '10s';

ALTER TABLE cost_events
    ADD COLUMN IF NOT EXISTS schema_version     SMALLINT    NOT NULL DEFAULT 1,
    ADD COLUMN IF NOT EXISTS feature            TEXT,
    ADD COLUMN IF NOT EXISTS provider_used      TEXT,
    ADD COLUMN IF NOT EXISTS input_tokens       BIGINT,
    ADD COLUMN IF NOT EXISTS output_tokens      BIGINT,
    ADD COLUMN IF NOT EXISTS cache_read_tokens  BIGINT,
    ADD COLUMN IF NOT EXISTS units              BIGINT,
    ADD COLUMN IF NOT EXISTS cost_basis         TEXT,
    ADD COLUMN IF NOT EXISTS estimated          BOOLEAN,
    ADD COLUMN IF NOT EXISTS zdr                BOOLEAN     NOT NULL DEFAULT FALSE,
    ADD COLUMN IF NOT EXISTS request_id         TEXT,
    ADD COLUMN IF NOT EXISTS forwarded_at       TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS forward_attempts   INTEGER     NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS forward_next_at    TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS forward_lease_until TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS forward_error      TEXT;

-- Version 1 rows have always been of the two kinds the table was built for; give them the
-- feature of the same name so a report by feature covers history as well as new rows.
-- Idempotent: only rows with no feature are touched, and nothing else about them changes.
UPDATE cost_events SET feature = event_type
 WHERE feature IS NULL AND schema_version = 1 AND event_type IN ('embedding', 'rerank');

-- ── Constraints ──────────────────────────────────────────────────────────────

ALTER TABLE cost_events DROP CONSTRAINT IF EXISTS cost_events_schema_version_chk;
ALTER TABLE cost_events ADD CONSTRAINT cost_events_schema_version_chk
    CHECK (schema_version IN (1, 2));

-- A version 2 row is complete: it is what is forwarded, and cost-core cannot price a row
-- that has no model, no counts or no basis.
ALTER TABLE cost_events DROP CONSTRAINT IF EXISTS cost_events_v2_complete_chk;
ALTER TABLE cost_events ADD CONSTRAINT cost_events_v2_complete_chk
    CHECK (schema_version < 2 OR (
        feature IS NOT NULL AND provider_used IS NOT NULL
        AND input_tokens IS NOT NULL AND output_tokens IS NOT NULL AND cache_read_tokens IS NOT NULL
        AND cost_basis IS NOT NULL AND estimated IS NOT NULL AND request_id IS NOT NULL));

ALTER TABLE cost_events DROP CONSTRAINT IF EXISTS cost_events_counts_chk;
ALTER TABLE cost_events ADD CONSTRAINT cost_events_counts_chk
    CHECK (count >= 0 AND estimated_tokens >= 0
        AND (input_tokens IS NULL OR input_tokens >= 0)
        AND (output_tokens IS NULL OR output_tokens >= 0)
        AND (cache_read_tokens IS NULL OR cache_read_tokens >= 0)
        AND (units IS NULL OR units >= 0));

-- The cost basis is a closed vocabulary: `metered` (the provider reported the numbers) or
-- `estimated` (this system did, or the provider reported none). `estimated` mirrors it so
-- a query does not have to compare strings.
ALTER TABLE cost_events DROP CONSTRAINT IF EXISTS cost_events_cost_basis_chk;
ALTER TABLE cost_events ADD CONSTRAINT cost_events_cost_basis_chk
    CHECK (cost_basis IS NULL OR (cost_basis IN ('metered', 'estimated') AND estimated IS NOT NULL AND estimated = (cost_basis = 'estimated')));

-- The feature is forwarded verbatim and keys cost-core's reports: lowercase letters, digits
-- and hyphens, as cost-core accepts. Which features a producer may report is the verifier's
-- business (data-orchestrator internal/cost/verifier.go), not a list to migrate.
ALTER TABLE cost_events DROP CONSTRAINT IF EXISTS cost_events_feature_chk;
ALTER TABLE cost_events ADD CONSTRAINT cost_events_feature_chk
    CHECK (feature IS NULL OR feature ~ '^[a-z][a-z0-9-]{0,62}$');

-- Counters only. A zero-retention row never names a user.
ALTER TABLE cost_events DROP CONSTRAINT IF EXISTS cost_events_zdr_counters_only_chk;
ALTER TABLE cost_events ADD CONSTRAINT cost_events_zdr_counters_only_chk
    CHECK (NOT zdr OR user_id IS NULL);

-- Only a version 2 row is forwarded, and the failure class is one of a closed, content-free
-- set (never the text of an error: that is where an organization's data leaks into a table).
ALTER TABLE cost_events DROP CONSTRAINT IF EXISTS cost_events_forward_state_chk;
ALTER TABLE cost_events ADD CONSTRAINT cost_events_forward_state_chk
    CHECK ((forwarded_at IS NULL OR schema_version >= 2)
        AND forward_attempts >= 0
        AND (forward_error IS NULL OR forward_error IN ('rejected', 'unauthorized', 'unavailable', 'token_unavailable')));

-- ── Indexes ──────────────────────────────────────────────────────────────────

-- The forwarder's queue: version 2 rows not yet forwarded. Small by construction (a row
-- leaves the index when it is forwarded), so the claim never scans the ledger.
CREATE INDEX IF NOT EXISTS idx_cost_events_forward_queue
    ON cost_events (id)
    WHERE forwarded_at IS NULL AND schema_version >= 2;

-- Spend by feature for one organization, newest first.
CREATE INDEX IF NOT EXISTS idx_cost_events_org_feature_created
    ON cost_events (org_id, feature, created_at DESC)
    WHERE feature IS NOT NULL;

-- ── Comments ─────────────────────────────────────────────────────────────────

COMMENT ON COLUMN cost_events.schema_version IS
    '1 = the count-only event (no exact numbers, never forwarded); 2 = the usage ledger event (usage-ledger-rs).';
COMMENT ON COLUMN cost_events.feature IS
    'What caused the spend: embedding | contextual-retrieval | graph-extraction | graph-community-summary | rerank | web-rerank. Forwarded verbatim to cost-core, which reports and budgets by it.';
COMMENT ON COLUMN cost_events.provider_used IS
    'The provider that served the calls (InferResponse.provider_used), never the one that was asked for.';
COMMENT ON COLUMN cost_events.input_tokens IS
    'Exact prompt tokens the provider reported, summed over the event''s calls; includes the cache reads. NULL on a version 1 row: unknown, not zero.';
COMMENT ON COLUMN cost_events.output_tokens IS
    'Exact completion tokens the provider reported. NULL on a version 1 row.';
COMMENT ON COLUMN cost_events.cache_read_tokens IS
    'Tokens served from the provider''s prompt cache: detail of input_tokens, not an addition to it.';
COMMENT ON COLUMN cost_events.units IS
    'Billed units that are not tokens (a reranker''s search units). NULL when the event has none.';
COMMENT ON COLUMN cost_events.cost_basis IS
    'metered = the provider reported the numbers; estimated = this system estimated them (an embedding call returns vectors only) or the provider reported none. NULL on a version 1 row.';
COMMENT ON COLUMN cost_events.estimated IS
    'True when cost_basis is estimated: the tokens of this row are not provider-reported.';
COMMENT ON COLUMN cost_events.zdr IS
    'The organization was zero-retention. The row holds counters only: it never names a user (cost_events_zdr_counters_only_chk).';
COMMENT ON COLUMN cost_events.request_id IS
    'sha256 (lowercase hex) of the sorted provider request ids of the event''s calls. Content-free correlation with inference-core''s logs; the idempotency key is derived from it.';
COMMENT ON COLUMN cost_events.estimated_tokens IS
    'Version 1: bytes / 4, an estimate. Version 2: input_tokens + output_tokens (exact unless estimated). Kept so the Data Plane''s own summary, which has always summed this column, still means tokens.';
COMMENT ON COLUMN cost_events.forwarded_at IS
    'When cost-core accepted the row (202). NULL: waiting to be forwarded (version 2 rows only).';
COMMENT ON COLUMN cost_events.forward_error IS
    'Class of the last failed forward: rejected | unauthorized | unavailable | token_unavailable. Never free text.';
COMMENT ON INDEX idx_cost_events_forward_queue IS
    'The forwarder''s queue: version 2 rows not yet forwarded.';
