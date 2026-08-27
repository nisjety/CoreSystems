-- URGENT: auth-service is crash-looping until this runs.
--
-- ## What happened
--
-- `scripts/eval-fixture-membership.sql` created the two evaluation orgs with a
-- `member` row of role 'member' and no owner. auth-core's owner-invariant
-- preflight (migration 018, pre-existing since 2026-07-16) refuses to start
-- while any organization is ownerless — a report-and-stop workflow that
-- deliberately will not pick an owner for you.
--
-- The fixture has therefore been a latent startup blocker since 2026-08-26.
-- Nothing detected it because auth-service had not restarted since; restarting
-- it to reload PLANE_SERVICE_PRINCIPALS_JSON (for the graph:read grant)
-- detonated it. Current state:
--
--   owner_invariant_preflight_report
--   org-corpus-baseline    | ownerless_unmapped | owners=0 | mappings=0
--   org-corpus-contextual  | ownerless_unmapped | owners=0 | mappings=0
--
-- ## Why this is a script and not something already applied
--
-- Two reasons, both deliberate:
--   1. `reviewed_by` is an operator attestation. Migration 018's own header says
--      an operator must insert exactly one reviewed mapping and then explicitly
--      call the apply function. Forging that signature would defeat the control.
--   2. It grants owner authority in the auth database. That is yours to approve.
--
-- Reviewer: ima.dacosta@aquatiq.com (authorized 2026-08-27).
--
-- Run with:
--   docker exec -i controlplane-postgres sh -c 'psql -U "$POSTGRES_USER" -d auth_service -v ON_ERROR_STOP=1' < "apps/Data Plane v2/scripts/eval-fixture-owner-repair.sql"
--
-- Then restart auth-service:
--   docker restart auth-service
--
-- ## Alternative, if you would rather not grant owner to a service principal
--
-- Delete the fixture instead (this also clears the preflight, and costs only
-- the eval corpus, which is reproducible):
--   DELETE FROM member WHERE user_id = 'service:corpus-seeder';
--   DELETE FROM organization WHERE id IN ('org-corpus-baseline','org-corpus-contextual');
--   DELETE FROM "user" WHERE id = 'service:corpus-seeder';

BEGIN;

-- Reviewed mapping: name the owner for each ownerless fixture org. The
-- expected_* columns are the preflight's optimistic-concurrency check — the
-- apply function refuses if the org drifted since review.
INSERT INTO owner_invariant_reviewed_mapping (
  mapping_id,
  organization_id,
  owner_user_id,
  expected_organization_created_at,
  expected_member_role,
  reviewed_by,
  reviewed_at
)
SELECT
  'eval-fixture-owner-' || o.id,
  o.id,
  'service:corpus-seeder',
  o.created_at,
  -- expected_member_role is a PRECONDITION on the member's CURRENT role, not the
  -- target. `apply_reviewed_owner_repairs()` joins on
  -- `canonical_member.role = mapping.expected_member_role` and then does
  -- `SET role = 'owner'` itself. Passing 'owner' here classifies the mapping as
  -- `stale_member_role` and the apply refuses — the fixture's members hold
  -- 'member'.
  'member',
  -- Operator who reviewed and authorized this repair.
  'ima.dacosta@aquatiq.com',
  now()
FROM organization o
WHERE o.id IN ('org-corpus-baseline', 'org-corpus-contextual')
ON CONFLICT (mapping_id) DO NOTHING;

-- Apply. Raises OWNER_INVARIANT_PREFLIGHT_FAILED if anything remains
-- unresolved, so a partial repair aborts the transaction rather than leaving
-- auth-core still unable to boot.
SELECT apply_reviewed_owner_repairs();

-- Should return zero rows.
SELECT organization_id, issue FROM owner_invariant_preflight_report;

COMMIT;
