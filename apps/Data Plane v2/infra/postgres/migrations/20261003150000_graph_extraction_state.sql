-- Graph extraction: a durable state per chunk (design task CR-T12).
--
-- ── Why this exists ──────────────────────────────────────────────────────────
--
-- graph-index decided what to extract from two things that live nowhere durable:
--
--   * "has no graph_text_units rows" was the only signal of "not extracted yet", so a
--     chunk whose extraction legitimately found nothing (or failed) looked the same as a
--     chunk nobody had tried. The reconciler re-selected it on every tick: measured on the
--     eval corpus, 63 provider calls per tick (about 18,000 a day) for nine documents that
--     hold no entities, and one failing chunk retried at least ten times at six-minute
--     intervals (docker logs graph-index, 2026-09-29 and 2026-09-30).
--   * the only memory of "tried, nothing there" was a HashSet in the process, so every
--     restart paid for every empty document again.
--
-- Nothing recorded which model or prompt produced a graph row, why a chunk has no graph
-- (zero-retention organization, restricted document, policy off, document not visible to
-- the organization, a failing provider), or how many times a chunk had been tried.
--
-- This table is that memory: one row per (organization, chunk), written by graph-index
-- and read by graph-index. It is NOT the graph (graph_entities, graph_relationships,
-- graph_claims and graph_text_units stay as they are); it records what was done to a chunk.
--
-- ── Status vocabulary ────────────────────────────────────────────────────────
--
--   pending             eligible and waiting: never tried, or waiting for something to
--                       resolve (the organization's retention posture could not be
--                       resolved: error_class posture_unavailable).
--   ready               extracted, and at least one entity, relationship or claim was
--                       stored.
--   empty               extracted cleanly and there was nothing to store. DURABLE: it is
--                       not retried until the extraction prompt changes (a different
--                       prompt_version), and a restart does not forget it.
--   skipped_zdr         withheld from the provider because the organization is
--                       zero-retention (zdr_org), the document is restricted
--                       (zdr_document) or no attested zero-retention route exists
--                       (zdr_unavailable). Zero provider calls were made. Always carries
--                       an error_class.
--   skipped_policy      the organization's processing policy turns graph extraction off
--                       (operator-set, with a reason, in org_processing_policy).
--   skipped_visibility  the chunk's document is not visible to the whole organization
--                       (private or shared). Graph entities are organization-shared, so
--                       extracting such a document would leak it through the graph.
--   skipped_budget      the organization's background budget is used up and its budget
--                       mode is `stop` (explicit, operator-set). Never the default.
--   failed_transient    a retryable failure; next_attempt_at says when. Always carries
--                       an error_class.
--   failed_permanent    given up: the attempts reached the poison cap (6) or the failure
--                       cannot be fixed by trying again. Not retried until an operator
--                       puts the row back to pending. Always carries an error_class.
--
-- A row that is not final (everything except ready, failed_permanent and a current empty)
-- is due when next_attempt_at is NULL or past and no worker holds a lease on it.
--
-- ── Error classes ────────────────────────────────────────────────────────────
--
-- `error_class` is a CLOSED set of content-free labels an operator can group by, never
-- free text (free text is where document content leaks into a log). The names are the
-- ones knowledge_units.context_error_class uses, so one vocabulary reads across both:
--
--   rate_limited, provider_unavailable, timeout, auth, zdr_unavailable, zdr_org,
--   zdr_document, residency_unavailable, posture_unavailable, invalid_output, too_long,
--   internal
--
-- Adding a class is a deliberate act: a later migration drops and re-adds the named
-- constraint, and the pinned vocabulary in tests/contract/postgres changes with it.
--
-- ── Version stamps ───────────────────────────────────────────────────────────
--
-- prompt_version is the version of the extraction prompt that produced the row
-- (graph-v1 today; the code pins the prompt text to the version with a fingerprint test),
-- `model` and `provider` are what actually served the call (InferResponse.model_used and
-- provider_used), never what was asked for. They are joined to the graph through
-- graph_text_units.knowledge_id, which is how an entity is traced to the model and prompt
-- that produced it: graph_entities.provenance stays the constant 'extracted' because an
-- entity is merged from many chunks and has no single model.
--
-- ── Leases ───────────────────────────────────────────────────────────────────
--
-- The event consumer and the reconciler can reach the same chunk. A worker claims a chunk
-- in one statement (attempts + 1, lease_until in the future); the claim fails while
-- another worker holds an unexpired lease, so one chunk is never paid for twice at once.
-- A worker that dies mid-call leaves a lease that expires: the chunk is due again and its
-- attempt was already counted, so a chunk that kills its worker is still bounded by the cap.
--
-- ── Legacy backfill ──────────────────────────────────────────────────────────
--
-- Every chunk that already has graph_text_units rows was extracted before this table
-- existed. They become `ready` with prompt_version `legacy` and no model (never recorded,
-- not guessed), so deploying this migration does not make the service pay to extract the
-- whole corpus a second time. A chunk that was tried and found nothing has no rows to show
-- for it; it is attempted once more and then recorded as `empty`.
--
-- ── Rollout / runbook ────────────────────────────────────────────────────────
--
--   Apply   the `migrate` one-shot of apps/Data Plane v2/docker-compose.yml
--           (`docker compose run --rm migrate up`); graph-index depends on it
--           (service_completed_successfully), so a graph-index image never starts against
--           a database without this table. Additive: a new table and a view; no existing
--           table is altered, and the running graph-index does not read either.
--   Verify  SELECT * FROM v_graph_extraction_coverage;
--           Expected on the live corpus at the time of writing: the chunks that already
--           have graph rows are `ready_legacy`, every other org-visible chunk is `no_state`
--           until graph-index (the new image) records it.
--   Revert  migrator down --to 20261003140000 (the paired .down.sql drops the view and the
--           table). The graph itself is untouched; only what this migration recorded is lost.
--
-- Every statement is idempotent: CREATE ... IF NOT EXISTS / OR REPLACE, constraints are
-- dropped and re-added by name, the policy is dropped and re-created and the backfill
-- inserts only rows that do not exist.

-- A new table takes no lock on the tables it references beyond the foreign key's own, but
-- do not queue behind a long transaction on knowledge_units (see 20260824120000): give up
-- after 10 s and let the operator retry.
SET LOCAL lock_timeout = '10s';

-- ── 1. The table ─────────────────────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS graph_extraction_state (
    org_id             TEXT        NOT NULL,
    -- The chunk. Cascades: a re-chunk deletes the old knowledge units and with them their
    -- state, exactly as graph_text_units goes.
    knowledge_id       TEXT        NOT NULL REFERENCES knowledge_units(knowledge_id) ON DELETE CASCADE,
    status             TEXT        NOT NULL DEFAULT 'pending',
    -- Attempts that count toward the poison cap, incremented when a worker claims the
    -- chunk. A deployment problem (credential, posture) hands the attempt back.
    attempts           INTEGER     NOT NULL DEFAULT 0,
    next_attempt_at    TIMESTAMPTZ,
    lease_until        TIMESTAMPTZ,
    last_attempt_at    TIMESTAMPTZ,
    prompt_version     TEXT,
    model              TEXT,
    provider           TEXT,
    extracted_at       TIMESTAMPTZ,
    entity_count       INTEGER,
    relationship_count INTEGER,
    claim_count        INTEGER,
    error_class        TEXT,
    created_at         TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at         TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (org_id, knowledge_id)
);

-- ── 2. Constraints ───────────────────────────────────────────────────────────

ALTER TABLE graph_extraction_state DROP CONSTRAINT IF EXISTS graph_extraction_state_org_chk;
ALTER TABLE graph_extraction_state ADD CONSTRAINT graph_extraction_state_org_chk
    CHECK (char_length(btrim(org_id)) > 0);

ALTER TABLE graph_extraction_state DROP CONSTRAINT IF EXISTS graph_extraction_state_status_chk;
ALTER TABLE graph_extraction_state ADD CONSTRAINT graph_extraction_state_status_chk
    CHECK (status IN (
        'pending', 'ready', 'empty',
        'skipped_zdr', 'skipped_policy', 'skipped_visibility', 'skipped_budget',
        'failed_transient', 'failed_permanent'));

ALTER TABLE graph_extraction_state DROP CONSTRAINT IF EXISTS graph_extraction_state_error_class_chk;
ALTER TABLE graph_extraction_state ADD CONSTRAINT graph_extraction_state_error_class_chk
    CHECK (error_class IS NULL OR error_class IN (
        'rate_limited', 'provider_unavailable', 'timeout', 'auth',
        'zdr_unavailable', 'zdr_org', 'zdr_document',
        'residency_unavailable', 'posture_unavailable',
        'invalid_output', 'too_long', 'internal'));

-- A failure and a zero-retention skip always say why.
ALTER TABLE graph_extraction_state DROP CONSTRAINT IF EXISTS graph_extraction_state_failure_class_chk;
ALTER TABLE graph_extraction_state ADD CONSTRAINT graph_extraction_state_failure_class_chk
    CHECK (status NOT IN ('failed_transient', 'failed_permanent', 'skipped_zdr') OR error_class IS NOT NULL);

ALTER TABLE graph_extraction_state DROP CONSTRAINT IF EXISTS graph_extraction_state_counters_chk;
ALTER TABLE graph_extraction_state ADD CONSTRAINT graph_extraction_state_counters_chk
    CHECK (attempts >= 0
        AND (entity_count IS NULL OR entity_count >= 0)
        AND (relationship_count IS NULL OR relationship_count >= 0)
        AND (claim_count IS NULL OR claim_count >= 0));

ALTER TABLE graph_extraction_state DROP CONSTRAINT IF EXISTS graph_extraction_state_prompt_version_chk;
ALTER TABLE graph_extraction_state ADD CONSTRAINT graph_extraction_state_prompt_version_chk
    CHECK (prompt_version IS NULL OR char_length(btrim(prompt_version)) > 0);

-- An extraction that completed is stamped: when, with which prompt and counts, and (except
-- for the legacy rows, which never recorded one) by which model.
ALTER TABLE graph_extraction_state DROP CONSTRAINT IF EXISTS graph_extraction_state_stamp_chk;
ALTER TABLE graph_extraction_state ADD CONSTRAINT graph_extraction_state_stamp_chk
    CHECK (status NOT IN ('ready', 'empty') OR (
        prompt_version IS NOT NULL
        AND extracted_at IS NOT NULL
        AND entity_count IS NOT NULL
        AND relationship_count IS NOT NULL
        AND claim_count IS NOT NULL
        AND (model IS NOT NULL OR prompt_version = 'legacy')));

-- `ready` stored something, `empty` stored nothing. The two words must mean that.
ALTER TABLE graph_extraction_state DROP CONSTRAINT IF EXISTS graph_extraction_state_yield_chk;
ALTER TABLE graph_extraction_state ADD CONSTRAINT graph_extraction_state_yield_chk
    CHECK (
        (status <> 'ready' OR entity_count + relationship_count + claim_count > 0)
        AND (status <> 'empty' OR entity_count + relationship_count + claim_count = 0));

-- ── 3. Indexes ───────────────────────────────────────────────────────────────

-- The work queue: rows that are not final, by the time they come due.
CREATE INDEX IF NOT EXISTS idx_ges_work
    ON graph_extraction_state (org_id, next_attempt_at)
    WHERE status IN ('pending', 'failed_transient', 'skipped_zdr', 'skipped_policy',
                     'skipped_visibility', 'skipped_budget');

-- Coverage and the operator's "what is failing" question.
CREATE INDEX IF NOT EXISTS idx_ges_status
    ON graph_extraction_state (org_id, status);

-- ── 4. updated_at, grants and row-level security ─────────────────────────────

CREATE OR REPLACE TRIGGER set_graph_extraction_state_updated_at
    BEFORE UPDATE ON graph_extraction_state
    FOR EACH ROW EXECUTE FUNCTION trigger_set_updated_at();

-- A new org table is NOT protected automatically (see 20260809120000): this migration
-- establishes its own grant and its own policy, identical to the other org tables.
GRANT SELECT, INSERT, UPDATE, DELETE ON public.graph_extraction_state TO dataplane_app;
ALTER TABLE public.graph_extraction_state ENABLE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS graph_extraction_state_rls_isolation ON public.graph_extraction_state;
CREATE POLICY graph_extraction_state_rls_isolation ON public.graph_extraction_state
    USING (org_id = current_setting('app.current_org', true))
    WITH CHECK (org_id = current_setting('app.current_org', true));

-- ── 5. Legacy backfill ───────────────────────────────────────────────────────
--
-- Every chunk that already has graph rows is `ready` at prompt version `legacy`, with the
-- counts of what is mapped to it and extracted_at no later than its oldest mapping. The
-- join to knowledge_units keeps the foreign key and the organization honest: a mapping
-- whose chunk is gone, or that names another organization's chunk, is not carried over.
INSERT INTO graph_extraction_state
    (org_id, knowledge_id, status, attempts, prompt_version, extracted_at,
     entity_count, relationship_count, claim_count)
SELECT g.org_id, g.knowledge_id, 'ready', 0, 'legacy', min(g.created_at),
       count(g.entity_id), count(g.rel_id), count(g.claim_id)
  FROM graph_text_units AS g
  JOIN knowledge_units AS ku
    ON ku.knowledge_id = g.knowledge_id AND ku.org_id = g.org_id
 GROUP BY g.org_id, g.knowledge_id
ON CONFLICT (org_id, knowledge_id) DO NOTHING;

-- Post-condition, proved inside the transaction (the same pattern as
-- 20261003130000_context_provenance): no chunk with graph rows may be left without a
-- state, or the service would pay to extract it again.
DO $post$
DECLARE
    v_unrecorded BIGINT;
BEGIN
    SELECT count(*) INTO v_unrecorded
      FROM (
            SELECT DISTINCT g.org_id, g.knowledge_id
              FROM public.graph_text_units AS g
              JOIN public.knowledge_units AS ku
                ON ku.knowledge_id = g.knowledge_id AND ku.org_id = g.org_id
           ) AS mapped
      LEFT JOIN public.graph_extraction_state AS s
        ON s.org_id = mapped.org_id AND s.knowledge_id = mapped.knowledge_id
     WHERE s.knowledge_id IS NULL;
    IF v_unrecorded > 0 THEN
        RAISE EXCEPTION 'graph extraction state backfill left % chunk(s) with graph rows and no state', v_unrecorded;
    END IF;
END
$post$;

-- ── 6. v_graph_extraction_coverage ───────────────────────────────────────────
--
-- One row per org that has chunks graph extraction may look at: live chunks of documents
-- that are visible to the whole organization and whose classification may reach the model
-- (internal, public, sensitive: the same set the reconciler selects and the contextual
-- retrieval coverage view uses). Every count is over those chunks.
--
--   chunks         = no_state + pending + ready + empty + skipped_* + failed_*  (a partition)
--   no_state       chunks graph-index has not recorded yet (the backlog).
--   ready_legacy   `ready` chunks the backfill recorded (prompt_version legacy): extracted
--                  before provenance was kept, so their model is unknown.
--
-- There is no "empty at the current prompt version" column on purpose: the current prompt
-- version is an application constant the database cannot know; group by prompt_version.
--
-- An operator's alert is `failed_permanent` growing, `skipped_budget` not zero,
-- `failed_transient` that does not drain, and `no_state` that does not fall.
--
-- security_invoker is REQUIRED. Without it the view runs with its owner's privileges (a
-- superuser, which bypasses row-level security), so a scoped dataplane_app transaction
-- would read every org's counts through it.
CREATE OR REPLACE VIEW v_graph_extraction_coverage
    WITH (security_invoker = true) AS
SELECT
    ku.org_id,
    count(*)                                                                    AS chunks,
    count(*) FILTER (WHERE s.knowledge_id IS NULL)                              AS no_state,
    count(*) FILTER (WHERE s.status = 'pending')                                AS pending,
    count(*) FILTER (WHERE s.status = 'ready')                                  AS ready,
    count(*) FILTER (WHERE s.status = 'ready' AND s.prompt_version = 'legacy')  AS ready_legacy,
    count(*) FILTER (WHERE s.status = 'empty')                                  AS empty,
    count(*) FILTER (WHERE s.status = 'skipped_zdr')                            AS skipped_zdr,
    count(*) FILTER (WHERE s.status = 'skipped_policy')                         AS skipped_policy,
    count(*) FILTER (WHERE s.status = 'skipped_visibility')                     AS skipped_visibility,
    count(*) FILTER (WHERE s.status = 'skipped_budget')                         AS skipped_budget,
    count(*) FILTER (WHERE s.status = 'failed_transient')                       AS failed_transient,
    count(*) FILTER (WHERE s.status = 'failed_permanent')                       AS failed_permanent
FROM knowledge_units AS ku
JOIN documents AS d
  ON d.document_id = ku.document_id AND d.org_id = ku.org_id
LEFT JOIN graph_extraction_state AS s
  ON s.org_id = ku.org_id AND s.knowledge_id = ku.knowledge_id
WHERE d.visibility = 'org'
  AND d.deleted_at IS NULL
  AND lower(btrim(d.zdr_classification)) IN ('internal', 'public', 'sensitive')
GROUP BY ku.org_id;

-- Read-only. ALTER DEFAULT PRIVILEGES (20260809120000) hands the runtime role
-- INSERT/UPDATE/DELETE on every new relation, including this aggregate view, where they
-- could only ever fail; take them back so the grant says what is meant.
GRANT SELECT ON public.v_graph_extraction_coverage TO dataplane_app;
REVOKE INSERT, UPDATE, DELETE ON public.v_graph_extraction_coverage FROM dataplane_app;

-- ── 7. Comments (the database is the documentation operators actually read) ──

COMMENT ON TABLE graph_extraction_state IS
    'Graph extraction: what was done to each chunk (status, attempts, backoff, the prompt version and model that produced it). One row per (org, chunk). Not the graph itself. graph-index writes and reads it; RLS by org.';
COMMENT ON COLUMN graph_extraction_state.status IS
    'pending | ready | empty | skipped_zdr | skipped_policy | skipped_visibility | skipped_budget | failed_transient | failed_permanent. ready stored something; empty found nothing (durable until the prompt version changes); failed_permanent is given up (poison cap or unfixable) until an operator requeues it.';
COMMENT ON COLUMN graph_extraction_state.attempts IS
    'Attempts that count toward the poison cap (6). Incremented when a worker claims the chunk; handed back when the cause was a deployment problem (credential, retention posture) and not the chunk.';
COMMENT ON COLUMN graph_extraction_state.next_attempt_at IS
    'Earliest time the chunk may be worked again; NULL on a non-final row means now. Backoff 5 minutes to 6 hours.';
COMMENT ON COLUMN graph_extraction_state.lease_until IS
    'A worker holds the chunk until then. Cleared when the attempt settles; an expired lease means the worker died.';
COMMENT ON COLUMN graph_extraction_state.prompt_version IS
    'Version of the extraction prompt that produced the row (graph-v1). legacy: extracted before provenance was recorded.';
COMMENT ON COLUMN graph_extraction_state.model IS
    'Model that served the call (InferResponse.model_used). NULL on legacy rows; not guessed.';
COMMENT ON COLUMN graph_extraction_state.provider IS
    'Provider that served the call (InferResponse.provider_used). NULL on legacy rows.';
COMMENT ON COLUMN graph_extraction_state.error_class IS
    'Closed, content-free label: rate_limited | provider_unavailable | timeout | auth | zdr_unavailable | zdr_org | zdr_document | residency_unavailable | posture_unavailable | invalid_output | too_long | internal. Required with failed_* and skipped_zdr. Never free text.';
COMMENT ON VIEW v_graph_extraction_coverage IS
    'Graph extraction coverage per org over the chunks extraction may look at (live, organization-visible documents whose classification may reach the model). security_invoker: honours the caller row-level security.';
