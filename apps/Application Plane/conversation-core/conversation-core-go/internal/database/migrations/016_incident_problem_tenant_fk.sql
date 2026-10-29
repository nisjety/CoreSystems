-- Defense in depth for Incident -> Problem linkage. Service validation already
-- scopes problem lookups by org, but the database must enforce that invariant
-- too so a future repository path cannot create a cross-tenant association.

CREATE UNIQUE INDEX IF NOT EXISTS conversation_problems_org_id_id_key
    ON conversation_problems (org_id, id);

ALTER TABLE conversation_incidents
    DROP CONSTRAINT IF EXISTS conversation_incidents_problem_id_fkey;

ALTER TABLE conversation_incidents
    ADD CONSTRAINT conversation_incidents_org_problem_fkey
    FOREIGN KEY (org_id, problem_id)
    REFERENCES conversation_problems (org_id, id)
    ON DELETE SET NULL;
