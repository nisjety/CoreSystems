-- Canonical Space-to-Data mapping for the first retrieval vertical.
--
-- Application owns Space identity/lifecycle and Control owns the signed access
-- decision. This table owns neither: it maps a Control-authorized Space to one
-- *actual* Data workspace/collection and the owner-resource authorization that
-- must still be current. A name match or a client-supplied workspace filter is
-- never a mapping.
CREATE TABLE IF NOT EXISTS space_retrieval_bindings (
    binding_id                     TEXT PRIMARY KEY DEFAULT gen_random_uuid()::TEXT,
    space_ref                      TEXT NOT NULL,
    org_id                         TEXT NOT NULL,
    workspace_id                   TEXT,
    collection_id                  TEXT,
    owner_resource_ref             TEXT NOT NULL,
    resource_authorization_ref     TEXT NOT NULL,
    binding_state                  TEXT NOT NULL DEFAULT 'active',
    created_at                     TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at                     TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    revoked_at                     TIMESTAMPTZ,
    CONSTRAINT space_retrieval_bindings_identity_chk CHECK (
        char_length(btrim(space_ref)) > 0
        AND char_length(btrim(org_id)) > 0
        AND char_length(btrim(owner_resource_ref)) > 0
        AND char_length(btrim(resource_authorization_ref)) > 0
    ),
    CONSTRAINT space_retrieval_bindings_target_chk CHECK (
        NULLIF(btrim(workspace_id), '') IS NOT NULL
        OR NULLIF(btrim(collection_id), '') IS NOT NULL
    ),
    CONSTRAINT space_retrieval_bindings_state_chk CHECK (
        binding_state IN ('active', 'revoked')
    ),
    CONSTRAINT space_retrieval_bindings_revoked_state_chk CHECK (
        (binding_state = 'active' AND revoked_at IS NULL)
        OR (binding_state = 'revoked' AND revoked_at IS NOT NULL)
    )
);

-- One active mapping makes the first vertical deterministic. Multiple Data
-- resources require an explicit later fan-out contract, not implicit union.
CREATE UNIQUE INDEX IF NOT EXISTS uq_space_retrieval_bindings_active_space
    ON space_retrieval_bindings (space_ref)
    WHERE binding_state = 'active';
CREATE INDEX IF NOT EXISTS idx_space_retrieval_bindings_org_space
    ON space_retrieval_bindings (org_id, space_ref)
    WHERE binding_state = 'active';

-- This migration is after the explicit RLS rollout, so it must establish its
-- own policy rather than relying on a historic enumerated table list.
GRANT SELECT, INSERT, UPDATE, DELETE ON public.space_retrieval_bindings TO dataplane_app;
ALTER TABLE public.space_retrieval_bindings ENABLE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS space_retrieval_bindings_rls_isolation ON public.space_retrieval_bindings;
CREATE POLICY space_retrieval_bindings_rls_isolation ON public.space_retrieval_bindings
    USING (org_id = current_setting('app.current_org', true))
    WITH CHECK (org_id = current_setting('app.current_org', true));
