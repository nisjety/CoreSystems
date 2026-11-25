-- F-f10-06 (D3-998): the 30-day organisation purge skips an organisation under a legal hold.
--
-- purge_old_deleted_organizations() (003) hard-deletes every organisation soft-deleted before the
-- threshold. org_legal_holds (031) cascades from organizations(id), so a hold placed during the
-- grace period was erased with the organisation it was meant to keep. The HTTP erasure routes
-- refuse a held organisation (ERASURE_HOLD_CHECK, internal/http/gdpr_handlers.go); this closes
-- the cron path.
--
-- The switch is the transaction-local setting app.erasure_hold_check, which org-core sets to 'on'
-- in the purge transaction only when ERASURE_HOLD_CHECK is the literal "true"
-- (Repository.PurgeOldDeletedOrganizations). With the setting anything but 'on' the body below
-- runs 003's statements unchanged and returns 003's receipt. With it 'on', an organisation that
-- has any hold with released_at IS NULL (an org hold or a thread hold) is skipped, and the
-- receipt names the skipped organisations in held_org_ids, ids only. A released hold does not
-- keep an organisation. The cron never overrides a hold: it is released through the legal-hold
-- routes first.
--
-- The candidates, the hold test and the purge are one statement, so the organisations reported
-- as held are exactly the ones this pass did not purge.
--
-- CREATE OR REPLACE keeps the signature, owner, SECURITY DEFINER and grants, so a re-run changes
-- nothing.

CREATE OR REPLACE FUNCTION purge_old_deleted_organizations(days_threshold INTEGER DEFAULT 30)
RETURNS JSONB
LANGUAGE plpgsql
SECURITY DEFINER
AS $$
DECLARE
    result JSONB;
    deleted_count INTEGER;
    org_ids TEXT[];
    held_org_ids TEXT[];
BEGIN
    IF COALESCE(current_setting('app.erasure_hold_check', true), '') <> 'on' THEN
        -- Switch off: 003's body, unchanged.
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
    END IF;

    -- Switch on: an organisation with an active legal hold is not purged.
    WITH due AS (
        SELECT organizations.id,
               EXISTS (
                   SELECT 1 FROM org_legal_holds h
                   WHERE h.org_id = organizations.id AND h.released_at IS NULL
               ) AS held
        FROM organizations
        WHERE deleted_at IS NOT NULL
        AND deleted_at < NOW() - (days_threshold || ' days')::INTERVAL
    ),
    to_purge AS (
        SELECT id FROM due WHERE NOT held
    ),
    purged AS (
        SELECT gdpr_hard_delete_organization(id) as result
        FROM to_purge
    )
    SELECT (SELECT COUNT(*) FROM purged),
           (SELECT array_agg((result->>'org_id')::TEXT) FROM purged),
           (SELECT COALESCE(array_agg(id ORDER BY id), ARRAY[]::TEXT[]) FROM due WHERE held)
    INTO deleted_count, org_ids, held_org_ids;

    result := jsonb_build_object(
        'success', true,
        'purged_count', deleted_count,
        'org_ids', org_ids,
        'held_org_ids', to_jsonb(held_org_ids),
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
