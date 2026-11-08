-- W3-E3.7.1: org-scoped job Idempotency-Key (control migration 028).
--
-- 007 made jobs.idempotency_key unique across every tenant. createJob already looks
-- a key up per org (FindByIdempotencyKey filters on org_id), so a key one org used
-- made the same key fail for every other org: the lookup missed (other tenant), the
-- insert hit the global index, and the caller got a 409 it could not explain. The
-- key is the caller's, so its uniqueness is per org: (org_id, idempotency_key).
--
-- Order: the org-scoped index is built first, then the global one is dropped, so
-- there is no moment without a uniqueness guard. Every row that satisfies the
-- global index also satisfies the org-scoped one, so the build cannot fail on
-- existing data. Built without CONCURRENTLY (the runner applies each migration in
-- a transaction); jobs is a small control table.
--
-- Idempotent: IF NOT EXISTS / IF EXISTS, so a re-run changes nothing.

CREATE UNIQUE INDEX IF NOT EXISTS idx_jobs_org_idempotency_key
    ON jobs (org_id, idempotency_key) WHERE idempotency_key IS NOT NULL;

DROP INDEX IF EXISTS idx_jobs_idempotency_key;
