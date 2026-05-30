-- Add GDPR hard delete functionality for org-core

-- Function to hard delete an organization and all related data (GDPR compliance)  
CREATE OR REPLACE FUNCTION gdpr_hard_delete_organization(org_id_param TEXT)
RETURNS JSONB
LANGUAGE plpgsql
SECURITY DEFINER
AS $$
DECLARE
    result JSONB;
    deleted_records JSONB;
BEGIN
    -- Initialize result
    deleted_records := jsonb_build_object(
        'org_id', org_id_param,
        'deleted_at', NOW()
    );
    
    -- Delete from all related tables (explicit for audit trail)
    -- Entitlements
    WITH deleted_entitlements AS (
        DELETE FROM org_entitlements WHERE org_id = org_id_param RETURNING entitlement_key
    )
    SELECT jsonb_agg(entitlement_key) INTO deleted_records->'entitlements' FROM deleted_entitlements;
    
    -- Quotas
    WITH deleted_quotas AS (
        DELETE FROM org_quotas WHERE org_id = org_id_param RETURNING quota_key
    )
    SELECT jsonb_agg(quota_key) INTO deleted_records->'quotas' FROM deleted_quotas;
    
    -- Billing
    WITH deleted_billing AS (
        DELETE FROM org_billing WHERE org_id = org_id_param RETURNING org_id
    )
    SELECT jsonb_agg(org_id) INTO deleted_records->'billing' FROM deleted_billing;
    
    -- Compliance
    WITH deleted_compliance AS (
        DELETE FROM org_compliance WHERE org_id = org_id_param RETURNING org_id
    )
    SELECT jsonb_agg(org_id) INTO deleted_records->'compliance' FROM deleted_compliance;
    
    -- Role mappings
    WITH deleted_roles AS (
        DELETE FROM org_role_mappings WHERE org_id = org_id_param RETURNING id, role_name
    )
    SELECT jsonb_agg(jsonb_build_object('id', id, 'role', role_name)) 
    INTO deleted_records->'role_mappings' FROM deleted_roles;
    
    -- Plan history
    WITH deleted_history AS (
        DELETE FROM org_plan_history WHERE org_id = org_id_param RETURNING id
    )
    SELECT jsonb_agg(id) INTO deleted_records->'plan_history' FROM deleted_history;
    
    -- Finally, delete the organization record itself
    DELETE FROM organizations WHERE id = org_id_param;
    
    result := jsonb_build_object(
        'success', true,
        'org_id', org_id_param,
        'deleted_at', NOW(),
        'deleted_records', deleted_records
    );
    
    RETURN result;
    
EXCEPTION
    WHEN OTHERS THEN
        RETURN jsonb_build_object(
            'success', false,
            'error', SQLERRM,
            'org_id', org_id_param
        );
END;
$$;

-- Function to soft delete an organization (sets deleted_at timestamp)
CREATE OR REPLACE FUNCTION soft_delete_organization(org_id_param TEXT)
RETURNS JSONB
LANGUAGE plpgsql
SECURITY DEFINER
AS $$
DECLARE
    result JSONB;
BEGIN
    -- Soft delete by setting deleted_at timestamp
    UPDATE organizations 
    SET 
        deleted_at = NOW(),
        status = 'deleted',
        updated_at = NOW()
    WHERE id = org_id_param AND deleted_at IS NULL;
    
    IF FOUND THEN
        result := jsonb_build_object(
            'success', true,
            'org_id', org_id_param,
            'deleted_at', NOW()
        );
    ELSE
        result := jsonb_build_object(
            'success', false,
            'error', 'Organization not found or already deleted',
            'org_id', org_id_param
        );
    END IF;
    
    RETURN result;
    
EXCEPTION
    WHEN OTHERS THEN
        RETURN jsonb_build_object(
            'success', false,
            'error', SQLERRM,
            'org_id', org_id_param
        );
END;
$$;

-- Function to purge old soft-deleted organizations (run periodically)
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
