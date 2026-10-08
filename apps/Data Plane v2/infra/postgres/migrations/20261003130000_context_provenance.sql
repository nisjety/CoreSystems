-- Contextual Retrieval: provenance, status machine, per-org processing policy and
-- a coverage view (design task CR-T6).
--
-- ── Why this exists ──────────────────────────────────────────────────────────
--
-- 20260824120000 added `chunk_context` and nothing else, so a NULL context
-- conflates every cause: a ZDR-restricted document, "not generated yet", a
-- provider failure, a trivial chunk. Nothing recorded which model, prompt or
-- input produced a stored context, nothing recorded whether the stored VECTOR was
-- built from the stored CONTEXT (the context is persisted after the vector,
-- best-effort), and `embedding_model` has been NULL on every row since that
-- column was created. This migration adds the columns and tables that the later
-- changes build on: the contextualizer (CR-T7), the write-ahead pipeline (CR-T8),
-- the reconciler (CR-T9a) and the policy/coverage API (CR-T11).
--
-- NOTHING READS OR WRITES THE NEW COLUMNS YET. This migration changes no
-- behaviour and is deployable on its own. Until CR-T8 ships, the running
-- embedding-engine keeps writing `chunk_context` / `context_prompt_version` with
-- `context_status` NULL; that is a supported state (see "NULL status" below) and
-- `context_provenance_backfill()` is idempotent, so it is simply run again at the
-- CR-T8 deploy.
--
-- Existing writers are compatible with the new CHECKs (verified against the code,
-- not assumed). The only writer of `chunk_context` is embedding-engine's
-- `persist_chunk_context`, which stores a non-empty context and never clears one;
-- index-engine only inserts and deletes units and never touches the column; no
-- production query is `SELECT *` over knowledge_units. A `legacy` or `ready` row
-- therefore cannot be pushed into a violation of the "a context exists" CHECK by
-- today's code. A writer that clears `chunk_context` (a purge) must change
-- `context_status` in the same statement; CR-T8 owns that rule.
--
-- ── Status vocabulary ────────────────────────────────────────────────────────
--
--   NULL              unknown: the row predates this migration or was written by
--                     code that does not record a status. NOT "pending".
--   pending           eligible, no usable context yet; the reconciler fills it.
--   ready             a context exists and its provenance was recorded.
--   legacy            a context exists but its provenance is unknown: it was
--                     written before provenance was recorded. A real status, not
--                     a synonym for ready. See "Legacy backfill".
--   skipped_zdr       withheld from the provider because the document or org is
--                     zero-data-retention. Always carries an error_class.
--   skipped_policy    org processing policy is off (operator-set, with a reason).
--   skipped_budget    org budget mode is `stop` and the budget is exhausted.
--   failed_transient  retryable failure. Always carries an error_class.
--   failed_permanent  terminal failure. Always carries an error_class.
--
-- ── Error classes ────────────────────────────────────────────────────────────
--
-- `context_error_class` is a CLOSED set of content-free labels an operator can
-- group by, never free text (free text is where document content leaks into a
-- log). The set is enforced by a CHECK so a typo or an upstream message cannot
-- be stored:
--
--   rate_limited, provider_unavailable, timeout, auth, content_filter,
--   invalid_output, doc_missing, too_long, internal
--   zdr_org / zdr_document / zdr_unavailable
--                     why a ZDR skip happened: the ORG is zero-retention, the
--                     DOCUMENT is restricted, or no attested route exists. A
--                     restricted document and a ZDR org must stay
--                     distinguishable (CR-T7).
--   residency_unavailable
--                     no provider satisfies the residency floor.
--   posture_unavailable
--                     the org's retention posture could not be resolved
--                     (Control returned 503 retention_posture_unavailable). The
--                     unit stays `pending` and is retried; it is neither sent
--                     nor skipped, and the class says why it is waiting.
--
-- Adding a class is a deliberate act: a later migration drops and re-adds the
-- named constraint, and the pinned vocabulary in
-- tests/contract/postgres/migration_static_test.go changes with it.
--
-- ── Vector coherence ─────────────────────────────────────────────────────────
--
-- `embedded_context_hash` is the sha256 (lowercase hex) of the exact `chunk_context`
-- bytes the stored vector was built from, NULL for a raw vector. The canonical
-- definition is `context_text_hash()` below: sha256 over the UTF-8 bytes of the
-- stored string, no re-normalisation. Whatever trimming a writer wants happens
-- BEFORE it stores the context; it then hashes what it stored. A vector is
-- coherent when its hash equals `context_text_hash(chunk_context)`.
--
-- ── Legacy backfill (deliberately does NOT assert coherence) ─────────────────
--
-- Rows that already carry a context were written by a pipeline that persisted the
-- context AFTER the vector and never recorded a hash, so whether any given stored
-- vector was built from its stored context is UNKNOWN. Marking them `ready` with
-- a hash computed from the stored context would make `vector_incoherent` read 0
-- for vectors that may be raw or built from a stale context, i.e. it would hide
-- exactly the defect this schema exists to expose. So the backfill:
--   * marks them `legacy` and leaves `embedded_context_hash` NULL;
--   * leaves `embedding_model` NULL (it is stamped only when the legacy
--     verification pass re-embeds the persisted context, the one moment the
--     model is actually known; nothing here hard-codes one);
--   * marks eligible rows that have NO context `pending`;
--   * leaves rows that are not eligible yet (embedding not done, document
--     deleted or restricted) untouched.
-- The backfill does not touch `updated_at`: it is metadata about the context, not
-- a change to the unit, and `updated_at > embedded_at` is what the stale-embedding
-- detector reads as "content changed after it was embedded".
--
-- ── Rollout / runbook ────────────────────────────────────────────────────────
--
--   Apply   the `migrate` one-shot of apps/Data Plane v2/docker-compose.yml
--           (`docker compose run --rm migrate up`). The launcher's Data stage runs
--           it as a dependency of the Data services (it is declared in
--           BOOTSTRAP_SERVICES); the launcher hook apply_dataplane_migrations
--           applies only the two ownership files and does NOT apply this one. The
--           migrate service bind-mounts infra/postgres/migrations, so no image
--           rebuild is needed. Take a backup of knowledge_units first
--           (scripts/backup.sh).
--           ALTER TABLE needs a brief exclusive lock on knowledge_units. The
--           migration waits at most 10 s for it (lock_timeout) and otherwise
--           aborts, with nothing applied: a long builder transaction (see
--           20260824120000) was holding the table, so run `migrate up` again
--           when ingestion is quiet. It never queues behind that transaction
--           and blocks every other reader and writer while it waits.
--   Verify  tests/contract/postgres/live_verify.sql (read-only), or by hand:
--           SELECT * FROM v_context_coverage;
--           Expected on the live corpus at the time of writing: 206 legacy,
--           1 pending, 0 vector_incoherent, row counts unchanged,
--           embedding_model still NULL.
--   Re-run  SELECT * FROM context_provenance_backfill();   -- as the table owner
--           (the migrator / operator connection). Idempotent: it only ever
--           touches rows whose status is still NULL. It takes the same brief
--           table lock (to suspend the updated_at trigger) and gives up after
--           10 s rather than queueing; run it when ingestion is quiet.
--   Revert  migrator down --to 20260907120000 (the paired .down.sql drops the
--           view, functions, table, index, constraints and columns).
--
-- The backfill proves its own result before the transaction commits (the DO
-- $post$ block at the end): a unit that carries a context, or is eligible, and
-- still has no status aborts the whole migration.
--
-- Every statement is idempotent: ADD COLUMN IF NOT EXISTS, constraints are
-- dropped and re-added by name, CREATE ... IF NOT EXISTS / OR REPLACE, and the
-- policy is dropped and re-created.

-- Bound the wait for the table lock (see "Apply" above). SET LOCAL lasts until
-- the migrator's transaction ends, so it cannot leak into a pooled connection.
SET LOCAL lock_timeout = '10s';

-- ── 1. knowledge_units: provenance and status ────────────────────────────────
--
-- All additive and nullable (context_attempts has a constant default, which
-- Postgres applies without rewriting the table), so this is zero-downtime.
ALTER TABLE knowledge_units
    ADD COLUMN IF NOT EXISTS context_status          TEXT,
    ADD COLUMN IF NOT EXISTS context_model           TEXT,
    ADD COLUMN IF NOT EXISTS context_provider        TEXT,
    ADD COLUMN IF NOT EXISTS context_residency       TEXT,
    ADD COLUMN IF NOT EXISTS context_input_hash      TEXT,
    ADD COLUMN IF NOT EXISTS embedded_context_hash   TEXT,
    ADD COLUMN IF NOT EXISTS context_generated_at    TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS context_attempts        INTEGER NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS context_next_attempt_at TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS context_error_class     TEXT,
    ADD COLUMN IF NOT EXISTS context_lease_until     TIMESTAMPTZ;

-- Named constraints, dropped and re-added, so re-running the file converges on
-- the definitions below (the same pattern as 20260907120000_document_space_ref).
ALTER TABLE knowledge_units DROP CONSTRAINT IF EXISTS knowledge_units_context_status_chk;
ALTER TABLE knowledge_units ADD CONSTRAINT knowledge_units_context_status_chk CHECK (
    context_status IS NULL OR context_status IN (
        'pending', 'ready', 'legacy',
        'skipped_zdr', 'skipped_policy', 'skipped_budget',
        'failed_transient', 'failed_permanent'
    )
);

-- Closed set: an error class is a content-free label an operator can group by,
-- never free text (free text is where document content leaks into a log). See
-- "Error classes" above for what each label means and how to add one.
ALTER TABLE knowledge_units DROP CONSTRAINT IF EXISTS knowledge_units_context_error_class_chk;
ALTER TABLE knowledge_units ADD CONSTRAINT knowledge_units_context_error_class_chk CHECK (
    context_error_class IS NULL OR context_error_class IN (
        'rate_limited', 'provider_unavailable', 'timeout', 'auth',
        'zdr_unavailable', 'zdr_org', 'zdr_document',
        'residency_unavailable', 'posture_unavailable',
        'content_filter', 'invalid_output', 'doc_missing', 'too_long', 'internal'
    )
);

-- Nothing silent: a failure or a ZDR skip must say why. (skipped_policy and
-- skipped_budget explain themselves; a `ready` row may carry the class of a
-- failed REGENERATION whose previous context stays in force, so ready is not
-- constrained.)
ALTER TABLE knowledge_units DROP CONSTRAINT IF EXISTS knowledge_units_context_failure_class_chk;
ALTER TABLE knowledge_units ADD CONSTRAINT knowledge_units_context_failure_class_chk CHECK (
    context_status IS NULL
    OR context_status NOT IN ('failed_transient', 'failed_permanent', 'skipped_zdr')
    OR context_error_class IS NOT NULL
);

-- `ready` and `legacy` both assert that a context exists. A writer that clears
-- `chunk_context` (a purge) must change the status in the same statement.
ALTER TABLE knowledge_units DROP CONSTRAINT IF EXISTS knowledge_units_context_present_chk;
ALTER TABLE knowledge_units ADD CONSTRAINT knowledge_units_context_present_chk CHECK (
    context_status IS NULL
    OR context_status NOT IN ('ready', 'legacy')
    OR (chunk_context IS NOT NULL AND btrim(chunk_context) <> '')
);

ALTER TABLE knowledge_units DROP CONSTRAINT IF EXISTS knowledge_units_context_attempts_chk;
ALTER TABLE knowledge_units ADD CONSTRAINT knowledge_units_context_attempts_chk CHECK (
    context_attempts >= 0
);

-- Lowercase hex sha256 only: a hash in any other encoding would silently never
-- match `context_text_hash()` and read as incoherent.
ALTER TABLE knowledge_units DROP CONSTRAINT IF EXISTS knowledge_units_embedded_context_hash_chk;
ALTER TABLE knowledge_units ADD CONSTRAINT knowledge_units_embedded_context_hash_chk CHECK (
    embedded_context_hash IS NULL OR embedded_context_hash ~ '^[0-9a-f]{64}$'
);
ALTER TABLE knowledge_units DROP CONSTRAINT IF EXISTS knowledge_units_context_input_hash_chk;
ALTER TABLE knowledge_units ADD CONSTRAINT knowledge_units_context_input_hash_chk CHECK (
    context_input_hash IS NULL OR context_input_hash ~ '^[0-9a-f]{64}$'
);

-- The work queue the reconciler and the legacy verification pass scan. Partial:
-- almost every row, almost all of the time, is `ready` or not eligible, and
-- those rows are never read through this path. `legacy` is in the predicate
-- because the verification pass selects it (it empties out once that pass has
-- run); `skipped_zdr` is deliberately not (it is re-checked per org, not per row).
CREATE INDEX IF NOT EXISTS idx_ku_context_work
    ON knowledge_units (org_id, context_next_attempt_at)
    WHERE embedding_status = 'done'
      AND (context_status IS NULL
           OR context_status IN ('pending', 'legacy', 'failed_transient', 'skipped_budget'));

-- ── 2. The canonical context hash ────────────────────────────────────────────
--
-- One definition of "the bytes that are hashed", used by the coverage view here
-- and available to writers (`SET embedded_context_hash = context_text_hash(...)`).
-- STRICT: NULL in, NULL out, so a missing context is never "hashed".
--
-- STABLE, not IMMUTABLE: convert_to() is STABLE in Postgres (it consults the
-- conversion catalog), and a wrapper may not promise more than what it calls. An
-- IMMUTABLE label would also invite an index or a generated column over it, which
-- would then silently go stale if the database encoding or a conversion changed.
-- Nothing here needs either.
CREATE OR REPLACE FUNCTION context_text_hash(p_text TEXT)
RETURNS TEXT
LANGUAGE sql
STABLE
STRICT
PARALLEL SAFE
AS $fn$
    SELECT encode(sha256(convert_to(p_text, 'UTF8')), 'hex')
$fn$;

-- ── 3. org_processing_policy ─────────────────────────────────────────────────
--
-- Per-org overrides for background content processing. An ABSENT row means "the
-- system default", which deployment configuration sets (CONTEXTUAL_RETRIEVAL_ENABLED
-- in the Data Plane compose file is true, the incident kill switch); a row exists only because an
-- operator set something, which is why `updated_by` is mandatory. Turning a
-- feature off is an exceptional, audited act: it needs a reason, and the pipeline
-- records an explicit `skipped_policy` status for the affected units, so "off" is
-- never silent.
--
--   contextual_retrieval / graph_extraction   inherit | on | off
--   context_prompt_version                    canary pin for prompt rollouts
--   budget_mode                               inherit | alert_only | throttle | stop
--                                             (alert_only is the system default:
--                                             exceeding a budget alerts, it does
--                                             not stop work; `stop` is explicit)
--   monthly_budget_usd                        NULL = no per-org figure
--
-- The unused org_quotas table (no rows, no consumers) is deliberately untouched.
CREATE TABLE IF NOT EXISTS org_processing_policy (
    org_id                 TEXT        PRIMARY KEY,
    contextual_retrieval   TEXT        NOT NULL DEFAULT 'inherit',
    graph_extraction       TEXT        NOT NULL DEFAULT 'inherit',
    context_prompt_version TEXT,
    budget_mode            TEXT        NOT NULL DEFAULT 'inherit',
    monthly_budget_usd     NUMERIC,
    reason                 TEXT,
    updated_by             TEXT        NOT NULL,
    created_at             TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at             TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT org_processing_policy_org_chk CHECK (char_length(btrim(org_id)) > 0),
    CONSTRAINT org_processing_policy_contextual_retrieval_chk
        CHECK (contextual_retrieval IN ('inherit', 'on', 'off')),
    CONSTRAINT org_processing_policy_graph_extraction_chk
        CHECK (graph_extraction IN ('inherit', 'on', 'off')),
    CONSTRAINT org_processing_policy_budget_mode_chk
        CHECK (budget_mode IN ('inherit', 'alert_only', 'throttle', 'stop')),
    CONSTRAINT org_processing_policy_budget_chk
        CHECK (monthly_budget_usd IS NULL OR monthly_budget_usd >= 0),
    CONSTRAINT org_processing_policy_prompt_version_chk
        CHECK (context_prompt_version IS NULL OR char_length(btrim(context_prompt_version)) > 0),
    CONSTRAINT org_processing_policy_updated_by_chk CHECK (char_length(btrim(updated_by)) > 0),
    CONSTRAINT org_processing_policy_off_reason_chk CHECK (
        (contextual_retrieval <> 'off' AND graph_extraction <> 'off')
        OR (reason IS NOT NULL AND char_length(btrim(reason)) > 0)
    )
);

-- Keep `updated_at` honest the same way the other org tables do.
CREATE OR REPLACE TRIGGER set_org_processing_policy_updated_at
    BEFORE UPDATE ON org_processing_policy
    FOR EACH ROW EXECUTE FUNCTION trigger_set_updated_at();

-- A new org table is NOT protected automatically (see 20260809120000): this
-- migration establishes its own grant and its own policy, identical to the
-- other org tables.
GRANT SELECT, INSERT, UPDATE, DELETE ON public.org_processing_policy TO dataplane_app;
ALTER TABLE public.org_processing_policy ENABLE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS org_processing_policy_rls_isolation ON public.org_processing_policy;
CREATE POLICY org_processing_policy_rls_isolation ON public.org_processing_policy
    USING (org_id = current_setting('app.current_org', true))
    WITH CHECK (org_id = current_setting('app.current_org', true));

-- ── 4. v_context_coverage ────────────────────────────────────────────────────
--
-- One row per org that has indexed units. Every count below is over ELIGIBLE
-- units, the same population quickwit-adapter indexes: embedding done, document
-- not deleted, document classification internal / public / sensitive. A
-- restricted document is never sent to a model provider, so it is out of scope
-- here and counted only in `excluded_by_classification` (visible, never silent).
--
--   eligible  =  ready + legacy + pending + skipped_zdr + skipped_policy
--                + skipped_budget + failed_transient + failed_permanent
--                + status_unknown                      (a partition)
--
--   status_unknown     eligible units whose status is NULL.
--   vector_incoherent  `ready` units whose recorded embedded_context_hash is not
--                      the hash of the stored context (a vector built from a
--                      context that has since changed, or removed). Rows with a
--                      NULL hash are NOT counted here: they are `legacy` (counted
--                      separately) or counted in ready_unverified.
--   ready_unverified   `ready` units with no recorded hash: coverage claims a
--                      context, but nothing proves the vector reflects it.
--
-- A coverage gate should treat ready_unverified, status_unknown and legacy as
-- not-yet-ready. There is no "ready at the current prompt version" column on
-- purpose: the current prompt version is an application constant the database
-- cannot know; group knowledge_units.context_prompt_version in the consumer.
--
-- security_invoker is REQUIRED. Without it the view runs with its owner's
-- privileges (a superuser, which bypasses row-level security), so a scoped
-- dataplane_app transaction would read every org's counts through it.
CREATE OR REPLACE VIEW v_context_coverage
    WITH (security_invoker = true) AS
SELECT
    u.org_id,
    count(*) FILTER (WHERE u.cr_eligible)                                              AS eligible,
    count(*) FILTER (WHERE u.cr_eligible AND u.context_status = 'ready')               AS ready,
    count(*) FILTER (WHERE u.cr_eligible AND u.context_status = 'legacy')              AS legacy,
    count(*) FILTER (WHERE u.cr_eligible AND u.context_status = 'pending')             AS pending,
    count(*) FILTER (WHERE u.cr_eligible AND u.context_status = 'skipped_zdr')         AS skipped_zdr,
    count(*) FILTER (WHERE u.cr_eligible AND u.context_status = 'skipped_policy')      AS skipped_policy,
    count(*) FILTER (WHERE u.cr_eligible AND u.context_status = 'skipped_budget')      AS skipped_budget,
    count(*) FILTER (WHERE u.cr_eligible AND u.context_status = 'failed_transient')    AS failed_transient,
    count(*) FILTER (WHERE u.cr_eligible AND u.context_status = 'failed_permanent')    AS failed_permanent,
    count(*) FILTER (WHERE u.cr_eligible AND u.context_status IS NULL)                 AS status_unknown,
    count(*) FILTER (
        WHERE u.cr_eligible
          AND u.context_status = 'ready'
          AND u.embedded_context_hash IS NOT NULL
          AND u.embedded_context_hash IS DISTINCT FROM context_text_hash(u.chunk_context)
    )                                                                                  AS vector_incoherent,
    count(*) FILTER (
        WHERE u.cr_eligible
          AND u.context_status = 'ready'
          AND u.embedded_context_hash IS NULL
    )                                                                                  AS ready_unverified,
    count(*) FILTER (WHERE NOT u.cr_eligible)                                          AS excluded_by_classification
FROM (
    SELECT
        ku.org_id,
        ku.context_status,
        ku.chunk_context,
        ku.embedded_context_hash,
        lower(btrim(d.zdr_classification)) IN ('internal', 'public', 'sensitive') AS cr_eligible
    FROM knowledge_units ku
    JOIN documents d ON d.document_id = ku.document_id
    WHERE ku.embedding_status = 'done'
      AND d.deleted_at IS NULL
) AS u
GROUP BY u.org_id;

-- Read-only. ALTER DEFAULT PRIVILEGES (20260809120000) hands the runtime role
-- INSERT/UPDATE/DELETE on every new relation, including this aggregate view, where
-- they could only ever fail; take them back so the grant says what is meant.
GRANT SELECT ON public.v_context_coverage TO dataplane_app;
REVOKE INSERT, UPDATE, DELETE ON public.v_context_coverage FROM dataplane_app;

-- ── 5. Legacy backfill, as a re-runnable function ────────────────────────────
--
-- A function rather than inline statements so the migration and every later
-- re-run (the CR-T8 deploy, an operator repair) execute ONE definition. It only
-- ever touches rows whose status is still NULL, so it is idempotent, and it never
-- overwrites a status a newer writer set.
--
-- It suspends the `updated_at` trigger around its two UPDATEs (see "Legacy
-- backfill" above) and restores it to exactly the enablement it found. That needs
-- table ownership, so it must run as the migrator / operator connection, which is
-- why EXECUTE is revoked from PUBLIC: the runtime role dataplane_app cannot call
-- it, and could not alter the table anyway. If an UPDATE fails, the whole
-- statement (including the trigger change) rolls back with it.
--
-- Returns the number of rows marked legacy and pending. `p_org_id` limits the
-- run to one org.
--
-- The trigger change takes a SHARE ROW EXCLUSIVE lock on knowledge_units (writers
-- wait, readers do not). `SET lock_timeout` bounds how long the function queues for
-- it, so a re-run during ingestion fails fast instead of stalling every writer
-- behind a long builder transaction. The setting is restored when the function
-- returns. (Inside this migration the table is already exclusively locked by the
-- ALTER TABLE above, so the lock is granted at once.)
CREATE OR REPLACE FUNCTION context_provenance_backfill(p_org_id TEXT DEFAULT NULL)
RETURNS TABLE (legacy_marked BIGINT, pending_marked BIGINT)
LANGUAGE plpgsql
SET lock_timeout = '10s'
AS $fn$
DECLARE
    v_trigger_state "char";
    v_legacy        BIGINT := 0;
    v_pending       BIGINT := 0;
BEGIN
    SELECT t.tgenabled
      INTO v_trigger_state
      FROM pg_trigger t
     WHERE t.tgrelid = 'public.knowledge_units'::regclass
       AND t.tgname = 'set_knowledge_units_updated_at'
       AND NOT t.tgisinternal;

    -- 'D' = already disabled: nothing to suspend and nothing to restore.
    IF v_trigger_state IS NOT NULL AND v_trigger_state <> 'D' THEN
        ALTER TABLE public.knowledge_units DISABLE TRIGGER set_knowledge_units_updated_at;
    END IF;

    -- A stored context of unknown provenance. Counts for every such row, whatever
    -- the unit's embedding/document state: the context is there either way.
    UPDATE public.knowledge_units AS ku
       SET context_status = 'legacy'
     WHERE ku.context_status IS NULL
       AND ku.chunk_context IS NOT NULL
       AND btrim(ku.chunk_context) <> ''
       AND (p_org_id IS NULL OR ku.org_id = p_org_id);
    GET DIAGNOSTICS v_legacy = ROW_COUNT;

    -- Eligible and without a usable context: the reconciler fills these. Same
    -- eligibility predicate as v_context_coverage and quickwit-adapter.
    UPDATE public.knowledge_units AS ku
       SET context_status = 'pending',
           context_next_attempt_at = now()
      FROM public.documents AS d
     WHERE d.document_id = ku.document_id
       AND ku.context_status IS NULL
       AND (ku.chunk_context IS NULL OR btrim(ku.chunk_context) = '')
       AND ku.embedding_status = 'done'
       AND d.deleted_at IS NULL
       AND lower(btrim(d.zdr_classification)) IN ('internal', 'public', 'sensitive')
       AND (p_org_id IS NULL OR ku.org_id = p_org_id);
    GET DIAGNOSTICS v_pending = ROW_COUNT;

    IF v_trigger_state = 'O' THEN
        ALTER TABLE public.knowledge_units ENABLE TRIGGER set_knowledge_units_updated_at;
    ELSIF v_trigger_state = 'A' THEN
        ALTER TABLE public.knowledge_units ENABLE ALWAYS TRIGGER set_knowledge_units_updated_at;
    ELSIF v_trigger_state = 'R' THEN
        ALTER TABLE public.knowledge_units ENABLE REPLICA TRIGGER set_knowledge_units_updated_at;
    END IF;

    RAISE NOTICE 'context provenance backfill: % legacy, % pending', v_legacy, v_pending;
    legacy_marked := v_legacy;
    pending_marked := v_pending;
    RETURN NEXT;
END
$fn$;

REVOKE ALL ON FUNCTION context_provenance_backfill(TEXT) FROM PUBLIC;

SELECT * FROM context_provenance_backfill();

-- Post-condition, proved inside the transaction (the same pattern as
-- 20260620130000_add_document_ownership): after the backfill no unit that carries a
-- context, and no eligible unit, may be left without a status. If either count is
-- not zero the whole migration aborts and nothing is applied, instead of shipping a
-- schema whose coverage view would report "unknown" for rows this migration was
-- written to classify. (Design invariant I3: nothing is silent.)
DO $post$
DECLARE
    v_unclassified BIGINT;
BEGIN
    SELECT count(*) INTO v_unclassified
      FROM public.knowledge_units AS ku
     WHERE ku.context_status IS NULL
       AND ku.chunk_context IS NOT NULL
       AND btrim(ku.chunk_context) <> '';
    IF v_unclassified > 0 THEN
        RAISE EXCEPTION 'context provenance backfill left % unit(s) with a context and no status', v_unclassified;
    END IF;

    SELECT count(*) INTO v_unclassified
      FROM public.knowledge_units AS ku
      JOIN public.documents AS d ON d.document_id = ku.document_id
     WHERE ku.context_status IS NULL
       AND ku.embedding_status = 'done'
       AND d.deleted_at IS NULL
       AND lower(btrim(d.zdr_classification)) IN ('internal', 'public', 'sensitive');
    IF v_unclassified > 0 THEN
        RAISE EXCEPTION 'context provenance backfill left % eligible unit(s) without a status', v_unclassified;
    END IF;
END
$post$;

-- ── 6. Comments (the database is the documentation operators actually read) ──
--
-- `chunk_context`'s own comment is corrected here rather than by editing the
-- already-applied 20260824120000 (applied migration files are immutable). Its
-- header called a NULL context the expected default of an OFF feature; a NULL
-- context says nothing about the feature, `context_status` does.
COMMENT ON COLUMN knowledge_units.chunk_context IS
    'Contextual Retrieval: LLM-generated sentences situating this chunk in its document. NOT the text shown to callers - that is always `text`. Prepended to `text` at embed time and indexed at weight B in content_tsv. A NULL context does not mean the feature is off: `context_status` records why a unit has no context, and `context_status = legacy` marks a context written before provenance was recorded.';
COMMENT ON COLUMN knowledge_units.context_prompt_version IS
    'Which prompt produced chunk_context (for example ctx-v1). Lets a refresh pass find contexts built by a superseded prompt.';
COMMENT ON COLUMN knowledge_units.context_status IS
    'Contextual Retrieval status machine: pending | ready | legacy | skipped_zdr | skipped_policy | skipped_budget | failed_transient | failed_permanent. NULL = unknown (not pending). legacy = a context exists but its provenance was never recorded; its vector may or may not reflect it.';
COMMENT ON COLUMN knowledge_units.context_model IS
    'Model alias that generated chunk_context. NULL on legacy rows (never recorded); not guessed.';
COMMENT ON COLUMN knowledge_units.context_provider IS
    'Provider that served the generation call. NULL on legacy rows.';
COMMENT ON COLUMN knowledge_units.context_residency IS
    'Residency label of the route that served the generation call. NULL on legacy rows.';
COMMENT ON COLUMN knowledge_units.context_input_hash IS
    'sha256 (lowercase hex) of the generation inputs: prompt version, model alias, window text, title, chunk text. Decides reuse and staleness: unchanged inputs make no provider call.';
COMMENT ON COLUMN knowledge_units.embedded_context_hash IS
    'sha256 (lowercase hex) of the exact chunk_context bytes the stored vector was built from (see context_text_hash()); NULL = raw vector or unknown. Coherent when equal to context_text_hash(chunk_context).';
COMMENT ON COLUMN knowledge_units.context_generated_at IS
    'When chunk_context was generated. Compared with documents.updated_at to find contexts older than their document.';
COMMENT ON COLUMN knowledge_units.context_attempts IS
    'Generation attempts since the last success; drives the retry ladder.';
COMMENT ON COLUMN knowledge_units.context_next_attempt_at IS
    'Earliest time the reconciler may work this unit again.';
COMMENT ON COLUMN knowledge_units.context_error_class IS
    'Closed, content-free label: rate_limited | provider_unavailable | timeout | auth | zdr_unavailable | zdr_org | zdr_document | residency_unavailable | posture_unavailable | content_filter | invalid_output | doc_missing | too_long | internal. Required with failed_* and skipped_zdr. Never free text.';
COMMENT ON COLUMN knowledge_units.context_lease_until IS
    'Reconciler lease (FOR UPDATE SKIP LOCKED plus this deadline) so two workers never process one unit.';
COMMENT ON INDEX idx_ku_context_work IS
    'Contextual Retrieval work queue: units that are indexed and still need a context, a retry or the legacy verification pass.';
COMMENT ON FUNCTION context_text_hash(TEXT) IS
    'Canonical Contextual Retrieval hash: lowercase hex sha256 over the UTF-8 bytes of the stored string. NULL in, NULL out.';
COMMENT ON FUNCTION context_provenance_backfill(TEXT) IS
    'Idempotent legacy backfill: NULL-status rows with a context become legacy, eligible rows without one become pending. Does not touch updated_at, embedded_context_hash or embedding_model. Owner-only.';
COMMENT ON TABLE org_processing_policy IS
    'Per-org overrides for background content processing (contextual retrieval, graph extraction, prompt-version pin, budget mode). An absent row means the system default. Operator-set and audited; turning a feature off requires a reason.';
COMMENT ON VIEW v_context_coverage IS
    'Contextual Retrieval coverage per org over eligible units (embedding done, document not deleted, classification internal/public/sensitive). security_invoker: honours the caller row-level security.';
