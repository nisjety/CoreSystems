-- Historical owner repair is an operator-reviewed, report-and-stop workflow.
-- This migration deliberately does not choose an owner or mutate membership.
-- An operator must insert exactly one reviewed mapping to an existing canonical
-- Auth member, then explicitly call apply_reviewed_owner_repairs().

CREATE TABLE IF NOT EXISTS owner_invariant_reviewed_mapping (
  mapping_id TEXT PRIMARY KEY,
  organization_id TEXT NOT NULL REFERENCES organization(id) ON DELETE CASCADE,
  owner_user_id TEXT NOT NULL REFERENCES "user"(id) ON DELETE RESTRICT,
  expected_organization_created_at TIMESTAMP NOT NULL,
  expected_member_role TEXT NOT NULL,
  reviewed_by TEXT NOT NULL,
  reviewed_at TIMESTAMPTZ NOT NULL,
  applied_at TIMESTAMPTZ,
  applied_member_id TEXT,
  CONSTRAINT owner_repair_mapping_id_check CHECK (BTRIM(mapping_id) <> ''),
  CONSTRAINT owner_repair_role_check CHECK (BTRIM(expected_member_role) <> ''),
  CONSTRAINT owner_repair_reviewer_check CHECK (BTRIM(reviewed_by) <> '')
);

CREATE INDEX IF NOT EXISTS owner_invariant_reviewed_mapping_pending
  ON owner_invariant_reviewed_mapping (organization_id, reviewed_at)
  WHERE applied_at IS NULL;

CREATE TABLE IF NOT EXISTS owner_invariant_repair_audit (
  audit_id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  mapping_id TEXT NOT NULL,
  organization_id TEXT NOT NULL,
  owner_user_id TEXT NOT NULL,
  member_id TEXT NOT NULL,
  previous_role TEXT NOT NULL,
  applied_role TEXT NOT NULL,
  reviewed_by TEXT NOT NULL,
  reviewed_at TIMESTAMPTZ NOT NULL,
  applied_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE OR REPLACE FUNCTION reject_owner_invariant_audit_mutation()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
BEGIN
  RAISE EXCEPTION 'owner invariant repair audit is append-only';
END;
$$;

DROP TRIGGER IF EXISTS owner_invariant_repair_audit_immutable
  ON owner_invariant_repair_audit;
CREATE TRIGGER owner_invariant_repair_audit_immutable
BEFORE UPDATE OR DELETE ON owner_invariant_repair_audit
FOR EACH ROW EXECUTE FUNCTION reject_owner_invariant_audit_mutation();

CREATE OR REPLACE VIEW owner_invariant_preflight_report AS
WITH organization_state AS (
  SELECT
    o.id AS organization_id,
    o.created_at,
    COUNT(m.id) FILTER (
      WHERE 'owner' = ANY(
        regexp_split_to_array(COALESCE(m.role, ''), '[[:space:]]*,[[:space:]]*')
      )
    )::INTEGER AS owner_count
  FROM organization o
  LEFT JOIN member m ON m.organization_id = o.id
  GROUP BY o.id, o.created_at
), pending_mapping AS (
  SELECT
    organization_id,
    COUNT(*)::INTEGER AS mapping_count,
    MIN(owner_user_id) AS owner_user_id,
    MIN(expected_organization_created_at) AS expected_organization_created_at,
    MIN(expected_member_role) AS expected_member_role
  FROM owner_invariant_reviewed_mapping
  WHERE applied_at IS NULL
  GROUP BY organization_id
), classified AS (
  SELECT
    state.organization_id,
    state.owner_count,
    COALESCE(mapping.mapping_count, 0) AS mapping_count,
    mapping.owner_user_id,
    CASE
      WHEN state.owner_count = 0 AND COALESCE(mapping.mapping_count, 0) = 0
        THEN 'ownerless_unmapped'
      WHEN state.owner_count = 0 AND mapping.mapping_count > 1
        THEN 'ownerless_ambiguous_mapping'
      WHEN state.owner_count >= 1 AND COALESCE(mapping.mapping_count, 0) > 0
        THEN 'stale_already_owned'
      WHEN state.owner_count = 0 AND mapping.mapping_count = 1
           AND mapping.expected_organization_created_at <> state.created_at
        THEN 'stale_organization_precondition'
      WHEN state.owner_count = 0 AND mapping.mapping_count = 1
           AND NOT EXISTS (
             SELECT 1 FROM member candidate
             WHERE candidate.organization_id = state.organization_id
               AND candidate.user_id = mapping.owner_user_id
           )
        THEN 'stale_member_missing'
      WHEN state.owner_count = 0 AND mapping.mapping_count = 1
           AND NOT EXISTS (
             SELECT 1 FROM member candidate
             WHERE candidate.organization_id = state.organization_id
               AND candidate.user_id = mapping.owner_user_id
               AND candidate.role = mapping.expected_member_role
           )
        THEN 'stale_member_role'
      ELSE NULL
    END AS issue
  FROM organization_state state
  LEFT JOIN pending_mapping mapping
    ON mapping.organization_id = state.organization_id
)
SELECT organization_id, issue, owner_count, mapping_count, owner_user_id
FROM classified
WHERE issue IS NOT NULL;

CREATE OR REPLACE FUNCTION apply_reviewed_owner_repairs()
RETURNS INTEGER
LANGUAGE plpgsql
AS $$
DECLARE
  issue_count INTEGER;
  applied_count INTEGER := 0;
  affected INTEGER;
  candidate RECORD;
BEGIN
  -- Freeze the canonical evidence while the report is evaluated and mappings
  -- are applied. This is an explicit operator workflow, not a request path.
  LOCK TABLE organization IN SHARE ROW EXCLUSIVE MODE;
  LOCK TABLE member IN SHARE ROW EXCLUSIVE MODE;
  LOCK TABLE owner_invariant_reviewed_mapping IN SHARE ROW EXCLUSIVE MODE;

  SELECT COUNT(*)::INTEGER
  INTO issue_count
  FROM owner_invariant_preflight_report;

  IF issue_count > 0 THEN
    RAISE EXCEPTION 'OWNER_INVARIANT_PREFLIGHT_FAILED: % unresolved issue(s); inspect owner_invariant_preflight_report', issue_count
      USING ERRCODE = 'check_violation';
  END IF;

  FOR candidate IN
    SELECT
      mapping.mapping_id,
      mapping.organization_id,
      mapping.owner_user_id,
      mapping.expected_member_role,
      mapping.reviewed_by,
      mapping.reviewed_at,
      canonical_member.id AS member_id,
      organization.name,
      organization.slug,
      organization.metadata
    FROM owner_invariant_reviewed_mapping mapping
    JOIN organization
      ON organization.id = mapping.organization_id
     AND organization.created_at = mapping.expected_organization_created_at
    JOIN member canonical_member
      ON canonical_member.organization_id = mapping.organization_id
     AND canonical_member.user_id = mapping.owner_user_id
     AND canonical_member.role = mapping.expected_member_role
    WHERE mapping.applied_at IS NULL
    ORDER BY mapping.mapping_id
  LOOP
    UPDATE member
    SET role = 'owner'
    WHERE id = candidate.member_id
      AND organization_id = candidate.organization_id
      AND user_id = candidate.owner_user_id
      AND role = candidate.expected_member_role
      AND NOT EXISTS (
        SELECT 1 FROM member existing_owner
        WHERE existing_owner.organization_id = candidate.organization_id
          AND 'owner' = ANY(
            regexp_split_to_array(
              COALESCE(existing_owner.role, ''),
              '[[:space:]]*,[[:space:]]*'
            )
          )
      );
    GET DIAGNOSTICS affected = ROW_COUNT;
    IF affected <> 1 THEN
      RAISE EXCEPTION 'OWNER_INVARIANT_PREFLIGHT_STALE: mapping % no longer matches canonical membership', candidate.mapping_id
        USING ERRCODE = 'check_violation';
    END IF;

    INSERT INTO organization_projection_outbox (
      organization_id, name, slug, metadata, owner_user_id
    ) VALUES (
      candidate.organization_id,
      candidate.name,
      candidate.slug,
      CASE
        WHEN candidate.metadata IS NULL OR BTRIM(candidate.metadata) = ''
          THEN '{}'::jsonb
        ELSE candidate.metadata::jsonb
      END,
      candidate.owner_user_id
    )
    ON CONFLICT (organization_id) DO UPDATE SET
      owner_user_id = EXCLUDED.owner_user_id,
      revision = organization_projection_outbox.revision + 1,
      published_at = NULL,
      processing_at = NULL,
      last_error = NULL,
      updated_at = NOW();

    UPDATE owner_invariant_reviewed_mapping
    SET applied_at = NOW(), applied_member_id = candidate.member_id
    WHERE mapping_id = candidate.mapping_id AND applied_at IS NULL;
    GET DIAGNOSTICS affected = ROW_COUNT;
    IF affected <> 1 THEN
      RAISE EXCEPTION 'OWNER_INVARIANT_PREFLIGHT_STALE: mapping % was concurrently applied', candidate.mapping_id
        USING ERRCODE = 'check_violation';
    END IF;

    INSERT INTO owner_invariant_repair_audit (
      mapping_id, organization_id, owner_user_id, member_id,
      previous_role, applied_role, reviewed_by, reviewed_at
    ) VALUES (
      candidate.mapping_id, candidate.organization_id,
      candidate.owner_user_id, candidate.member_id,
      candidate.expected_member_role, 'owner',
      candidate.reviewed_by, candidate.reviewed_at
    );
    applied_count := applied_count + 1;
  END LOOP;

  RETURN applied_count;
END;
$$;

REVOKE ALL ON FUNCTION apply_reviewed_owner_repairs() FROM PUBLIC;
