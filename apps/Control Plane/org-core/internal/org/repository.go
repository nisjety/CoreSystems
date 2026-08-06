package org

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"strings"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/database"
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
	// db carries WithOrgScope, used by the single-org request paths so RLS
	// (migration 009) enforces a hard tenant filter at the DB.
	db *database.DB
	// pool is db.Pool, kept for the genuinely cross-org / multi-org / lookup
	// paths that intentionally run unscoped (superuser, RLS-bypassing).
	pool *pgxpool.Pool
}

func lockOrganizationLifecycle(ctx context.Context, tx pgx.Tx, orgID string) error {
	if _, err := tx.Exec(ctx, `SELECT pg_advisory_xact_lock(hashtextextended($1, 0))`, orgID); err != nil {
		return fmt.Errorf("lock organization lifecycle: %w", err)
	}
	return nil
}

func validateDeletionReceipt(receipt []byte) error {
	var result struct {
		Success bool   `json:"success"`
		Error   string `json:"error"`
	}
	if err := json.Unmarshal(receipt, &result); err != nil {
		return fmt.Errorf("decode deletion receipt: %w", err)
	}
	if !result.Success {
		if strings.TrimSpace(result.Error) == "" {
			result.Error = "organization erasure reported failure"
		}
		return fmt.Errorf("organization erasure failed: %s", result.Error)
	}
	return nil
}

func evaluateDeletionTombstone(
	incomingRevision, storedRevision int64,
	completed bool,
	storedReceipt []byte,
) (bool, []byte, error) {
	switch {
	case incomingRevision < storedRevision:
		return false, nil, fmt.Errorf(
			"%w: deletion revision %d is older than tombstone revision %d",
			ErrProjectionConflict, incomingRevision, storedRevision,
		)
	case incomingRevision > storedRevision:
		return false, nil, fmt.Errorf(
			"%w: organization tombstone revision is %d",
			ErrOrganizationDeleted, storedRevision,
		)
	case !completed:
		return true, nil, nil
	}
	if err := validateDeletionReceipt(storedReceipt); err != nil {
		return false, nil, fmt.Errorf("invalid completed deletion checkpoint: %w", err)
	}
	return false, append([]byte(nil), storedReceipt...), nil
}

func NewRepository(db *database.DB) *Repository {
	return &Repository{db: db, pool: db.Pool}
}

// Ping verifies the database is reachable and this pool can authenticate, by
// round-tripping a trivial query. A stale DB password surfaces here — which a
// port-only healthcheck (`nc -z`) silently misses while the pool limps on old
// connections for reads but fails every new connection for writes.
func (r *Repository) Ping(ctx context.Context) error {
	return r.pool.Ping(ctx)
}

func (r *Repository) GetOrganization(ctx context.Context, id string) (*Organization, error) {
	const q = `
SELECT id, name, COALESCE(slug, ''), plan, status, COALESCE(primary_domain, ''), COALESCE(region, 'eu'), COALESCE(default_locale, 'nb-NO'), metadata, created_at, updated_at,
       org_number, COALESCE(verification_status, 'unverified'), brreg_data
FROM organizations
WHERE id = $1 AND deleted_at IS NULL`

	var out *Organization
	err := r.db.WithOrgScope(ctx, id, func(tx pgx.Tx) error {
		org, err := scanOrg(tx.QueryRow(ctx, q, id))
		if err != nil {
			return err
		}
		out = &org
		return nil
	})
	if err != nil {
		if err == pgx.ErrNoRows {
			return nil, ErrNotFound
		}
		return nil, fmt.Errorf("query org: %w", err)
	}
	return out, nil
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

	return r.db.WithOrgScope(ctx, org.ID, func(tx pgx.Tx) error {
		if _, err := tx.Exec(ctx, q,
			org.ID, org.Name, org.Slug, org.Plan, org.Status, org.PrimaryDomain, region, defaultLocale, metaBuf,
			org.OrgNumber, verificationStatus, brregBuf,
		); err != nil {
			return fmt.Errorf("upsert org: %w", err)
		}
		return nil
	})
}

// UpdatePlanWithOutbox serializes plan changes per organization and commits
// the new plan, its positive monotonic revision, history, and publish intent in
// one transaction. A caller crash after commit cannot lose the event.
func (r *Repository) UpdatePlanWithOutbox(
	ctx context.Context,
	orgID, plan, changedBy, reason string,
) (PlanChange, bool, error) {
	change := PlanChange{OrgID: orgID, NewPlan: plan, ChangedBy: changedBy, Reason: reason}
	applied := false
	err := r.db.WithOrgScope(ctx, orgID, func(tx pgx.Tx) error {
		if err := lockOrganizationLifecycle(ctx, tx, orgID); err != nil {
			return err
		}
		if err := tx.QueryRow(ctx, `
SELECT name, plan, plan_revision
FROM organizations
WHERE id = $1 AND deleted_at IS NULL
FOR UPDATE`, orgID).Scan(&change.OrgName, &change.PreviousPlan, &change.Revision); err != nil {
			if errors.Is(err, pgx.ErrNoRows) {
				return ErrNotFound
			}
			return fmt.Errorf("lock organization plan: %w", err)
		}
		if change.PreviousPlan == plan {
			return nil
		}
		if err := tx.QueryRow(ctx, `
UPDATE organizations
SET plan = $2,
    plan_revision = plan_revision + 1,
    updated_at = NOW()
WHERE id = $1
RETURNING plan_revision`, orgID, plan).Scan(&change.Revision); err != nil {
			return fmt.Errorf("update organization plan revision: %w", err)
		}
		if change.Revision < 1 {
			return fmt.Errorf("organization plan revision must be positive")
		}
		if _, err := tx.Exec(ctx, `
INSERT INTO org_plan_history (
  id, org_id, previous_plan, new_plan, changed_by, change_reason, metadata
) VALUES ($1, $2, $3, $4, $5, $6, jsonb_build_object('revision', $7::bigint))`,
			fmt.Sprintf("%s:plan:%d", orgID, change.Revision), orgID,
			change.PreviousPlan, plan, changedBy, reason, change.Revision,
		); err != nil {
			return fmt.Errorf("record organization plan history: %w", err)
		}
		if _, err := tx.Exec(ctx, `
INSERT INTO organization_plan_change_outbox (
  org_id, revision, organization_name, previous_plan, new_plan, changed_by, change_reason
) VALUES ($1, $2, $3, $4, $5, $6, $7)`,
			orgID, change.Revision, change.OrgName, change.PreviousPlan, plan, changedBy, reason,
		); err != nil {
			return fmt.Errorf("record organization plan change outbox: %w", err)
		}
		applied = true
		return nil
	})
	return change, applied, err
}

// UpdatePlanFromBillingSync mirrors the plan billing-core just persisted for
// an account (its billing.account.updated event) into organizations.plan.
// Unlike UpdatePlanWithOutbox, it never enqueues an
// organization_plan_change_outbox row: billing-core is the origin of this
// change, so echoing "organization.plan.changed" back out would bounce the
// same update in a circle between the two services. An org with
// metadata.plan_override set (a manual pin, e.g. a permanently comped plan)
// is left untouched — skippedOverride reports this so the caller can log it
// distinctly from "already up to date".
func (r *Repository) UpdatePlanFromBillingSync(
	ctx context.Context,
	orgID, plan string,
) (applied bool, skippedOverride bool, err error) {
	err = r.db.WithOrgScope(ctx, orgID, func(tx pgx.Tx) error {
		if err := lockOrganizationLifecycle(ctx, tx, orgID); err != nil {
			return err
		}
		var previousPlan, planOverride string
		if err := tx.QueryRow(ctx, `
SELECT plan, COALESCE(metadata ->> 'plan_override', '')
FROM organizations
WHERE id = $1 AND deleted_at IS NULL
FOR UPDATE`, orgID).Scan(&previousPlan, &planOverride); err != nil {
			if errors.Is(err, pgx.ErrNoRows) {
				return ErrNotFound
			}
			return fmt.Errorf("lock organization plan: %w", err)
		}
		if planOverride != "" {
			skippedOverride = true
			return nil
		}
		if previousPlan == plan {
			return nil
		}
		var revision int64
		if err := tx.QueryRow(ctx, `
UPDATE organizations
SET plan = $2,
    plan_revision = plan_revision + 1,
    updated_at = NOW()
WHERE id = $1
RETURNING plan_revision`, orgID, plan).Scan(&revision); err != nil {
			return fmt.Errorf("update organization plan from billing sync: %w", err)
		}
		if _, err := tx.Exec(ctx, `
INSERT INTO org_plan_history (
  id, org_id, previous_plan, new_plan, changed_by, change_reason, metadata
) VALUES ($1, $2, $3, $4, 'billing-core-sync', 'billing_account_updated', jsonb_build_object('revision', $5::bigint))
ON CONFLICT (id) DO NOTHING`,
			fmt.Sprintf("%s:plan:billing-sync:%d", orgID, revision), orgID, previousPlan, plan, revision,
		); err != nil {
			return fmt.Errorf("record organization plan history: %w", err)
		}
		applied = true
		return nil
	})
	return applied, skippedOverride, err
}

// SetInteractiveRetention persists an organization's interactive Zero-Data-
// Retention posture into organizations.metadata.interactiveRetention. This
// records the org's durable INTENT (zdr=true is the privacy-preserving
// default). Live token-issue enforcement is applied separately through the
// managed, attested retention policy in auth-core; this writer never grants a
// per-request override. The write is a targeted jsonb_set so it never clobbers
// unrelated metadata keys, and it is RLS-scoped to the organization.
func (r *Repository) SetInteractiveRetention(ctx context.Context, orgID string, zdr bool, changedBy string) error {
	payload, err := json.Marshal(map[string]any{
		"zdr":       zdr,
		"updatedBy": changedBy,
		"updatedAt": time.Now().UTC().Format(time.RFC3339),
	})
	if err != nil {
		return fmt.Errorf("marshal interactive retention: %w", err)
	}
	return r.db.WithOrgScope(ctx, orgID, func(tx pgx.Tx) error {
		tag, err := tx.Exec(ctx, `
UPDATE organizations
SET metadata = jsonb_set(COALESCE(metadata, '{}'::jsonb), '{interactiveRetention}', $2::jsonb, true),
    updated_at = NOW()
WHERE id = $1 AND deleted_at IS NULL`, orgID, payload)
		if err != nil {
			return fmt.Errorf("update interactive retention: %w", err)
		}
		if tag.RowsAffected() == 0 {
			return ErrNotFound
		}
		return nil
	})
}

func (r *Repository) ClaimPlanChangeOutbox(ctx context.Context, limit int) ([]PlanChangeOutboxRow, error) {
	if limit < 1 || limit > 1000 {
		return nil, fmt.Errorf("plan change outbox limit must be between 1 and 1000")
	}
	tx, err := r.pool.Begin(ctx)
	if err != nil {
		return nil, fmt.Errorf("begin plan change outbox claim: %w", err)
	}
	defer func() { _ = tx.Rollback(ctx) }()
	rows, err := tx.Query(ctx, `
WITH claimed AS (
  SELECT org_id, revision
  FROM organization_plan_change_outbox
  WHERE published_at IS NULL
    AND (processing_at IS NULL OR processing_at < NOW() - INTERVAL '1 minute')
  ORDER BY created_at, org_id, revision
  LIMIT $1
  FOR UPDATE SKIP LOCKED
)
UPDATE organization_plan_change_outbox o
SET processing_at = NOW(), updated_at = NOW()
FROM claimed
WHERE o.org_id = claimed.org_id AND o.revision = claimed.revision
RETURNING o.org_id, o.organization_name, o.previous_plan, o.new_plan,
          o.changed_by, o.change_reason, o.revision, o.attempts`, limit)
	if err != nil {
		return nil, fmt.Errorf("claim plan change outbox: %w", err)
	}
	defer rows.Close()
	claimed := make([]PlanChangeOutboxRow, 0)
	for rows.Next() {
		var row PlanChangeOutboxRow
		if err := rows.Scan(
			&row.OrgID, &row.OrgName, &row.PreviousPlan, &row.NewPlan,
			&row.ChangedBy, &row.Reason, &row.Revision, &row.Attempts,
		); err != nil {
			return nil, fmt.Errorf("scan plan change outbox: %w", err)
		}
		claimed = append(claimed, row)
	}
	if err := rows.Err(); err != nil {
		return nil, fmt.Errorf("iterate plan change outbox: %w", err)
	}
	if err := tx.Commit(ctx); err != nil {
		return nil, fmt.Errorf("commit plan change outbox claim: %w", err)
	}
	return claimed, nil
}

func (r *Repository) MarkPlanChangePublished(ctx context.Context, orgID string, revision int64) error {
	result, err := r.pool.Exec(ctx, `
UPDATE organization_plan_change_outbox
SET published_at = NOW(), processing_at = NULL, attempts = attempts + 1,
    last_error = NULL, updated_at = NOW()
WHERE org_id = $1 AND revision = $2 AND published_at IS NULL`, orgID, revision)
	if err != nil {
		return fmt.Errorf("mark plan change published: %w", err)
	}
	if result.RowsAffected() != 1 {
		return fmt.Errorf("plan change outbox acknowledgement did not match pending revision")
	}
	return nil
}

func (r *Repository) MarkPlanChangePublishFailed(ctx context.Context, orgID string, revision int64, publishErr error) error {
	message := "unknown publish failure"
	if publishErr != nil {
		message = publishErr.Error()
	}
	if len(message) > 2048 {
		message = message[:2048]
	}
	_, err := r.pool.Exec(ctx, `
UPDATE organization_plan_change_outbox
SET processing_at = NULL, attempts = attempts + 1,
    last_error = $3, updated_at = NOW()
WHERE org_id = $1 AND revision = $2 AND published_at IS NULL`, orgID, revision, message)
	if err != nil {
		return fmt.Errorf("record plan change publish failure: %w", err)
	}
	return nil
}

// ProvisionOrganizationWithOwner creates or repairs an organization projection
// and its first owner in one local transaction. Cross-plane callers may safely
// retry this operation with the canonical Better Auth organization ID.
func (r *Repository) ProvisionOrganizationWithOwner(
	ctx context.Context,
	organization Organization,
	ownerUserID string,
) error {
	if strings.TrimSpace(organization.ID) == "" || strings.TrimSpace(ownerUserID) == "" {
		return fmt.Errorf("organization id and owner user id are required")
	}

	metadata := map[string]any{}
	if organization.Metadata != nil {
		metadata = organization.Metadata
	}
	metadataBuf, err := json.Marshal(metadata)
	if err != nil {
		return fmt.Errorf("marshal metadata: %w", err)
	}

	var brregBuf []byte
	if organization.BrregData != nil {
		brregBuf, err = json.Marshal(organization.BrregData)
		if err != nil {
			return fmt.Errorf("marshal brreg data: %w", err)
		}
	}

	plan := organization.Plan
	if plan == "" {
		plan = "free"
	}
	status := organization.Status
	if status == "" {
		status = "active"
	}
	region := organization.Region
	if region == "" {
		region = "eu"
	}
	locale := organization.DefaultLocale
	if locale == "" {
		locale = "nb-NO"
	}
	verificationStatus := organization.VerificationStatus
	if verificationStatus == "" {
		verificationStatus = "unverified"
	}

	return r.db.WithOrgScope(ctx, organization.ID, func(tx pgx.Tx) error {
		if err := lockOrganizationLifecycle(ctx, tx, organization.ID); err != nil {
			return err
		}
		return r.provisionOrganizationWithOwnerTx(
			ctx, tx, organization, ownerUserID, metadataBuf, brregBuf,
			plan, status, region, locale, verificationStatus,
		)
	})
}

func (r *Repository) provisionOrganizationWithOwnerTx(
	ctx context.Context,
	tx pgx.Tx,
	organization Organization,
	ownerUserID string,
	metadataBuf, brregBuf []byte,
	plan, status, region, locale, verificationStatus string,
) error {
	var tombstoned bool
	if err := tx.QueryRow(ctx,
		`SELECT EXISTS (SELECT 1 FROM auth_organization_tombstones WHERE org_id = $1)`,
		organization.ID,
	).Scan(&tombstoned); err != nil {
		return fmt.Errorf("check organization tombstone: %w", err)
	}
	if tombstoned {
		return ErrOrganizationDeleted
	}

	const upsertOrg = `
INSERT INTO organizations (id, name, slug, plan, status, primary_domain, region, default_locale, metadata, org_number, verification_status, brreg_data)
VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
ON CONFLICT (id) DO UPDATE SET
  name = EXCLUDED.name,
  slug = EXCLUDED.slug,
  metadata = organizations.metadata || EXCLUDED.metadata,
  primary_domain = COALESCE(NULLIF(organizations.primary_domain, ''), EXCLUDED.primary_domain),
  org_number = COALESCE(organizations.org_number, EXCLUDED.org_number),
  verification_status = CASE
    WHEN organizations.verification_status = 'verified' THEN organizations.verification_status
    ELSE EXCLUDED.verification_status
  END,
  brreg_data = COALESCE(organizations.brreg_data, EXCLUDED.brreg_data),
  updated_at = NOW()
WHERE organizations.deleted_at IS NULL`
	result, err := tx.Exec(ctx, upsertOrg,
		organization.ID, organization.Name, organization.Slug, plan, status,
		organization.PrimaryDomain, region, locale, metadataBuf,
		organization.OrgNumber, verificationStatus, brregBuf,
	)
	if err != nil {
		return fmt.Errorf("provision organization: %w", err)
	}
	if result.RowsAffected() == 0 {
		return ErrOrganizationDeleted
	}

	const existingOwner = `
SELECT user_id
FROM organization_members
WHERE org_id = $1 AND status = 'active'
  AND 'owner' = ANY(string_to_array(role, ','))
FOR UPDATE`
	rows, err := tx.Query(ctx, existingOwner, organization.ID)
	if err != nil {
		return fmt.Errorf("query existing owner: %w", err)
	}
	ownerMatches := false
	hasOwner := false
	for rows.Next() {
		hasOwner = true
		var existingUserID string
		if err := rows.Scan(&existingUserID); err != nil {
			rows.Close()
			return fmt.Errorf("scan existing owner: %w", err)
		}
		ownerMatches = ownerMatches || existingUserID == ownerUserID
	}
	rows.Close()
	if err := rows.Err(); err != nil {
		return fmt.Errorf("iterate existing owners: %w", err)
	}
	if hasOwner && !ownerMatches {
		return ErrOwnerConflict
	}

	const upsertOwner = `
INSERT INTO organization_members (id, org_id, user_id, role, status, joined_at)
VALUES (gen_random_uuid()::TEXT, $1, $2, 'owner', 'active', NOW())
ON CONFLICT (org_id, user_id) DO UPDATE SET
  role = 'owner', status = 'active', joined_at = COALESCE(organization_members.joined_at, NOW()), updated_at = NOW()`
	if _, err := tx.Exec(ctx, upsertOwner, organization.ID, ownerUserID); err != nil {
		return fmt.Errorf("provision owner: %w", err)
	}

	const upsertEntitlement = `
INSERT INTO org_entitlements (org_id, entitlement_key, enabled)
VALUES ($1, $2, $3)
ON CONFLICT (org_id, entitlement_key) DO NOTHING`
	for _, entitlement := range defaultEntitlements {
		if _, err := tx.Exec(ctx, upsertEntitlement, organization.ID, entitlement.Key, entitlement.Enabled); err != nil {
			return fmt.Errorf("provision entitlements: %w", err)
		}
	}

	const upsertOnboarding = `
INSERT INTO org_onboarding_states (org_id, status, steps, last_updated_at)
VALUES ($1, 'PROFILE_READY', '{"ownerProvisioned":true}'::jsonb, NOW())
ON CONFLICT (org_id) DO UPDATE SET
  status = CASE
    WHEN org_onboarding_states.status = 'COMPLETED' THEN org_onboarding_states.status
    ELSE 'PROFILE_READY'
  END,
  steps = org_onboarding_states.steps || EXCLUDED.steps,
  last_updated_at = NOW()`
	if _, err := tx.Exec(ctx, upsertOnboarding, organization.ID); err != nil {
		return fmt.Errorf("provision onboarding state: %w", err)
	}

	if organization.PrimaryDomain != "" {
		const addPendingDomain = `
INSERT INTO organization_domains (org_id, normalized_domain, status, verification_method, auto_invite_enabled)
VALUES ($1, $2, 'pending', 'authenticated_email', false)
ON CONFLICT (org_id, normalized_domain) DO NOTHING`
		if _, err := tx.Exec(ctx, addPendingDomain, organization.ID, organization.PrimaryDomain); err != nil {
			return fmt.Errorf("record pending organization domain: %w", err)
		}
	}

	return nil
}

// ReconcileOrganizationProjection applies an Auth-owned organization snapshot
// only when its revision is newer than the last committed projection. The
// revision compare, organization/owner upsert, and defaults share one
// transaction so a failed write remains retryable and delayed delivery cannot
// restore stale state.
func (r *Repository) ReconcileOrganizationProjection(
	ctx context.Context,
	organization Organization,
	ownerUserID string,
	revision int64,
) (bool, error) {
	if revision < 1 {
		return false, fmt.Errorf("revision must be positive")
	}
	if strings.TrimSpace(organization.ID) == "" || strings.TrimSpace(ownerUserID) == "" {
		return false, fmt.Errorf("organization id and owner user id are required")
	}

	metadata := map[string]any{}
	if organization.Metadata != nil {
		metadata = organization.Metadata
	}
	metadataBuf, err := json.Marshal(metadata)
	if err != nil {
		return false, fmt.Errorf("marshal metadata: %w", err)
	}
	var brregBuf []byte
	if organization.BrregData != nil {
		brregBuf, err = json.Marshal(organization.BrregData)
		if err != nil {
			return false, fmt.Errorf("marshal brreg data: %w", err)
		}
	}
	plan := organization.Plan
	if plan == "" {
		plan = "free"
	}
	status := organization.Status
	if status == "" {
		status = "active"
	}
	region := organization.Region
	if region == "" {
		region = "eu"
	}
	locale := organization.DefaultLocale
	if locale == "" {
		locale = "nb-NO"
	}
	verificationStatus := organization.VerificationStatus
	if verificationStatus == "" {
		verificationStatus = "unverified"
	}

	applied := false
	err = r.db.WithOrgScope(ctx, organization.ID, func(tx pgx.Tx) error {
		if err := lockOrganizationLifecycle(ctx, tx, organization.ID); err != nil {
			return err
		}
		result, err := tx.Exec(ctx, `
INSERT INTO auth_organization_projection_versions (
  org_id, revision, desired_name, desired_slug,
  desired_owner_user_id, desired_metadata
)
VALUES ($1, $2, $3, $4, $5, $6::JSONB)
ON CONFLICT (org_id) DO UPDATE SET
  revision = EXCLUDED.revision,
  desired_name = EXCLUDED.desired_name,
  desired_slug = EXCLUDED.desired_slug,
  desired_owner_user_id = EXCLUDED.desired_owner_user_id,
  desired_metadata = EXCLUDED.desired_metadata,
  applied_at = NOW()
WHERE auth_organization_projection_versions.revision < EXCLUDED.revision`,
			organization.ID, revision, organization.Name, organization.Slug,
			ownerUserID, metadataBuf)
		if err != nil {
			return fmt.Errorf("record organization projection revision: %w", err)
		}
		if result.RowsAffected() == 0 {
			var storedRevision int64
			var identical bool
			if err := tx.QueryRow(ctx, `
SELECT revision,
       desired_name IS NOT NULL AND
       desired_owner_user_id IS NOT NULL AND
       desired_metadata IS NOT NULL AND
       desired_name = $2 AND
       COALESCE(desired_slug, '') = $3 AND
       desired_owner_user_id = $4 AND
       desired_metadata = $5::JSONB
FROM auth_organization_projection_versions
WHERE org_id = $1
FOR UPDATE`, organization.ID, organization.Name, organization.Slug,
				ownerUserID, metadataBuf).Scan(&storedRevision, &identical); err != nil {
				return fmt.Errorf("read organization projection revision: %w", err)
			}
			if storedRevision == revision && !identical {
				return ErrProjectionConflict
			}
			return nil
		}
		if err := r.provisionOrganizationWithOwnerTx(
			ctx, tx, organization, ownerUserID, metadataBuf, brregBuf,
			plan, status, region, locale, verificationStatus,
		); err != nil {
			return err
		}
		applied = true
		return nil
	})
	return applied, err
}

func (r *Repository) SetDefaultEntitlements(ctx context.Context, orgID string) error {
	const q = `
INSERT INTO org_entitlements (org_id, entitlement_key, enabled)
VALUES ($1, $2, $3)
ON CONFLICT (org_id, entitlement_key)
DO NOTHING`
	return r.db.WithOrgScope(ctx, orgID, func(tx pgx.Tx) error {
		// Batch all inserts into one round-trip instead of N sequential Exec calls
		batch := &pgx.Batch{}
		for _, ent := range defaultEntitlements {
			batch.Queue(q, orgID, ent.Key, ent.Enabled)
		}
		results := tx.SendBatch(ctx, batch)
		defer results.Close()
		for range defaultEntitlements {
			if _, err := results.Exec(); err != nil {
				return fmt.Errorf("set default entitlements: %w", err)
			}
		}
		return nil
	})
}

func (r *Repository) GetEntitlements(ctx context.Context, orgID string) ([]Entitlement, error) {
	const q = `
SELECT entitlement_key, enabled
FROM org_entitlements
WHERE org_id = $1
ORDER BY entitlement_key`

	var items []Entitlement
	err := r.db.WithOrgScope(ctx, orgID, func(tx pgx.Tx) error {
		rows, err := tx.Query(ctx, q, orgID)
		if err != nil {
			return fmt.Errorf("query entitlements: %w", err)
		}
		defer rows.Close()

		items = make([]Entitlement, 0, 8)
		for rows.Next() {
			var e Entitlement
			if err := rows.Scan(&e.Key, &e.Enabled); err != nil {
				return fmt.Errorf("scan entitlements: %w", err)
			}
			items = append(items, e)
		}
		return rows.Err()
	})
	if err != nil {
		return nil, err
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

	return r.db.WithOrgScope(ctx, orgID, func(tx pgx.Tx) error {
		if _, err := tx.Exec(ctx, q, orgID, userID, role); err != nil {
			return fmt.Errorf("add organization member: %w", err)
		}
		return nil
	})
}

// AddPendingInvite records an email-based invitation for a user who hasn't registered yet.
func (r *Repository) AddPendingInvite(ctx context.Context, orgID, invitedEmail, role, invitedBy string) (string, error) {
	memberID := "invite_" + fmt.Sprintf("%d", time.Now().UnixNano())
	const q = `
INSERT INTO organization_members (id, org_id, user_id, role, status, invited_by, invited_email, invited_at)
VALUES ($1, $2, $3, $4, 'invited', $5, $6, NOW())
ON CONFLICT (org_id, user_id)
DO UPDATE SET role = EXCLUDED.role, invited_email = EXCLUDED.invited_email, updated_at = NOW()`

	err := r.db.WithOrgScope(ctx, orgID, func(tx pgx.Tx) error {
		if _, err := tx.Exec(ctx, q, memberID, orgID, memberID, role, invitedBy, invitedEmail); err != nil {
			return fmt.Errorf("add pending invite: %w", err)
		}
		return nil
	})
	if err != nil {
		return "", err
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

	return r.db.WithOrgScope(ctx, link.OrgID, func(tx pgx.Tx) error {
		if _, err := tx.Exec(ctx, q, link.OrgID, provider, link.MicrosoftTenantID, link.Verified, domainsBuf, link.DisplayNameFromTenant); err != nil {
			return fmt.Errorf("upsert org tenant link: %w", err)
		}
		return nil
	})
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

	return r.db.WithOrgScope(ctx, state.OrgID, func(tx pgx.Tx) error {
		if _, err := tx.Exec(ctx, q, state.OrgID, state.Status, stepsBuf); err != nil {
			return fmt.Errorf("upsert org onboarding state: %w", err)
		}
		return nil
	})
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

	var members []OrgMember
	err := r.db.WithOrgScope(ctx, orgID, func(tx pgx.Tx) error {
		rows, err := tx.Query(ctx, q, orgID)
		if err != nil {
			return fmt.Errorf("list organization members: %w", err)
		}
		defer rows.Close()

		for rows.Next() {
			var m OrgMember
			if err := rows.Scan(&m.ID, &m.OrgID, &m.UserID, &m.Role, &m.Status,
				&m.InvitedBy, &m.InvitedEmail, &m.JoinedAt, &m.UpdatedAt); err != nil {
				return fmt.Errorf("scan org member: %w", err)
			}
			members = append(members, m)
		}
		return rows.Err()
	})
	if err != nil {
		return nil, err
	}
	return members, nil
}

// IsActiveMember reports whether userID is an ACTIVE member of orgID using a
// single indexed point query (idx_org_members_user_status covers this), so the
// membership-recheck guard does not pay an O(N) member-list scan on every
// mutation. Parameterized — orgID/userID are never interpolated.
func (r *Repository) IsActiveMember(ctx context.Context, orgID, userID string) (bool, error) {
	const q = `
SELECT EXISTS (
  SELECT 1 FROM organization_members
  WHERE org_id = $1 AND user_id = $2 AND status = 'active'
)`
	var exists bool
	err := r.db.WithOrgScope(ctx, orgID, func(tx pgx.Tx) error {
		if err := tx.QueryRow(ctx, q, orgID, userID).Scan(&exists); err != nil {
			return fmt.Errorf("check active membership: %w", err)
		}
		return nil
	})
	if err != nil {
		return false, err
	}
	return exists, nil
}

// RemoveOrganizationMember soft-removes a member from an organization.
func (r *Repository) RemoveOrganizationMember(ctx context.Context, orgID, userID string) error {
	const q = `
UPDATE organization_members
SET status = 'removed', updated_at = NOW()
WHERE org_id = $1 AND user_id = $2`

	return r.db.WithOrgScope(ctx, orgID, func(tx pgx.Tx) error {
		if _, err := tx.Exec(ctx, q, orgID, userID); err != nil {
			return fmt.Errorf("remove organization member: %w", err)
		}
		return nil
	})
}

// PromoteMember assigns role (owner|admin) to an EXISTING active member as
// part of an admin-succession handoff: a departing sole owner/admin nominates
// a successor before self-erasing (see internal/http/succession_handlers.go
// and user-core's Service.EnsureSuccession, which calls this via org-core's
// internal succession endpoint before the erasure saga starts).
//
// This is deliberately narrower than rbac.Repository.AssignMemberRole: it
// never invites, never removes, and never touches anyone other than the named
// user. It runs inside WithOrgScope like every other membership mutation, so
// the DB-level owner-invariant constraint trigger
// (migrations/010_owner_invariant.up.sql) still applies at commit — promoting
// a successor can only ever add an owner/admin, so that trigger can never
// reject this specific call, but the transaction boundary is kept consistent
// with every other membership write in this file.
func (r *Repository) PromoteMember(ctx context.Context, orgID, userID, role string) error {
	if role != "owner" && role != "admin" {
		return fmt.Errorf("role must be owner or admin")
	}
	const q = `
UPDATE organization_members
SET role = $3, updated_at = NOW()
WHERE org_id = $1 AND user_id = $2 AND status = 'active'`

	return r.db.WithOrgScope(ctx, orgID, func(tx pgx.Tx) error {
		result, err := tx.Exec(ctx, q, orgID, userID, role)
		if err != nil {
			return fmt.Errorf("promote member: %w", err)
		}
		if result.RowsAffected() == 0 {
			return fmt.Errorf("no active membership found for user %s in organization %s", userID, orgID)
		}
		return nil
	})
}

// ReconcileOrganizationMember applies a canonical Auth Core membership intent
// only when its per-member revision is newer than the last applied revision.
// Version check and membership mutation share one transaction, preventing a
// delayed request from restoring stale authorization.
func (r *Repository) ReconcileOrganizationMember(
	ctx context.Context,
	orgID, userID, role, action string,
	revision int64,
) (bool, error) {
	if revision < 1 {
		return false, fmt.Errorf("revision must be positive")
	}
	if action == "remove" {
		role = ""
	} else if role == "" {
		role = "member"
	}

	applied := false
	err := r.db.WithOrgScope(ctx, orgID, func(tx pgx.Tx) error {
		if err := lockOrganizationLifecycle(ctx, tx, orgID); err != nil {
			return err
		}
		var tombstoned bool
		if err := tx.QueryRow(ctx,
			`SELECT EXISTS (SELECT 1 FROM auth_organization_tombstones WHERE org_id = $1)`,
			orgID,
		).Scan(&tombstoned); err != nil {
			return fmt.Errorf("check organization tombstone: %w", err)
		}
		if tombstoned {
			return nil
		}

		result, err := tx.Exec(ctx, `
INSERT INTO auth_membership_projection_versions (
  org_id, user_id, revision, desired_action, desired_role
)
VALUES ($1, $2, $3, $4, NULLIF($5, ''))
ON CONFLICT (org_id, user_id) DO UPDATE SET
  revision = EXCLUDED.revision,
  desired_action = EXCLUDED.desired_action,
  desired_role = EXCLUDED.desired_role,
  applied_at = NOW()
WHERE auth_membership_projection_versions.revision < EXCLUDED.revision`,
			orgID, userID, revision, action, role)
		if err != nil {
			return fmt.Errorf("record membership projection revision: %w", err)
		}
		if result.RowsAffected() == 0 {
			var storedRevision int64
			var storedAction, storedRole string
			if err := tx.QueryRow(ctx, `
SELECT revision, desired_action, COALESCE(desired_role, '')
FROM auth_membership_projection_versions
WHERE org_id = $1 AND user_id = $2
FOR UPDATE`, orgID, userID).Scan(
				&storedRevision, &storedAction, &storedRole,
			); err != nil {
				return fmt.Errorf("read membership projection revision: %w", err)
			}
			if storedRevision == revision &&
				(storedAction != action || storedRole != role) {
				return ErrProjectionConflict
			}
			return nil
		}

		switch action {
		case "upsert":
			_, err = tx.Exec(ctx, `
INSERT INTO organization_members (id, org_id, user_id, role, status, joined_at)
VALUES (gen_random_uuid()::TEXT, $1, $2, $3, 'active', NOW())
ON CONFLICT (org_id, user_id) DO UPDATE SET
  role = EXCLUDED.role, status = 'active', updated_at = NOW()`, orgID, userID, role)
		case "remove":
			_, err = tx.Exec(ctx, `
UPDATE organization_members SET status = 'removed', updated_at = NOW()
WHERE org_id = $1 AND user_id = $2`, orgID, userID)
		}
		if err != nil {
			return fmt.Errorf("apply membership projection: %w", err)
		}
		applied = true
		return nil
	})
	return applied, err
}

// ReconcileOrganizationDeletion commits a permanent Auth tombstone before it
// erases the local projection. The independent tombstone survives an erasure
// failure and prevents delayed create/member delivery from restoring authority.
func (r *Repository) ReconcileOrganizationDeletion(
	ctx context.Context,
	orgID string,
	revision int64,
) (json.RawMessage, bool, error) {
	orgID = strings.TrimSpace(orgID)
	if orgID == "" {
		return nil, false, fmt.Errorf("organization id is required")
	}
	if revision < 1 || revision > MaxSafeAuthRevision {
		return nil, false, fmt.Errorf("revision must be a positive safe integer")
	}

	var completedReceipt []byte
	resumeErasure := false
	// Persist the anti-resurrection tombstone first and independently. If the
	// downstream erasure reports a semantic failure, retries remain safe and no
	// delayed create/member event can restore authorization in the meantime.
	if err := r.db.WithOrgScope(ctx, orgID, func(tx pgx.Tx) error {
		if err := lockOrganizationLifecycle(ctx, tx, orgID); err != nil {
			return err
		}

		var storedRevision int64
		var completedAt *time.Time
		var storedReceipt []byte
		err := tx.QueryRow(ctx, `
SELECT revision, erasure_completed_at, deletion_receipt
FROM auth_organization_tombstones
WHERE org_id = $1
FOR UPDATE`, orgID).Scan(&storedRevision, &completedAt, &storedReceipt)
		if err == nil {
			resume, checkpoint, err := evaluateDeletionTombstone(
				revision, storedRevision, completedAt != nil, storedReceipt,
			)
			if err != nil {
				return err
			}
			resumeErasure = resume
			completedReceipt = checkpoint
			return nil
		}
		if !errors.Is(err, pgx.ErrNoRows) {
			return fmt.Errorf("read organization tombstone: %w", err)
		}

		var projectionRevision int64
		err = tx.QueryRow(ctx, `
SELECT revision
FROM auth_organization_projection_versions
WHERE org_id = $1
FOR UPDATE`, orgID).Scan(&projectionRevision)
		if err == nil && revision <= projectionRevision {
			return fmt.Errorf("%w: deletion revision %d must be newer than projection revision %d", ErrProjectionConflict, revision, projectionRevision)
		}
		if err != nil && !errors.Is(err, pgx.ErrNoRows) {
			return fmt.Errorf("read organization projection revision: %w", err)
		}

		if _, err := tx.Exec(ctx, `
INSERT INTO auth_organization_tombstones (org_id, revision)
VALUES ($1, $2)`, orgID, revision); err != nil {
			return fmt.Errorf("record organization tombstone: %w", err)
		}
		resumeErasure = true
		return nil
	}); err != nil {
		return nil, false, err
	}
	if !resumeErasure {
		return json.RawMessage(completedReceipt), false, nil
	}

	var receipt []byte
	applied := false
	err := r.db.WithOrgScope(ctx, orgID, func(tx pgx.Tx) error {
		if err := lockOrganizationLifecycle(ctx, tx, orgID); err != nil {
			return err
		}

		var storedRevision int64
		var completedAt *time.Time
		var storedReceipt []byte
		if err := tx.QueryRow(ctx, `
SELECT revision, erasure_completed_at, deletion_receipt
FROM auth_organization_tombstones
WHERE org_id = $1
FOR UPDATE`, orgID).Scan(&storedRevision, &completedAt, &storedReceipt); err != nil {
			return fmt.Errorf("lock organization tombstone: %w", err)
		}
		resume, checkpoint, err := evaluateDeletionTombstone(
			revision, storedRevision, completedAt != nil, storedReceipt,
		)
		if err != nil {
			return err
		}
		if !resume {
			receipt = checkpoint
			return nil
		}

		if err := tx.QueryRow(ctx, `SELECT gdpr_hard_delete_organization($1)`, orgID).Scan(&receipt); err != nil {
			return fmt.Errorf("gdpr_hard_delete_organization: %w", err)
		}
		if err := validateDeletionReceipt(receipt); err != nil {
			return err
		}
		result, err := tx.Exec(ctx, `
UPDATE auth_organization_tombstones
SET erasure_completed_at = NOW(), deletion_receipt = $3::JSONB
WHERE org_id = $1 AND revision = $2
  AND erasure_completed_at IS NULL AND deletion_receipt IS NULL`,
			orgID, revision, receipt)
		if err != nil {
			return fmt.Errorf("checkpoint organization erasure: %w", err)
		}
		if result.RowsAffected() != 1 {
			return fmt.Errorf("organization erasure checkpoint did not match revision")
		}
		applied = true
		return nil
	})
	return json.RawMessage(receipt), applied, err
}

// ============================================
// GDPR ERASURE — call the stored procedures defined in
// migrations/003_gdpr_hard_delete.up.sql. All calls are parameterized
// ($1) — the org id is NEVER string-interpolated into the SQL.
// ============================================

// GDPRHardDeleteOrganization invokes gdpr_hard_delete_organization($1), which
// cascades a DELETE across all org-owned tables and returns a JSONB receipt.
// The receipt is returned verbatim to the caller (it records exactly which
// rows were removed — the GDPR erasure audit trail).
func (r *Repository) GDPRHardDeleteOrganization(
	ctx context.Context,
	orgID string,
	auditEvent GDPRAuditEvent,
) (json.RawMessage, error) {
	return r.executeGDPRAuditOperation(
		ctx, orgID, `SELECT gdpr_hard_delete_organization($1)`, auditEvent,
	)
}

// SoftDeleteOrganization invokes soft_delete_organization($1), which sets
// deleted_at + status='deleted' so the row is purged later by the retention
// sweep. Returns the proc's JSONB receipt verbatim.
func (r *Repository) SoftDeleteOrganization(
	ctx context.Context,
	orgID string,
	auditEvent GDPRAuditEvent,
) (json.RawMessage, error) {
	return r.executeGDPRAuditOperation(
		ctx, orgID, `SELECT soft_delete_organization($1)`, auditEvent,
	)
}

// PurgeOldDeletedOrganizations invokes purge_old_deleted_organizations($1),
// hard-deleting organizations soft-deleted more than daysThreshold days ago.
// Returns the proc's JSONB receipt ({ purged_count, org_ids, ... }) verbatim.
func (r *Repository) PurgeOldDeletedOrganizations(ctx context.Context, daysThreshold int) (json.RawMessage, error) {
	var receipt []byte
	err := r.pool.QueryRow(ctx, `SELECT purge_old_deleted_organizations($1)`, daysThreshold).Scan(&receipt)
	if err != nil {
		return nil, fmt.Errorf("purge_old_deleted_organizations: %w", err)
	}
	return json.RawMessage(receipt), nil
}
