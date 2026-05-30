-- Add invited_email column to organization_members for recording email invitations
-- before the user has registered an account.

ALTER TABLE organization_members
    ADD COLUMN IF NOT EXISTS invited_email TEXT;

CREATE INDEX IF NOT EXISTS idx_org_members_invited_email
    ON organization_members (invited_email)
    WHERE invited_email IS NOT NULL;
