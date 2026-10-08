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
-- WHAT THIS FILE DOES NOT DO (CR-T14, OPS-ORG): it is the Better Auth half of the provisioning, and only that.
-- org-core builds its own `organizations` table from events that auth-core emits when an organization is created
-- through the application; rows inserted here with SQL emit none. With the `tenant` retention posture (CR-T1,
-- CR-T10) the Data Plane's background services ask Auth Core, which asks org-core, whether an organization is
-- zero-retention, and an organization org-core does not know is refused (503 retention_posture_unavailable,
-- no token, no embedding, no context). So the two evaluation organizations must ALSO be created through the real
-- organization service by an operator (docs/contextual-retrieval-runbook.md, section 5.12); this file then adds
-- the corpus-seeder's membership, which a service subject can never get through sign-up, and an owner, which the
-- startup preflight of auth-core requires.
--
-- Run with:
--   docker exec -i controlplane-postgres sh -c 'psql -U "$POSTGRES_USER" -d auth_service -v ON_ERROR_STOP=1' < "apps/Data Plane v2/scripts/eval-fixture-membership.sql"
--
-- Undo with:
--   DELETE FROM member WHERE user_id IN ('service:corpus-seeder', 'user:eval-fixture-owner');
--   DELETE FROM organization WHERE id IN ('org-corpus-baseline','org-corpus-contextual');
--   DELETE FROM "user" WHERE id IN ('service:corpus-seeder', 'user:eval-fixture-owner');

INSERT INTO "user" (id, name, email)
VALUES ('service:corpus-seeder', 'Corpus Seeder (dev eval fixture)', 'corpus-seeder@fixture.invalid')
ON CONFLICT (id) DO NOTHING;

INSERT INTO organization (id, name, created_at)
VALUES
  ('org-corpus-baseline',   'Corpus Baseline (dev eval fixture)',   now()),
  ('org-corpus-contextual', 'Corpus Contextual (dev eval fixture)', now())
ON CONFLICT (id) DO NOTHING;

-- The owner (fixed 2026-10-07). auth-core's owner-invariant preflight (migration 018) refuses to START while any
-- organization has no member whose role contains 'owner'. The first version of this file made the seeder a plain
-- 'member' and so installed a latent startup blocker: nothing failed until auth-service next restarted, and then it
-- crash-looped with OWNER_INVARIANT_PREFLIGHT_FAILED (that is what happened on 2026-08-27, after a restart to reload
-- PLANE_SERVICE_PRINCIPALS_JSON, and what scripts/eval-fixture-owner-repair.sql repaired by an operator-reviewed
-- mapping). Each organization now gets a PLACEHOLDER owner that cannot sign in (no account row, no password, an
-- address under .invalid), and the seeder stays a plain member: this file never makes a service principal an
-- owner, and needs no reviewed mapping.
INSERT INTO "user" (id, name, email)
VALUES ('user:eval-fixture-owner', 'Evaluation fixture owner (cannot sign in)', 'eval-fixture-owner@fixture.invalid')
ON CONFLICT (id) DO NOTHING;

INSERT INTO member (id, organization_id, user_id, role, created_at)
VALUES
  ('member-eval-fixture-owner-baseline',   'org-corpus-baseline',   'user:eval-fixture-owner', 'owner',  now()),
  ('member-eval-fixture-owner-contextual', 'org-corpus-contextual', 'user:eval-fixture-owner', 'owner',  now()),
  ('member-corpus-seeder-baseline',        'org-corpus-baseline',   'service:corpus-seeder',   'member', now()),
  ('member-corpus-seeder-contextual',      'org-corpus-contextual', 'service:corpus-seeder',   'member', now())
ON CONFLICT (organization_id, user_id) DO NOTHING;

SELECT 'fixture memberships: ' || count(*) FROM member WHERE user_id = 'service:corpus-seeder';
-- Must be 0: an organization listed here would stop auth-service at its next start.
SELECT 'ownerless fixture organizations: ' || count(*) FROM owner_invariant_preflight_report
 WHERE organization_id IN ('org-corpus-baseline', 'org-corpus-contextual');
