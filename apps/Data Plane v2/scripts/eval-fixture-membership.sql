-- Dev-eval fixture: make the `corpus-seeder` service principal a Better Auth
-- member of the two evaluation orgs, so retrieval-engine's STRICT membership
-- decision (auth-core /api/v1/internal/authorization/data-plane/decision)
-- allows its queries. Idempotent; scoped to fixture ids only.
--
-- Why this exists: the decision endpoint answers from Better Auth's `member`
-- table, and a service subject (`service:corpus-seeder`) can never arrive
-- there through the signup flow. Without these rows every /v1/retrieve from
-- the eval principal is denied `not_member`, and the retrieval-quality
-- evaluation cannot run. This keeps CONTROL_PLANE_ENFORCEMENT=strict intact —
-- preferable to the ALLOW_INSECURE_DEV_DEFAULTS escape hatch.
--
-- Run with:
--   docker exec -i controlplane-postgres sh -c 'psql -U "$POSTGRES_USER" -d auth_service -v ON_ERROR_STOP=1' < "apps/Data Plane v2/scripts/eval-fixture-membership.sql"
--
-- Undo with:
--   DELETE FROM member WHERE user_id = 'service:corpus-seeder';
--   DELETE FROM organization WHERE id IN ('org-corpus-baseline','org-corpus-contextual');
--   DELETE FROM "user" WHERE id = 'service:corpus-seeder';

INSERT INTO "user" (id, name, email)
VALUES ('service:corpus-seeder', 'Corpus Seeder (dev eval fixture)', 'corpus-seeder@fixture.invalid')
ON CONFLICT (id) DO NOTHING;

INSERT INTO organization (id, name, created_at)
VALUES
  ('org-corpus-baseline',   'Corpus Baseline (dev eval fixture)',   now()),
  ('org-corpus-contextual', 'Corpus Contextual (dev eval fixture)', now())
ON CONFLICT (id) DO NOTHING;

INSERT INTO member (id, organization_id, user_id, role, created_at)
VALUES
  ('member-corpus-seeder-baseline',   'org-corpus-baseline',   'service:corpus-seeder', 'member', now()),
  ('member-corpus-seeder-contextual', 'org-corpus-contextual', 'service:corpus-seeder', 'member', now())
ON CONFLICT (organization_id, user_id) DO NOTHING;

SELECT 'fixture memberships: ' || count(*) FROM member WHERE user_id = 'service:corpus-seeder';
