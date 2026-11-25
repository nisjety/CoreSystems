-- Reverts 037: purge_old_deleted_organizations() returns to 003's body, which purges every
-- organisation soft-deleted before the threshold whatever its legal holds.

CREATE OR REPLACE FUNCTION purge_old_deleted_organizations(days_threshold INTEGER DEFAULT 30)
RETURNS JSONB
LANGUAGE plpgsql
SECURITY DEFINER
AS $$
DECLARE
    result JSONB;
    deleted_count INTEGER;
    org_ids TEXT[];
BEGIN
    -- Find organizations deleted more than threshold days ago
    WITH to_purge AS (
        SELECT id FROM organizations
        WHERE deleted_at IS NOT NULL
        AND deleted_at < NOW() - (days_threshold || ' days')::INTERVAL
    ),
    purged AS (
        SELECT gdpr_hard_delete_organization(id) as result
        FROM to_purge
    )
    SELECT COUNT(*), array_agg((result->>'org_id')::TEXT)
    INTO deleted_count, org_ids
    FROM purged;

    result := jsonb_build_object(
        'success', true,
        'purged_count', deleted_count,
        'org_ids', org_ids,
        'purged_at', NOW(),
        'days_threshold', days_threshold
    );

    RETURN result;

EXCEPTION
    WHEN OTHERS THEN
        RETURN jsonb_build_object(
            'success', false,
            'error', SQLERRM
        );
END;
$$;
