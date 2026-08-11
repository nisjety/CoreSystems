package org

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"log"
	"strings"
	"time"

	rediscache "github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/redis"
)

var ErrNotFound = errors.New("not found")
var ErrPlanUpgradeRequired = errors.New("plan upgrade required for this feature")
var ErrOwnerConflict = errors.New("organization already has a different owner")
var ErrOrganizationDeleted = errors.New("organization is deleted and cannot be reprovisioned")
var ErrProjectionConflict = errors.New("conflicting state for an existing Auth projection revision")

const (
	orgCacheTTL         = 10 * time.Minute // Orgs change rarely; 10m reduces DB reads
	entitlementCacheTTL = 15 * time.Minute // Entitlements are very stable
)

// Publisher interface for publishing events
type Publisher interface {
	Publish(ctx context.Context, subject string, data map[string]any) error
}

// SharedPublisher is satisfied by *nats.SharedPublisher.
// Defined here (not importing nats) to avoid an import cycle.
type SharedPublisher interface {
	PublishOrgCreated(ctx context.Context, orgID, name, slug, plan string, metadata map[string]any)
	PublishOrgUpdated(ctx context.Context, orgID string, changes map[string]any)
	PublishOrgDeleted(ctx context.Context, orgID, name string)
	PublishPlanChanged(ctx context.Context, orgID, orgName, previousPlan, newPlan, changedBy, reason string, revision int64) error
	PublishInteractiveRetentionEnabled(ctx context.Context, orgID string, eventID int64) error
	PublishMemberAdded(ctx context.Context, orgID, orgName, userID, userEmail, role string)
	PublishMemberRemoved(ctx context.Context, orgID, userID string)
	PublishPlain(subject string, payload map[string]any)
}

// AuditPublisher emits raw audit events over CORE NATS on the control-plane
// bus (controlplane-nats), where audit-core's primary QueueSubscribe listens.
// Satisfied by *nats.Client. Defined here (not importing nats) to avoid an
// import cycle. Kept separate from the cross-plane SharedPublisher (verevon-nats)
// so audit stays CP-local and never depends on the shared bus being up.
type AuditPublisher interface {
	PublishAudit(ctx context.Context, subject, eventID string, payload map[string]any) error
}

// Service handles organization business logic and event publishing
type Service struct {
	repo            *Repository
	publisher       Publisher
	sharedPublisher SharedPublisher    // cross-plane events on verevon-nats
	auditPublisher  AuditPublisher     // durable verevon.audit.v2.* dispatch on local controlplane-nats
	cache           *rediscache.Client // optional, nil if Redis disabled
}

func NewService(repo *Repository, publisher Publisher, cache ...*rediscache.Client) *Service {
	svc := &Service{
		repo:      repo,
		publisher: publisher,
	}
	if len(cache) > 0 {
		svc.cache = cache[0]
	}
	return svc
}

// SetSharedPublisher wires the cross-plane NATS publisher for controlplane.org.* subjects.
func (s *Service) SetSharedPublisher(sp SharedPublisher) {
	s.sharedPublisher = sp
}

// SetAuditPublisher wires the local Control JetStream dispatcher. A nil
// publisher defers delivery; transactional audit intents remain in PostgreSQL.
func (s *Service) SetAuditPublisher(ap AuditPublisher) {
	s.auditPublisher = ap
}

// Ping checks database connectivity + authentication for the /health probe.
func (s *Service) Ping(ctx context.Context) error {
	return s.repo.Ping(ctx)
}

func (s *Service) GetOrganization(ctx context.Context, id string) (*Organization, error) {
	if id == "" {
		return nil, fmt.Errorf("organization id is required")
	}

	// Cache-aside
	if s.cache != nil {
		key := "org:id:" + id
		if cached, err := s.cache.Get(ctx, key); err == nil {
			var org Organization
			if json.Unmarshal([]byte(cached), &org) == nil {
				return &org, nil
			}
		}
	}

	org, err := s.repo.GetOrganization(ctx, id)
	if err != nil {
		return nil, err
	}

	if s.cache != nil {
		if b, merr := json.Marshal(org); merr == nil {
			_ = s.cache.Set(ctx, "org:id:"+id, string(b), orgCacheTTL)
		}
	}

	return org, nil
}

func (s *Service) GetOrganizationWithDetails(ctx context.Context, id string) (*OrganizationWithDetails, error) {
	if id == "" {
		return nil, fmt.Errorf("organization id is required")
	}

	org, err := s.GetOrganization(ctx, id)
	if err != nil {
		return nil, err
	}

	details := &OrganizationWithDetails{
		Organization: *org,
	}

	// Load entitlements
	if entitlements, err := s.repo.GetEntitlements(ctx, id); err == nil {
		details.Entitlements = entitlements
	}

	return details, nil
}

func (s *Service) GetEntitlements(ctx context.Context, id string) ([]Entitlement, error) {
	if id == "" {
		return nil, fmt.Errorf("organization id is required")
	}

	// Cache-aside
	if s.cache != nil {
		key := "org:ent:" + id
		if cached, err := s.cache.Get(ctx, key); err == nil {
			var ents []Entitlement
			if json.Unmarshal([]byte(cached), &ents) == nil {
				return ents, nil
			}
		}
	}

	ents, err := s.repo.GetEntitlements(ctx, id)
	if err != nil {
		return nil, err
	}

	if s.cache != nil {
		if b, merr := json.Marshal(ents); merr == nil {
			_ = s.cache.Set(ctx, "org:ent:"+id, string(b), entitlementCacheTTL)
		}
	}

	// Zero Data Retention is a computed, plan-gated entitlement (never stored).
	// Append it fresh so the UI can render it locked/upsell for ineligible plans
	// without depending on the cached stored-entitlement set.
	if current, err := s.repo.GetOrganization(ctx, id); err == nil {
		ents = append(ents, Entitlement{
			Key:     InteractiveZdrEntitlementKey,
			Enabled: PlanAllowsZeroDataRetention(current.Plan),
		})
	}

	return ents, nil
}

// GetQuotas returns an organization's configured quotas.
//
// Not cached: quotas gate spend, and serving a stale cap after an operator
// lowers one would let an org keep spending past the new ceiling for the cache
// TTL. Entitlements can tolerate that; a limit cannot.
func (s *Service) GetQuotas(ctx context.Context, id string) ([]Quota, error) {
	if id == "" {
		return nil, fmt.Errorf("organization id is required")
	}
	return s.repo.GetQuotas(ctx, id)
}

// SetQuotaLimit sets one quota's ceiling for an organization.
//
// `resetPeriod` may be empty to leave an existing period untouched. A negative
// limit is rejected rather than clamped: it almost certainly means the caller
// computed it wrong, and silently turning it into 0 would block the org
// entirely.
func (s *Service) SetQuotaLimit(
	ctx context.Context, id, key string, limit int64, resetPeriod string,
) (Quota, error) {
	if id == "" {
		return Quota{}, fmt.Errorf("organization id is required")
	}
	key = strings.TrimSpace(key)
	if key == "" {
		return Quota{}, fmt.Errorf("quota key is required")
	}
	if limit < 0 {
		return Quota{}, fmt.Errorf("quota limit must not be negative")
	}
	switch strings.TrimSpace(resetPeriod) {
	case "", "none", "daily", "monthly":
	default:
		return Quota{}, fmt.Errorf("reset_period must be one of: none, daily, monthly")
	}
	return s.repo.SetQuotaLimit(ctx, id, key, limit, strings.TrimSpace(resetPeriod))
}

// ListOrganizations returns a page of non-deleted organizations, ordered by
// creation time descending. Repository.ListOrganizations clamps limit to
// [1, 500]; callers wanting the full set should page with offset until a
// page shorter than the requested limit is returned.
func (s *Service) ListOrganizations(ctx context.Context, limit, offset int) ([]Organization, error) {
	return s.repo.ListOrganizations(ctx, limit, offset)
}

func (s *Service) ListUserOrganizations(ctx context.Context, userID string) ([]Organization, error) {
	if userID == "" {
		return nil, fmt.Errorf("user id is required")
	}
	return s.repo.ListUserOrganizations(ctx, userID)
}

func (s *Service) AddOrganizationMember(ctx context.Context, orgID, userID, role string) error {
	if orgID == "" || userID == "" {
		return fmt.Errorf("organization id and user id are required")
	}
	if role == "" {
		role = "member"
	}
	if err := s.repo.AddOrganizationMember(ctx, orgID, userID, role); err != nil {
		return err
	}

	orgName := ""
	if existing, err := s.repo.GetOrganization(ctx, orgID); err == nil && existing != nil {
		orgName = existing.Name
	}
	s.publishMemberAdded(ctx, orgID, orgName, userID, "", role)
	return nil
}

// ProvisionOrganizationWithOwner commits the organization projection, its
// canonical first owner, default entitlements, and initial onboarding state as
// one atomic org-core write. Events are emitted only after the commit succeeds.
func (s *Service) ProvisionOrganizationWithOwner(
	ctx context.Context,
	organization Organization,
	ownerUserID string,
) error {
	existing, _ := s.repo.GetOrganization(ctx, organization.ID)
	isNew := existing == nil
	if err := s.repo.ProvisionOrganizationWithOwner(ctx, organization, ownerUserID); err != nil {
		return err
	}

	if s.cache != nil {
		_ = s.cache.Del(ctx, "org:id:"+organization.ID, "org:ent:"+organization.ID)
	}
	if isNew {
		s.publishOrganizationCreated(
			ctx,
			organization.ID,
			organization.Name,
			organization.Slug,
			organization.Plan,
			organization.Metadata,
		)
	}
	s.publishMemberAdded(ctx, organization.ID, organization.Name, ownerUserID, "", "owner")
	return nil
}

// ListOrganizationMembers returns all active/invited members for an org.
func (s *Service) ListOrganizationMembers(ctx context.Context, orgID string) ([]OrgMember, error) {
	if orgID == "" {
		return nil, fmt.Errorf("organization id is required")
	}
	return s.repo.ListOrganizationMembers(ctx, orgID)
}

// RemoveOrganizationMember soft-deletes a member from an org.
func (s *Service) RemoveOrganizationMember(ctx context.Context, orgID, userID string) error {
	if orgID == "" || userID == "" {
		return fmt.Errorf("organization id and user id are required")
	}
	if err := s.repo.RemoveOrganizationMember(ctx, orgID, userID); err != nil {
		return err
	}

	orgName := ""
	if existing, err := s.repo.GetOrganization(ctx, orgID); err == nil && existing != nil {
		orgName = existing.Name
	}
	s.publishMemberRemoved(ctx, orgID, orgName, userID)
	return nil
}

func (s *Service) ReconcileOrganizationMember(ctx context.Context, orgID, userID, role, action string, revision int64) (bool, error) {
	return s.repo.ReconcileOrganizationMember(ctx, orgID, userID, role, action, revision)
}

func (s *Service) ReconcileOrganizationProjection(
	ctx context.Context,
	organization Organization,
	ownerUserID string,
	revision int64,
) (bool, error) {
	existing, _ := s.repo.GetOrganization(ctx, organization.ID)
	applied, err := s.repo.ReconcileOrganizationProjection(ctx, organization, ownerUserID, revision)
	if err != nil || !applied {
		return applied, err
	}
	if s.cache != nil {
		_ = s.cache.Del(ctx, "org:id:"+organization.ID, "org:ent:"+organization.ID)
	}
	if existing == nil {
		s.publishOrganizationCreated(
			ctx,
			organization.ID,
			organization.Name,
			organization.Slug,
			organization.Plan,
			organization.Metadata,
		)
	} else {
		s.publishOrganizationUpdated(ctx, organization.ID, map[string]interface{}{
			"name": organization.Name,
			"slug": organization.Slug,
		})
	}
	return true, nil
}

func (s *Service) ReconcileOrganizationDeletion(
	ctx context.Context,
	orgID string,
	revision int64,
) (json.RawMessage, bool, error) {
	receipt, applied, err := s.repo.ReconcileOrganizationDeletion(ctx, orgID, revision)
	if err == nil && s.cache != nil {
		_ = s.cache.Del(ctx, "org:id:"+orgID, "org:ent:"+orgID)
	}
	return receipt, applied, err
}

// AddPendingInvite records an email invite for a user who has not yet registered.
func (s *Service) AddPendingInvite(ctx context.Context, orgID, invitedEmail, role, invitedBy string) (string, error) {
	if orgID == "" || invitedEmail == "" {
		return "", fmt.Errorf("organization id and invited email are required")
	}
	if role == "" {
		role = "member"
	}
	return s.repo.AddPendingInvite(ctx, orgID, invitedEmail, role, invitedBy)
}

func (s *Service) UpsertFromAuthEvent(ctx context.Context, id, name, slug string, metadata map[string]any) error {
	if id == "" || name == "" {
		return fmt.Errorf("organization id and name are required")
	}

	// Creation must carry a canonical owner and use
	// ProvisionOrganizationWithOwner. This legacy method is update-only so it
	// can never violate the database owner invariant.
	existing, err := s.repo.GetOrganization(ctx, id)
	if err != nil || existing == nil {
		return fmt.Errorf("organization must already exist: %w", ErrNotFound)
	}

	existing.Name = name
	existing.Slug = slug
	existing.Metadata = metadata
	if err := s.repo.UpsertOrganization(ctx, *existing); err != nil {
		return err
	}

	if err := s.repo.SetDefaultEntitlements(ctx, id); err != nil {
		return err
	}

	// Invalidate cached org and entitlement data after write
	if s.cache != nil {
		_ = s.cache.Del(ctx, "org:id:"+id, "org:ent:"+id)
	}

	// Publish event
	s.publishOrganizationUpdated(ctx, id, map[string]interface{}{
		"name": name,
		"slug": slug,
	})

	return nil
}

// EnsureOrganizationFromTenant resolves an organization by provider+tenant id,
// creating a new org + tenant link + onboarding state when needed.
func (s *Service) EnsureOrganizationFromTenant(ctx context.Context, provider, tenantID, ownerUserID, displayName, primaryDomain string, domains []string, region, defaultLocale string) (*Organization, bool, error) {
	provider = strings.TrimSpace(provider)
	if provider == "" {
		provider = "microsoft"
	}
	if strings.TrimSpace(tenantID) == "" {
		return nil, false, fmt.Errorf("tenant id is required")
	}
	if strings.TrimSpace(ownerUserID) == "" {
		return nil, false, fmt.Errorf("owner user id is required")
	}

	resolved, err := s.repo.GetOrganizationByTenant(ctx, provider, tenantID)
	if err == nil && resolved != nil {
		return resolved, false, nil
	}
	if err != nil && !errors.Is(err, ErrNotFound) {
		return nil, false, err
	}

	name := strings.TrimSpace(displayName)
	if name == "" {
		if primaryDomain != "" {
			name = primaryDomain
		} else {
			name = "Enterprise Organization"
		}
	}

	orgID := fmt.Sprintf("org_%d", time.Now().UnixNano()/1_000_000)
	org := Organization{
		ID:            orgID,
		Name:          name,
		Plan:          "free",
		Status:        "active",
		PrimaryDomain: primaryDomain,
		Region:        region,
		DefaultLocale: defaultLocale,
	}

	if err := s.ProvisionOrganizationWithOwner(ctx, org, ownerUserID); err != nil {
		return nil, false, err
	}

	// No cached entry yet for a brand-new org, but invalidate in case of re-creation
	if s.cache != nil {
		_ = s.cache.Del(ctx, "org:id:"+orgID, "org:ent:"+orgID)
	}

	if err := s.repo.UpsertOrgTenantLink(ctx, OrgTenantLink{
		OrgID:                 orgID,
		Provider:              provider,
		MicrosoftTenantID:     tenantID,
		Verified:              false,
		Domains:               domains,
		DisplayNameFromTenant: displayName,
	}); err != nil {
		return nil, false, err
	}

	if err := s.repo.UpsertOrgOnboardingState(ctx, OrgOnboardingState{
		OrgID:  orgID,
		Status: "CREATED",
		Steps: map[string]any{
			"tenantLinked": true,
		},
	}); err != nil {
		return nil, false, err
	}

	created, getErr := s.repo.GetOrganization(ctx, orgID)
	if getErr != nil {
		return nil, false, getErr
	}

	s.publishOrganizationCreated(ctx, created.ID, created.Name, created.Slug, created.Plan, created.Metadata)
	return created, true, nil
}

// GetOrganizationByTenant resolves organization mapping by provider + tenant id.
func (s *Service) GetOrganizationByTenant(ctx context.Context, provider, tenantID string) (*Organization, error) {
	provider = strings.TrimSpace(provider)
	if provider == "" {
		provider = "microsoft"
	}
	if strings.TrimSpace(tenantID) == "" {
		return nil, fmt.Errorf("tenant id is required")
	}
	return s.repo.GetOrganizationByTenant(ctx, provider, tenantID)
}

// UpdateOnboardingState upserts onboarding state for an organization.
func (s *Service) UpdateOnboardingState(ctx context.Context, orgID, status string, steps map[string]any) error {
	if strings.TrimSpace(orgID) == "" {
		return fmt.Errorf("organization id is required")
	}
	if strings.TrimSpace(status) == "" {
		status = "CREATED"
	}
	return s.repo.UpsertOrgOnboardingState(ctx, OrgOnboardingState{
		OrgID:  orgID,
		Status: status,
		Steps:  steps,
	})
}

func (s *Service) UpdatePlan(ctx context.Context, orgID, plan, changedBy, reason string) (*Organization, error) {
	if strings.TrimSpace(orgID) == "" {
		return nil, fmt.Errorf("organization id is required")
	}

	plan = strings.ToLower(strings.TrimSpace(plan))

	switch plan {
	case "free", "trial", "hobby", "standard", "pro", "enterprise":
	default:
		return nil, fmt.Errorf("invalid plan")
	}

	_, applied, err := s.repo.UpdatePlanWithOutbox(ctx, orgID, plan, changedBy, reason)
	if err != nil {
		return nil, err
	}

	if applied && s.cache != nil {
		_ = s.cache.Del(ctx, "org:id:"+orgID, "org:ent:"+orgID)
	}
	if _, err := s.FlushPlanChangeOutbox(ctx, 100); err != nil {
		return nil, err
	}
	return s.repo.GetOrganization(ctx, orgID)
}

// ApplyBillingPlanSync mirrors a plan value carried on billing-core's
// billing.account.updated event into this org's plan column. This is the
// org-core side of the sync bug fix: billing-core's EffectivePlan (which
// elevates a plan during an active trial) previously never reached
// organizations.plan, so plan-gated features kept reading the stale,
// never-elevated stored value. Returns skippedOverride=true (not an error)
// when the org has a manual metadata.plan_override pin in effect.
func (s *Service) ApplyBillingPlanSync(ctx context.Context, orgID, plan string) (skippedOverride bool, err error) {
	if strings.TrimSpace(orgID) == "" {
		return false, fmt.Errorf("organization id is required")
	}
	plan = strings.ToLower(strings.TrimSpace(plan))
	switch plan {
	case "free", "trial", "hobby", "standard", "pro", "enterprise":
	default:
		return false, fmt.Errorf("invalid plan")
	}

	applied, skippedOverride, err := s.repo.UpdatePlanFromBillingSync(ctx, orgID, plan)
	if err != nil {
		return false, err
	}
	if applied && s.cache != nil {
		_ = s.cache.Del(ctx, "org:id:"+orgID, "org:ent:"+orgID)
	}
	return skippedOverride, nil
}

// PlanAllowsZeroDataRetention reports whether an organization on the given plan
// may enable interactive Zero Data Retention (a premium, plan-gated feature).
func PlanAllowsZeroDataRetention(plan string) bool {
	return zeroDataRetentionPlans[strings.ToLower(strings.TrimSpace(plan))]
}

// SetInteractiveRetention persists the org's interactive Zero-Data-Retention
// posture. Enabling ZDR (zdr=true) is a premium feature gated by plan — it is
// rejected with ErrPlanUpgradeRequired unless the org's plan is entitled;
// disabling it is always allowed. The stored value is durable org-admin intent;
// live enforcement is applied through auth-core's managed retention policy.
// Returns the refreshed organization projection.
func (s *Service) SetInteractiveRetention(ctx context.Context, orgID string, zdr bool, changedBy string) (*Organization, error) {
	if strings.TrimSpace(orgID) == "" {
		return nil, fmt.Errorf("organization id is required")
	}
	if zdr {
		current, err := s.repo.GetOrganization(ctx, orgID)
		if err != nil {
			return nil, err
		}
		if !PlanAllowsZeroDataRetention(current.Plan) {
			return nil, ErrPlanUpgradeRequired
		}
	}
	enqueued, err := s.repo.SetInteractiveRetention(ctx, orgID, zdr, changedBy)
	if err != nil {
		return nil, err
	}
	if s.cache != nil {
		_ = s.cache.Del(ctx, "org:id:"+orgID, "org:ent:"+orgID)
	}
	if enqueued {
		if _, err := s.FlushInteractiveRetentionOutbox(ctx, 100); err != nil {
			return nil, err
		}
	}
	return s.repo.GetOrganization(ctx, orgID)
}

// FlushInteractiveRetentionOutbox publishes committed false-to-true ZDR
// transitions to the scoped Control Plane stream. A failed publish remains
// pending for a later retry; a duplicate delivery is safe because its sole
// subscriber uses an org-scoped DELETE of personal drafts.
func (s *Service) FlushInteractiveRetentionOutbox(ctx context.Context, limit int) (int, error) {
	if limit < 1 || limit > 1000 {
		return 0, fmt.Errorf("interactive retention outbox limit must be between 1 and 1000")
	}
	if s.sharedPublisher == nil {
		return 0, fmt.Errorf("interactive retention shared event publisher is unavailable")
	}
	if s.repo == nil || s.repo.pool == nil {
		return 0, fmt.Errorf("interactive retention outbox repository is unavailable")
	}
	rows, err := s.repo.ClaimInteractiveRetentionOutbox(ctx, limit)
	if err != nil {
		return 0, err
	}
	published := 0
	var publishErrors error
	for _, row := range rows {
		if err := s.sharedPublisher.PublishInteractiveRetentionEnabled(ctx, row.OrgID, row.EventID); err != nil {
			if markErr := s.repo.MarkInteractiveRetentionPublishFailed(ctx, row.EventID, err); markErr != nil {
				publishErrors = errors.Join(publishErrors, err, markErr)
			} else {
				publishErrors = errors.Join(publishErrors, err)
			}
			continue
		}
		if err := s.repo.MarkInteractiveRetentionPublished(ctx, row.EventID); err != nil {
			publishErrors = errors.Join(publishErrors, err)
			continue
		}
		published++
	}
	return published, publishErrors
}

func (s *Service) SetSupportAIMode(ctx context.Context, orgID, mode, changedBy string) (*Organization, error) {
	if strings.TrimSpace(orgID) == "" {
		return nil, fmt.Errorf("organization id is required")
	}
	mode = strings.ToLower(strings.TrimSpace(mode))
	if mode != "off" && mode != "assist" && mode != "review" {
		return nil, fmt.Errorf("invalid support AI mode")
	}
	if err := s.repo.SetSupportAIMode(ctx, orgID, mode, changedBy); err != nil {
		return nil, err
	}
	if s.cache != nil {
		_ = s.cache.Del(ctx, "org:id:"+orgID)
	}
	return s.repo.GetOrganization(ctx, orgID)
}

// FlushPlanChangeOutbox publishes committed plan intents and acknowledges each
// row only after JetStream confirms the publish. A crash after publish but
// before acknowledgement causes a duplicate delivery, which Billing rejects by
// revision; a publish error remains visible and retryable in the outbox.
func (s *Service) FlushPlanChangeOutbox(ctx context.Context, limit int) (int, error) {
	if limit <= 0 || limit > 1000 {
		return 0, fmt.Errorf("plan outbox limit must be between 1 and 1000")
	}
	if s.publisher == nil {
		return 0, fmt.Errorf("organization plan event publisher is unavailable")
	}
	if s.sharedPublisher == nil {
		return 0, fmt.Errorf("organization shared plan event publisher is unavailable")
	}
	rows, err := s.repo.ClaimPlanChangeOutbox(ctx, limit)
	if err != nil {
		return 0, err
	}
	published := 0
	var publishErrors error
	for _, row := range rows {
		event := map[string]any{
			"organization_id":   row.OrgID,
			"organization_name": row.OrgName,
			"previous_plan":     row.PreviousPlan,
			"new_plan":          row.NewPlan,
			"changed_by":        row.ChangedBy,
			"change_reason":     row.Reason,
			"revision":          row.Revision,
		}
		if err := s.publisher.Publish(ctx, "organization.plan.changed", event); err != nil {
			if markErr := s.repo.MarkPlanChangePublishFailed(ctx, row.OrgID, row.Revision, err); markErr != nil {
				publishErrors = errors.Join(publishErrors, err, markErr)
			} else {
				publishErrors = errors.Join(publishErrors, err)
			}
			continue
		}
		if err := s.publishPlanChanged(ctx, row.PlanChange); err != nil {
			if markErr := s.repo.MarkPlanChangePublishFailed(ctx, row.OrgID, row.Revision, err); markErr != nil {
				publishErrors = errors.Join(publishErrors, err, markErr)
			} else {
				publishErrors = errors.Join(publishErrors, err)
			}
			continue
		}
		if err := s.repo.MarkPlanChangePublished(ctx, row.OrgID, row.Revision); err != nil {
			publishErrors = errors.Join(publishErrors, err)
			continue
		}
		published++
	}
	return published, publishErrors
}

// UpdateBrregVerification sets the org_number, brreg_data, and verification_status
// on an existing organization. Pass verificationStatus = "verified" when the user
// has confirmed the Brreg match; "unverified" is the default.
func (s *Service) UpdateBrregVerification(ctx context.Context, id, orgNumber string, brregData map[string]any, verificationStatus string) error {
	if id == "" {
		return fmt.Errorf("organization id is required")
	}
	existing, err := s.repo.GetOrganization(ctx, id)
	if err != nil {
		return err
	}
	if verificationStatus == "" {
		verificationStatus = "unverified"
	}
	existing.OrgNumber = &orgNumber
	existing.BrregData = brregData
	existing.VerificationStatus = verificationStatus
	if err := s.repo.UpsertOrganization(ctx, *existing); err != nil {
		return err
	}

	// Invalidate cached org data
	if s.cache != nil {
		_ = s.cache.Del(ctx, "org:id:"+id)
	}

	s.publishOrganizationUpdated(ctx, id, map[string]interface{}{
		"org_number":          orgNumber,
		"verification_status": verificationStatus,
	})
	return nil
}

// HardDelete atomically commits the irreversible organization erasure and its
// durable audit intent. Publication is asynchronous and retried from the local
// outbox, so a broker outage cannot create an unaudited successful erasure.
func (s *Service) HardDelete(ctx context.Context, orgID, actorID, actorRole string) (json.RawMessage, error) {
	occurredAt := time.Now().UTC()
	auditEvent, err := newGDPRAuditEvent(
		orgID, "organization", orgID, actorID, actorRole, "ok", occurredAt,
	)
	if err != nil {
		return nil, err
	}

	// Capture the org name before the row is deleted, but do not announce the
	// deletion unless the erasure and its durable audit intent both commit.
	orgName := ""
	if org, err := s.repo.GetOrganization(ctx, orgID); err == nil {
		orgName = org.Name
	}

	receipt, err := s.repo.GDPRHardDeleteOrganization(ctx, orgID, auditEvent)
	if err != nil {
		return nil, errors.Join(err, s.enqueueGDPRErrorAudit(
			ctx, orgID, "organization", actorID, actorRole, occurredAt,
		))
	}
	if orgName != "" {
		s.publishOrganizationDeleted(ctx, orgID, orgName)
	}

	if s.cache != nil {
		_ = s.cache.Del(ctx, "org:id:"+orgID, "org:ent:"+orgID)
	}
	return receipt, nil
}

// SoftDelete atomically marks an organization deleted and records its
// durable audit intent, then opens the 30-day Flow C grace window: it
// records one org_deletion_members ledger row per currently-active member
// and publishes verevon.org.deletion.pending so notification-core can tell
// them. orgName is the caller-supplied "type the org name to confirm" value
// — it is validated here against the organization's real, server-fetched
// name (case-sensitive, exact match) BEFORE anything is deleted; a mismatch
// returns *ErrOrgNameMismatch and no state changes. The name is deliberately
// never trusted from the client for anything other than this confirmation
// check. Reversible soft deletion does not emit the irreversible cross-plane
// erasure fan-out (see HardDelete / PurgeDeletedOrganizations for that).
func (s *Service) SoftDelete(ctx context.Context, orgID, orgName, actorID, actorRole string) (json.RawMessage, error) {
	orgID = strings.TrimSpace(orgID)
	if orgID == "" {
		return nil, fmt.Errorf("organization id is required")
	}

	// Fetch the real name server-side and compare. If the org can't be found
	// at all (already gone, or already past this grace window), skip the
	// comparison and let the existing not-found/already-deleted failure from
	// the proc below surface exactly as it did before this check existed.
	existing, lookupErr := s.repo.GetOrganization(ctx, orgID)
	if lookupErr == nil && existing != nil && existing.Name != orgName {
		return nil, &ErrOrgNameMismatch{OrgID: orgID}
	}

	occurredAt := time.Now().UTC()
	auditEvent, err := newGDPRAuditEvent(
		orgID, "organization_soft", orgID, actorID, actorRole, "ok", occurredAt,
	)
	if err != nil {
		return nil, err
	}
	receipt, err := s.repo.SoftDeleteOrganization(ctx, orgID, auditEvent)
	if err != nil {
		return nil, errors.Join(err, s.enqueueGDPRErrorAudit(
			ctx, orgID, "organization_soft", actorID, actorRole, occurredAt,
		))
	}
	if s.cache != nil {
		_ = s.cache.Del(ctx, "org:id:"+orgID, "org:ent:"+orgID)
	}

	// The soft delete and its audit intent are already durably committed
	// above. Everything below (ledger + pending notice) is best-effort: a
	// failure here is logged, not rolled back, and does not change the
	// response — the org is deleted either way, and CreateDeletionLedger's
	// ON CONFLICT DO NOTHING makes a later retry of this same request safe.
	orgNameForEvent := orgName
	if existing != nil {
		orgNameForEvent = existing.Name
	}

	var memberIDs []string
	if members, mErr := s.repo.ListOrganizationMembers(ctx, orgID); mErr != nil {
		log.Printf("org-core: soft-delete %s: list members for deletion ledger failed: %v", orgID, mErr)
	} else {
		memberIDs = make([]string, 0, len(members))
		for _, m := range members {
			if strings.EqualFold(m.Status, "active") {
				memberIDs = append(memberIDs, m.UserID)
			}
		}
	}

	if len(memberIDs) > 0 {
		if lErr := s.repo.CreateDeletionLedger(ctx, orgID, memberIDs); lErr != nil {
			log.Printf("org-core: soft-delete %s: create deletion ledger failed: %v", orgID, lErr)
		}
	}

	deadline := occurredAt.AddDate(0, 0, deletionGracePeriodDays)
	s.publishDeletionPending(orgID, orgNameForEvent, actorID, deadline, memberIDs)

	return receipt, nil
}

// publishDeletionPending emits verevon.org.deletion.pending. A nil shared
// publisher (verevon-nats disabled) makes this a no-op.
func (s *Service) publishDeletionPending(orgID, orgName, requestedBy string, deadline time.Time, memberUserIDs []string) {
	sp := s.SharedPub()
	if sp == nil {
		return
	}
	sp.PublishPlain("verevon.org.deletion.pending", map[string]any{
		"org_id":          orgID,
		"org_name":        orgName,
		"requested_by":    requestedBy,
		"deadline":        deadline.UTC().Format(time.RFC3339),
		"member_user_ids": memberUserIDs,
	})
}

func (s *Service) enqueueGDPRErrorAudit(
	ctx context.Context,
	orgID, subjectType, actorID, actorRole string,
	occurredAt time.Time,
) error {
	event, err := newGDPRAuditEvent(
		orgID, subjectType, orgID, actorID, actorRole, "error", occurredAt,
	)
	if err != nil {
		return err
	}
	if err := s.repo.EnqueueGDPRAuditEvent(ctx, event); err != nil {
		return fmt.Errorf("record failed GDPR operation audit: %w", err)
	}
	return nil
}

// gdprPurgeReceipt mirrors the purge_old_deleted_organizations($1) stored
// proc's JSONB shape (migrations/003_gdpr_hard_delete.up.sql) closely enough
// to recover the purged org ids for the cross-plane erasure fan-out below.
type gdprPurgeReceipt struct {
	OrgIDs []string `json:"org_ids"`
}

// PurgeDeletedOrganizations hard-deletes organizations soft-deleted more than
// daysThreshold days ago by invoking purge_old_deleted_organizations (param).
// Used by the retention cron. For every organization the sweep actually
// purged, it also publishes GDPRErasureFanoutSubject
// ("verevon.gdpr.erasure.requested") — the same cross-plane contract the
// explicit HTTP hard-delete path emits — so Model Plane / Data Plane purge
// their side regardless of which path triggered the erasure. Returns the
// proc's JSONB receipt verbatim (unchanged from before this fan-out existed).
func (s *Service) PurgeDeletedOrganizations(ctx context.Context, daysThreshold int) (json.RawMessage, error) {
	if daysThreshold < 1 {
		daysThreshold = 1
	}
	receipt, err := s.repo.PurgeOldDeletedOrganizations(ctx, daysThreshold)
	if err != nil {
		return nil, err
	}

	var parsed gdprPurgeReceipt
	if jsonErr := json.Unmarshal(receipt, &parsed); jsonErr != nil {
		log.Printf("org-core: purge sweep: could not parse purge receipt for erasure fan-out: %v", jsonErr)
		return receipt, nil
	}
	for _, orgID := range parsed.OrgIDs {
		if orgID == "" {
			continue
		}
		s.PublishGDPRErasureFanout(orgID, gdprPurgeCronActorID)
		if s.cache != nil {
			_ = s.cache.Del(ctx, "org:id:"+orgID, "org:ent:"+orgID)
		}
	}
	return receipt, nil
}

// CallerRole returns the caller's role within an org (e.g. "owner", "admin"),
// or "" when the caller is not an active member. Used to owner-gate erasure.
func (s *Service) CallerRole(ctx context.Context, orgID, userID string) (string, error) {
	if strings.TrimSpace(orgID) == "" || strings.TrimSpace(userID) == "" {
		return "", nil
	}
	members, err := s.repo.ListOrganizationMembers(ctx, orgID)
	if err != nil {
		return "", err
	}
	for _, m := range members {
		if m.UserID == userID && m.Status == "active" {
			return m.Role, nil
		}
	}
	return "", nil
}

// IsActiveMember reports whether userID is an active member of orgID. It is the
// DB-backed second layer behind the internal-API-key gate: even a caller that
// holds the internal key must prove the path-supplied org belongs to the acting
// user before a mutation is allowed. Backed by an indexed point query (not a
// full member-list scan) since the guard fires on every mutation. Empty org/user
// IDs are non-members (never an error); callers (the guard) pre-validate the IDs
// and this path only guarantees fail-closed behavior for any direct caller.
func (s *Service) IsActiveMember(ctx context.Context, orgID, userID string) (bool, error) {
	if strings.TrimSpace(orgID) == "" || strings.TrimSpace(userID) == "" {
		return false, nil
	}
	return s.repo.IsActiveMember(ctx, orgID, userID)
}

// SharedPub exposes the cross-plane publisher so the GDPR handlers can emit
// the cross-plane erasure fan-out. Returns nil when shared NATS is disabled.
func (s *Service) SharedPub() SharedPublisher {
	return s.sharedPublisher
}

// Event publishing methods

func (s *Service) publishOrganizationCreated(ctx context.Context, orgID, name, slug, plan string, metadata map[string]any) {
	event := map[string]any{
		"organization_id": orgID,
		"name":            name,
		"slug":            slug,
		"plan":            plan,
		"metadata":        metadata,
	}

	if s.publisher != nil {
		s.publisher.Publish(ctx, "organization.created", event)
	}
	if s.sharedPublisher != nil {
		s.sharedPublisher.PublishOrgCreated(ctx, orgID, name, slug, plan, metadata)
	}
}

func (s *Service) publishOrganizationUpdated(ctx context.Context, orgID string, changes map[string]interface{}) {
	event := map[string]any{
		"organization_id": orgID,
		"changes":         changes,
	}

	if s.publisher != nil {
		s.publisher.Publish(ctx, "organization.updated", event)
	}
	if s.sharedPublisher != nil {
		s.sharedPublisher.PublishOrgUpdated(ctx, orgID, changes)
	}
}

func (s *Service) publishOrganizationDeleted(ctx context.Context, orgID, name string) {
	event := map[string]any{
		"organization_id": orgID,
		"name":            name,
	}

	if s.publisher != nil {
		s.publisher.Publish(ctx, "organization.deleted", event)
	}
	if s.sharedPublisher != nil {
		s.sharedPublisher.PublishOrgDeleted(ctx, orgID, name)
	}
}

func (s *Service) publishPlanChanged(ctx context.Context, change PlanChange) error {
	if s.sharedPublisher != nil {
		if err := s.sharedPublisher.PublishPlanChanged(
			ctx, change.OrgID, change.OrgName, change.PreviousPlan, change.NewPlan,
			change.ChangedBy, change.Reason, change.Revision,
		); err != nil {
			return err
		}
		// Notify the user who initiated the plan change (changedBy = user ID).
		if change.ChangedBy != "" {
			s.sharedPublisher.PublishPlain("notifications.billing.plan_changed", map[string]any{
				"subscriberId": change.ChangedBy,
				"orgId":        change.OrgID,
				"orgName":      change.OrgName,
				"previousPlan": change.PreviousPlan,
				"newPlan":      change.NewPlan,
				"revision":     change.Revision,
			})
		}
	}
	return nil
}

func (s *Service) publishMemberAdded(ctx context.Context, orgID, orgName, userID, userEmail, role string) {
	event := map[string]any{
		"organization_id":   orgID,
		"organization_name": orgName,
		"user_id":           userID,
		"user_email":        userEmail,
		"role":              role,
	}

	if s.publisher != nil {
		s.publisher.Publish(ctx, "organization.member.added", event)
	}
	if s.sharedPublisher != nil {
		s.sharedPublisher.PublishMemberAdded(ctx, orgID, orgName, userID, userEmail, role)
	}
}

func (s *Service) publishMemberRemoved(ctx context.Context, orgID, orgName, userID string) {
	event := map[string]any{
		"organization_id":   orgID,
		"organization_name": orgName,
		"user_id":           userID,
	}

	if s.publisher != nil {
		s.publisher.Publish(ctx, "organization.member.removed", event)
	}
	if s.sharedPublisher != nil {
		s.sharedPublisher.PublishMemberRemoved(ctx, orgID, userID)
		s.sharedPublisher.PublishPlain("notifications.org.member_removed", map[string]any{
			"subscriberId": userID,
			"orgId":        orgID,
			"orgName":      orgName,
		})
	}
}
