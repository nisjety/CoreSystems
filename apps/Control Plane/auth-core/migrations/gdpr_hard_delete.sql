-- Add GDPR hard delete functionality for auth-core

-- Function to hard delete a user and all related data (GDPR compliance)
CREATE OR REPLACE FUNCTION gdpr_hard_delete_user(user_id_param TEXT)
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
        'user_id', user_id_param,
        'deleted_at', NOW()
    );
    
    -- Delete from all related tables (cascade should handle most, but explicit for audit)
    -- Sessions
    WITH deleted_sessions AS (
        DELETE FROM session WHERE user_id = user_id_param RETURNING id
    )
    SELECT jsonb_agg(id) INTO deleted_records->'sessions' FROM deleted_sessions;
    
    -- Accounts (OAuth connections)
    WITH deleted_accounts AS (
        DELETE FROM account WHERE user_id = user_id_param RETURNING id
    )
    SELECT jsonb_agg(id) INTO deleted_records->'accounts' FROM deleted_accounts;
    
    -- Two-factor
    WITH deleted_2fa AS (
        DELETE FROM two_factor WHERE user_id = user_id_param RETURNING id
    )
    SELECT jsonb_agg(id) INTO deleted_records->'two_factor' FROM deleted_2fa;
    
    -- Passkeys
    WITH deleted_passkeys AS (
        DELETE FROM passkey WHERE user_id = user_id_param RETURNING id
    )
    SELECT jsonb_agg(id) INTO deleted_records->'passkeys' FROM deleted_passkeys;
    
    -- API Keys
    WITH deleted_apikeys AS (
        DELETE FROM apikey WHERE user_id = user_id_param RETURNING id
    )
    SELECT jsonb_agg(id) INTO deleted_records->'apikeys' FROM deleted_apikeys;
    
    -- Organization members (removes from all orgs)
    WITH deleted_members AS (
        DELETE FROM member WHERE user_id = user_id_param RETURNING id, organization_id
    )
    SELECT jsonb_agg(jsonb_build_object('id', id, 'org_id', organization_id)) 
    INTO deleted_records->'memberships' FROM deleted_members;
    
    -- Team members
    WITH deleted_team_members AS (
        DELETE FROM team_member WHERE user_id = user_id_param RETURNING id
    )
    SELECT jsonb_agg(id) INTO deleted_records->'team_memberships' FROM deleted_team_members;
    
    -- OAuth consents
    WITH deleted_consents AS (
        DELETE FROM oauth_consent WHERE user_id = user_id_param RETURNING id
    )
    SELECT jsonb_agg(id) INTO deleted_records->'oauth_consents' FROM deleted_consents;
    
    -- Finally, delete the user record itself
    DELETE FROM "user" WHERE id = user_id_param;
    
    result := jsonb_build_object(
        'success', true,
        'user_id', user_id_param,
        'deleted_at', NOW(),
        'deleted_records', deleted_records
    );
    
    RETURN result;
    
EXCEPTION
    WHEN OTHERS THEN
        RETURN jsonb_build_object(
            'success', false,
            'error', SQLERRM,
            'user_id', user_id_param
        );
END;
$$;

-- Function to anonymize user data instead of hard delete (softer GDPR option)
CREATE OR REPLACE FUNCTION gdpr_anonymize_user(user_id_param TEXT)
RETURNS JSONB
LANGUAGE plpgsql
SECURITY DEFINER
AS $$
DECLARE
    result JSONB;
    anonymized_email TEXT;
BEGIN
    -- Generate anonymized email
    anonymized_email := 'deleted_' || user_id_param || '@anonymized.local';
    
    -- Anonymize user data
    UPDATE "user" 
    SET 
        name = 'Deleted User',
        email = anonymized_email,
        phone_number = NULL,
        image = NULL,
        banned = true,
        ban_reason = 'GDPR deletion request',
        updated_at = NOW()
    WHERE id = user_id_param;
    
    -- Remove sessions
    DELETE FROM session WHERE user_id = user_id_param;
    
    -- Remove OAuth accounts
    DELETE FROM account WHERE user_id = user_id_param;
    
    -- Remove 2FA
    DELETE FROM two_factor WHERE user_id = user_id_param;
    
    -- Remove passkeys
    DELETE FROM passkey WHERE user_id = user_id_param;
    
    -- Remove API keys
    DELETE FROM apikey WHERE user_id = user_id_param;
    
    result := jsonb_build_object(
        'success', true,
        'user_id', user_id_param,
        'anonymized_at', NOW(),
        'anonymized_email', anonymized_email
    );
    
    RETURN result;
    
EXCEPTION
    WHEN OTHERS THEN
        RETURN jsonb_build_object(
            'success', false,
            'error', SQLERRM,
            'user_id', user_id_param
        );
END;
$$;

-- Index for faster GDPR deletion queries
CREATE INDEX IF NOT EXISTS idx_user_email_banned ON "user"(email, banned);
