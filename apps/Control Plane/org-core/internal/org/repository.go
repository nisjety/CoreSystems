package org

import (
	"context"
	"encoding/json"
	"fmt"
	"time"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgxpool"
)

// orgScanner abstracts both pgx.Row and pgx.Rows for scanOrg
type orgScanner interface {
	Scan(dest ...any) error
}

// scanOrg scans the 14 standard org columns into an Organization.
// Column order must match all SELECT statements in this file.
func scanOrg(row orgScanner) (Organization, error) {
	var org Organization
	var metadata []byte
	var brregRaw []byte
	err := row.Scan(
		&org.ID,
		&org.Name,
		&org.Slug,
		&org.Plan,
		&org.Status,
		&org.PrimaryDomain,
		&org.Region,
		&org.DefaultLocale,
		&metadata,
		&org.CreatedAt,
		&org.UpdatedAt,
		&org.OrgNumber,
		&org.VerificationStatus,
		&brregRaw,
	)
	if err != nil {
		return Organization{}, err
	}
	if len(metadata) > 0 {
		_ = json.Unmarshal(metadata, &org.Metadata)
	}
	if len(brregRaw) > 0 {
		_ = json.Unmarshal(brregRaw, &org.BrregData)
	}
	return org, nil
}

type Repository struct {
	pool *pgxpool.Pool
}

func NewRepository(pool *pgxpool.Pool) *Repository {
	return &Repository{pool: pool}
}

func (r *Repository) GetOrganization(ctx context.Context, id string) (*Organization, error) {
	const q = `
SELECT id, name, COALESCE(slug, ''), plan, status, COALESCE(primary_domain, ''), COALESCE(region, 'eu'), COALESCE(default_locale, 'nb-NO'), metadata, created_at, updated_at,
       org_number, COALESCE(verification_status, 'unverified'), brreg_data
FROM organizations
WHERE id = $1 AND deleted_at IS NULL`

	org, err := scanOrg(r.pool.QueryRow(ctx, q, id))
	if err != nil {
		if err == pgx.ErrNoRows {
			return nil, ErrNotFound
		}
		return nil, fmt.Errorf("query org: %w", err)
	}
	return &org, nil
}

func (r *Repository) UpsertOrganization(ctx context.Context, org Organization) error {
	const q = `
INSERT INTO organizations (id, name, slug, plan, status, primary_domain, region, default_locale, metadata, org_number, verification_status, brreg_data)
VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
ON CONFLICT (id)
DO UPDATE SET
  name = EXCLUDED.name,
  slug = EXCLUDED.slug,
  plan = EXCLUDED.plan,
  status = EXCLUDED.status,
	primary_domain = EXCLUDED.primary_domain,
	region = EXCLUDED.region,
	default_locale = EXCLUDED.default_locale,
  metadata = EXCLUDED.metadata,
  org_number = EXCLUDED.org_number,
  verification_status = EXCLUDED.verification_status,
  brreg_data = EXCLUDED.brreg_data,
  updated_at = NOW(),
  deleted_at = NULL`

	metadata := map[string]any{}
	if org.Metadata != nil {
		metadata = org.Metadata
	}
	metaBuf, err := json.Marshal(metadata)
	if err != nil {
		return fmt.Errorf("marshal metadata: %w", err)
	}

	verificationStatus := org.VerificationStatus
	if verificationStatus == "" {
		verificationStatus = "unverified"
	}

	region := org.Region
	if region == "" {
		region = "eu"
	}
	defaultLocale := org.DefaultLocale
	if defaultLocale == "" {
		defaultLocale = "nb-NO"
	}

	var brregBuf []byte
	if org.BrregData != nil {
		brregBuf, err = json.Marshal(org.BrregData)
		if err != nil {
			return fmt.Errorf("marshal brreg_data: %w", err)
		}
	}

	if _, err := r.pool.Exec(ctx, q,
		org.ID, org.Name, org.Slug, org.Plan, org.Status, org.PrimaryDomain, region, defaultLocale, metaBuf,
		org.OrgNumber, verificationStatus, brregBuf,
	); err != nil {
		return fmt.Errorf("upsert org: %w", err)
	}
	return nil
}

func (r *Repository) SetDefaultEntitlements(ctx context.Context, orgID string) error {
	// Batch all inserts into one round-trip instead of N sequential Exec calls
	batch := &pgx.Batch{}
	const q = `
INSERT INTO org_entitlements (org_id, entitlement_key, enabled)
VALUES ($1, $2, $3)
ON CONFLICT (org_id, entitlement_key)
DO NOTHING`
	for _, ent := range defaultEntitlements {
		batch.Queue(q, orgID, ent.Key, ent.Enabled)
	}
	results := r.pool.SendBatch(ctx, batch)
	defer results.Close()
	for range defaultEntitlements {
		if _, err := results.Exec(); err != nil {
			return fmt.Errorf("set default entitlements: %w", err)
		}
	}
	return nil
}

func (r *Repository) GetEntitlements(ctx context.Context, orgID string) ([]Entitlement, error) {
	const q = `
SELECT entitlement_key, enabled
FROM org_entitlements
WHERE org_id = $1
ORDER BY entitlement_key`

	rows, err := r.pool.Query(ctx, q, orgID)
	if err != nil {
		return nil, fmt.Errorf("query entitlements: %w", err)
	}
	defer rows.Close()

	items := make([]Entitlement, 0, 8)
	for rows.Next() {
		var e Entitlement
		if err := rows.Scan(&e.Key, &e.Enabled); err != nil {
			return nil, fmt.Errorf("scan entitlements: %w", err)
		}
		items = append(items, e)
	}
	if rows.Err() != nil {
		return nil, rows.Err()
	}
	if len(items) == 0 {
		return nil, ErrNotFound
	}
	return items, nil
}

// ListOrganizations returns a paginated list of all organizations.
// limit=0 defaults to 100; max is capped at 500 to prevent accidental full-table loads.
func (r *Repository) ListOrganizations(ctx context.Context, limit, offset int) ([]Organization, error) {
	if limit <= 0 {
		limit = 100
	}
	if limit > 500 {
		limit = 500
	}
	const q = `
SELECT id, name, COALESCE(slug, ''), plan, status, COALESCE(primary_domain, ''), COALESCE(region, 'eu'), COALESCE(default_locale, 'nb-NO'), metadata, created_at, updated_at,
       org_number, COALESCE(verification_status, 'unverified'), brreg_data
FROM organizations
WHERE deleted_at IS NULL
ORDER BY created_at DESC
LIMIT $1 OFFSET $2`

	rows, err := r.pool.Query(ctx, q, limit, offset)
	if err != nil {
		return nil, fmt.Errorf("query organizations: %w", err)
	}
	defer rows.Close()

	orgs := make([]Organization, 0, limit)
	for rows.Next() {
		org, err := scanOrg(rows)
		if err != nil {
			return nil, fmt.Errorf("scan org: %w", err)
		}
		orgs = append(orgs, org)
	}
	if rows.Err() != nil {
		return nil, rows.Err()
	}
	return orgs, nil
}

// ListUserOrganizations returns all organizations for a specific user
func (r *Repository) ListUserOrganizations(ctx context.Context, userID string) ([]Organization, error) {
	const q = `
SELECT o.id, o.name, COALESCE(o.slug, ''), o.plan, o.status, COALESCE(o.primary_domain, ''), COALESCE(o.region, 'eu'), COALESCE(o.default_locale, 'nb-NO'), o.metadata, o.created_at, o.updated_at,
       o.org_number, COALESCE(o.verification_status, 'unverified'), o.brreg_data
FROM organizations o
INNER JOIN organization_members om ON o.id = om.org_id
WHERE om.user_id = $1
  AND om.status = 'active'
  AND o.deleted_at IS NULL
ORDER BY om.joined_at DESC`

	rows, err := r.pool.Query(ctx, q, userID)
	if err != nil {
		return nil, fmt.Errorf("query user organizations: %w", err)
	}
	defer rows.Close()

	orgs := make([]Organization, 0, 8)
	for rows.Next() {
		org, err := scanOrg(rows)
		if err != nil {
			return nil, fmt.Errorf("scan org: %w", err)
		}
		orgs = append(orgs, org)
	}
	if rows.Err() != nil {
		return nil, rows.Err()
	}
	return orgs, nil
}

// AddOrganizationMember adds a user as a member of an organization
func (r *Repository) AddOrganizationMember(ctx context.Context, orgID, userID, role string) error {
	const q = `
INSERT INTO organization_members (id, org_id, user_id, role, status, joined_at)
VALUES (gen_random_uuid()::TEXT, $1, $2, $3, 'active', NOW())
ON CONFLICT (org_id, user_id)
DO UPDATE SET
  role = EXCLUDED.role,
  status = 'active',
  updated_at = NOW()`

	if _, err := r.pool.Exec(ctx, q, orgID, userID, role); err != nil {
		return fmt.Errorf("add organization member: %w", err)
	}
	return nil
}

// AddPendingInvite records an email-based invitation for a user who hasn't registered yet.
func (r *Repository) AddPendingInvite(ctx context.Context, orgID, invitedEmail, role, invitedBy string) (string, error) {
	memberID := "invite_" + fmt.Sprintf("%d", time.Now().UnixNano())
	const q = `
INSERT INTO organization_members (id, org_id, user_id, role, status, invited_by, invited_email, invited_at)
VALUES ($1, $2, $3, $4, 'invited', $5, $6, NOW())
ON CONFLICT (org_id, user_id)
DO UPDATE SET role = EXCLUDED.role, invited_email = EXCLUDED.invited_email, updated_at = NOW()`

	if _, err := r.pool.Exec(ctx, q, memberID, orgID, memberID, role, invitedBy, invitedEmail); err != nil {
		return "", fmt.Errorf("add pending invite: %w", err)
	}
	return memberID, nil
}

// GetOrganizationByTenant returns an organization resolved by provider + tenant id.
func (r *Repository) GetOrganizationByTenant(ctx context.Context, provider, tenantID string) (*Organization, error) {
	const q = `
SELECT o.id, o.name, COALESCE(o.slug, ''), o.plan, o.status, COALESCE(o.primary_domain, ''), COALESCE(o.region, 'eu'), COALESCE(o.default_locale, 'nb-NO'), o.metadata, o.created_at, o.updated_at,
       o.org_number, COALESCE(o.verification_status, 'unverified'), o.brreg_data
FROM organizations o
INNER JOIN org_tenant_links otl ON otl.org_id = o.id
WHERE otl.provider = $1
  AND otl.microsoft_tenant_id = $2
  AND o.deleted_at IS NULL
LIMIT 1`

	out, err := scanOrg(r.pool.QueryRow(ctx, q, provider, tenantID))
	if err != nil {
		if err == pgx.ErrNoRows {
			return nil, ErrNotFound
		}
		return nil, fmt.Errorf("query organization by tenant: %w", err)
	}
	return &out, nil
}

// UpsertOrgTenantLink creates or updates tenant mapping for an organization.
func (r *Repository) UpsertOrgTenantLink(ctx context.Context, link OrgTenantLink) error {
	const q = `
INSERT INTO org_tenant_links (org_id, provider, microsoft_tenant_id, verified, domains, display_name_from_tenant)
VALUES ($1, $2, $3, $4, $5, $6)
ON CONFLICT (provider, microsoft_tenant_id)
DO UPDATE SET
  org_id = EXCLUDED.org_id,
  verified = EXCLUDED.verified,
  domains = EXCLUDED.domains,
  display_name_from_tenant = EXCLUDED.display_name_from_tenant,
  updated_at = NOW()`

	provider := link.Provider
	if provider == "" {
		provider = "microsoft"
	}
	domainsBuf, err := json.Marshal(link.Domains)
	if err != nil {
		return fmt.Errorf("marshal domains: %w", err)
	}

	if _, err := r.pool.Exec(ctx, q, link.OrgID, provider, link.MicrosoftTenantID, link.Verified, domainsBuf, link.DisplayNameFromTenant); err != nil {
		return fmt.Errorf("upsert org tenant link: %w", err)
	}
	return nil
}

// UpsertOrgOnboardingState creates or updates onboarding state for an organization.
func (r *Repository) UpsertOrgOnboardingState(ctx context.Context, state OrgOnboardingState) error {
	const q = `
INSERT INTO org_onboarding_states (org_id, status, steps, last_updated_at)
VALUES ($1, $2, $3, NOW())
ON CONFLICT (org_id)
DO UPDATE SET
  status = EXCLUDED.status,
  steps = EXCLUDED.steps,
  last_updated_at = NOW()`

	steps := map[string]any{}
	if state.Steps != nil {
		steps = state.Steps
	}
	stepsBuf, err := json.Marshal(steps)
	if err != nil {
		return fmt.Errorf("marshal onboarding steps: %w", err)
	}

	if _, err := r.pool.Exec(ctx, q, state.OrgID, state.Status, stepsBuf); err != nil {
		return fmt.Errorf("upsert org onboarding state: %w", err)
	}
	return nil
}

// ListOrganizationMembers returns all active members of an organization.
func (r *Repository) ListOrganizationMembers(ctx context.Context, orgID string) ([]OrgMember, error) {
	const q = `
SELECT id, org_id, user_id, role, status,
       COALESCE(invited_by, ''),
       COALESCE(invited_email, ''),
       COALESCE(joined_at, created_at),
       updated_at
FROM organization_members
WHERE org_id = $1 AND status != 'removed'
ORDER BY COALESCE(joined_at, created_at) ASC`

	rows, err := r.pool.Query(ctx, q, orgID)
	if err != nil {
		return nil, fmt.Errorf("list organization members: %w", err)
	}
	defer rows.Close()

	var members []OrgMember
	for rows.Next() {
		var m OrgMember
		if err := rows.Scan(&m.ID, &m.OrgID, &m.UserID, &m.Role, &m.Status,
			&m.InvitedBy, &m.InvitedEmail, &m.JoinedAt, &m.UpdatedAt); err != nil {
			return nil, fmt.Errorf("scan org member: %w", err)
		}
		members = append(members, m)
	}
	return members, nil
}

// RemoveOrganizationMember soft-removes a member from an organization.
func (r *Repository) RemoveOrganizationMember(ctx context.Context, orgID, userID string) error {
	const q = `
UPDATE organization_members
SET status = 'removed', updated_at = NOW()
WHERE org_id = $1 AND user_id = $2`

	if _, err := r.pool.Exec(ctx, q, orgID, userID); err != nil {
		return fmt.Errorf("remove organization member: %w", err)
	}
	return nil
}
