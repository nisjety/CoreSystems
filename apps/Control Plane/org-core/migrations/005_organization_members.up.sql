-- Add organization membership tracking table
-- This table tracks which users belong to which organizations and their roles

CREATE TABLE IF NOT EXISTS organization_members (
    id TEXT PRIMARY KEY DEFAULT gen_random_uuid()::TEXT,
    org_id TEXT NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    user_id TEXT NOT NULL,  -- Better Auth user ID (base62 format)
    role TEXT NOT NULL DEFAULT 'member',  -- 'owner', 'admin', 'member', 'viewer'
    status TEXT NOT NULL DEFAULT 'active',  -- 'active', 'invited', 'suspended'
    invited_by TEXT,  -- User ID of the person who invited this member
    invited_at TIMESTAMPTZ,
    joined_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(org_id, user_id)
);

-- Indexes for common queries
CREATE INDEX IF NOT EXISTS idx_org_members_org_id ON organization_members(org_id);
CREATE INDEX IF NOT EXISTS idx_org_members_user_id ON organization_members(user_id);
CREATE INDEX IF NOT EXISTS idx_org_members_role ON organization_members(role);
CREATE INDEX IF NOT EXISTS idx_org_members_status ON organization_members(status);

-- Composite index for the most common query: get active memberships for a user
CREATE INDEX IF NOT EXISTS idx_org_members_user_status 
    ON organization_members(user_id, status) 
    WHERE status = 'active';

-- Function to add a member to an organization
CREATE OR REPLACE FUNCTION add_organization_member(
    p_org_id TEXT,
    p_user_id TEXT,
    p_role TEXT DEFAULT 'member',
    p_invited_by TEXT DEFAULT NULL
) RETURNS JSONB
LANGUAGE plpgsql
AS $$
DECLARE
    v_member_id TEXT;
    v_result JSONB;
BEGIN
    -- Insert or update membership
    INSERT INTO organization_members (
        id,
        org_id,
        user_id,
        role,
        status,
        invited_by,
        invited_at,
        joined_at
    ) VALUES (
        gen_random_uuid()::TEXT,
        p_org_id,
        p_user_id,
        p_role,
        'active',
        p_invited_by,
        NOW(),
        NOW()
    )
    ON CONFLICT (org_id, user_id) 
    DO UPDATE SET
        role = EXCLUDED.role,
        status = 'active',
        updated_at = NOW()
    RETURNING id INTO v_member_id;

    v_result := jsonb_build_object(
        'success', true,
        'member_id', v_member_id,
        'org_id', p_org_id,
        'user_id', p_user_id,
        'role', p_role
    );

    RETURN v_result;

EXCEPTION
    WHEN OTHERS THEN
        RETURN jsonb_build_object(
            'success', false,
            'error', SQLERRM
        );
END;
$$;

-- Function to remove a member from an organization
CREATE OR REPLACE FUNCTION remove_organization_member(
    p_org_id TEXT,
    p_user_id TEXT
) RETURNS JSONB
LANGUAGE plpgsql
AS $$
BEGIN
    DELETE FROM organization_members
    WHERE org_id = p_org_id AND user_id = p_user_id;

    IF FOUND THEN
        RETURN jsonb_build_object(
            'success', true,
            'org_id', p_org_id,
            'user_id', p_user_id
        );
    ELSE
        RETURN jsonb_build_object(
            'success', false,
            'error', 'Membership not found'
        );
    END IF;

EXCEPTION
    WHEN OTHERS THEN
        RETURN jsonb_build_object(
            'success', false,
            'error', SQLERRM
        );
END;
$$;

-- Function to get user's organizations
CREATE OR REPLACE FUNCTION get_user_organizations(p_user_id TEXT)
RETURNS TABLE (
    id TEXT,
    name TEXT,
    slug TEXT,
    plan TEXT,
    status TEXT,
    org_number TEXT,
    verification_status TEXT,
    brreg_data JSONB,
    metadata JSONB,
    created_at TIMESTAMPTZ,
    updated_at TIMESTAMPTZ,
    member_role TEXT,
    member_status TEXT,
    joined_at TIMESTAMPTZ
)
LANGUAGE sql
STABLE
AS $$
    SELECT 
        o.id,
        o.name,
        o.slug,
        o.plan,
        o.status,
        o.org_number,
        o.verification_status,
        o.brreg_data,
        o.metadata,
        o.created_at,
        o.updated_at,
        om.role as member_role,
        om.status as member_status,
        om.joined_at
    FROM organizations o
    INNER JOIN organization_members om ON o.id = om.org_id
    WHERE om.user_id = p_user_id
        AND om.status = 'active'
        AND o.deleted_at IS NULL
    ORDER BY om.joined_at DESC;
$$;
