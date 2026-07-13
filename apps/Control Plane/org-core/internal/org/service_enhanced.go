package org

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"strings"
	"time"

	rediscache "github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/redis"
)

var ErrNotFound = errors.New("not found")
var ErrOwnerConflict = errors.New("organization already has a different owner")
var ErrOrganizationDeleted = errors.New("organization is deleted and cannot be reprovisioned")

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
	PublishPlanChanged(ctx context.Context, orgID, orgName, previousPlan, newPlan, changedBy, reason string)
	PublishMemberAdded(ctx context.Context, orgID, orgName, userID, userEmail, role string)
	PublishMemberRemoved(ctx context.Context, orgID, userID string)
	PublishPlain(subject string, payload map[string]any)
}

// AuditPublisher emits raw audit events over CORE NATS on the control-plane
// bus (controlplane-nats), where audit-core's primary QueueSubscribe listens.
// Satisfied by *nats.Client. Defined here (not importing nats) to avoid an
// import cycle. Kept separate from the cross-plane SharedPublisher (velion-nats)
// so audit stays CP-local and never depends on the shared bus being up.
type AuditPublisher interface {
	PublishCore(subject string, payload map[string]any) error
}

// Service handles organization business logic and event publishing
type Service struct {
	repo            *Repository
	publisher       Publisher
	sharedPublisher SharedPublisher    // cross-plane events on velion-nats
	auditPublisher  AuditPublisher     // velion.audit.v1.* on the local controlplane-nats bus
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

// SetAuditPublisher wires the local control-plane bus publisher used to emit
// velion.audit.v1.* events to audit-core. nil disables audit emission.
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

	return ents, nil
}

func (s *Service) ListOrganizations(ctx context.Context) ([]Organization, error) {
	// Default to first 100 orgs; callers can add explicit pagination if needed
	return s.repo.ListOrganizations(ctx, 100, 0)
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

func (s *Service) ReconcileOrganizationDeletion(ctx context.Context, orgID string) (json.RawMessage, error) {
	receipt, err := s.repo.ReconcileOrganizationDeletion(ctx, orgID)
	if err == nil && s.cache != nil {
		_ = s.cache.Del(ctx, "org:id:"+orgID, "org:ent:"+orgID)
	}
	return receipt, err
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

	existing, err := s.repo.GetOrganization(ctx, orgID)
	if err != nil {
		return nil, err
	}

	previousPlan := existing.Plan
	if previousPlan == plan {
		return existing, nil
	}

	existing.Plan = plan
	if err := s.repo.UpsertOrganization(ctx, *existing); err != nil {
		return nil, err
	}

	if s.cache != nil {
		_ = s.cache.Del(ctx, "org:id:"+orgID, "org:ent:"+orgID)
	}

	s.publishPlanChanged(ctx, orgID, existing.Name, previousPlan, plan, changedBy, reason)
	return s.repo.GetOrganization(ctx, orgID)
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

// HardDelete performs an irreversible GDPR hard delete of an organization by
// invoking the gdpr_hard_delete_organization stored procedure (parameterized).
// It publishes the org.deleted domain event first (so subscribers see the org
// name before it is gone), then returns the proc's JSONB receipt. Erasure
// auditing + the cross-plane fan-out are emitted by the caller (see gdpr.go).
func (s *Service) HardDelete(ctx context.Context, orgID string) (json.RawMessage, error) {
	if strings.TrimSpace(orgID) == "" {
		return nil, fmt.Errorf("organization id is required")
	}

	// Capture the org name for the domain event before the row is deleted.
	if org, err := s.repo.GetOrganization(ctx, orgID); err == nil {
		s.publishOrganizationDeleted(ctx, orgID, org.Name)
	}

	receipt, err := s.repo.GDPRHardDeleteOrganization(ctx, orgID)
	if err != nil {
		return nil, err
	}

	if s.cache != nil {
		_ = s.cache.Del(ctx, "org:id:"+orgID, "org:ent:"+orgID)
	}
	return receipt, nil
}

// SoftDelete marks an organization deleted (reversible until purged) by
// invoking the soft_delete_organization stored procedure (parameterized).
// Returns the proc's JSONB receipt.
func (s *Service) SoftDelete(ctx context.Context, orgID string) (json.RawMessage, error) {
	if strings.TrimSpace(orgID) == "" {
		return nil, fmt.Errorf("organization id is required")
	}
	receipt, err := s.repo.SoftDeleteOrganization(ctx, orgID)
	if err != nil {
		return nil, err
	}
	if s.cache != nil {
		_ = s.cache.Del(ctx, "org:id:"+orgID, "org:ent:"+orgID)
	}
	return receipt, nil
}

// PurgeDeletedOrganizations hard-deletes organizations soft-deleted more than
// daysThreshold days ago by invoking purge_old_deleted_organizations (param).
// Used by the retention cron. Returns the proc's JSONB receipt.
func (s *Service) PurgeDeletedOrganizations(ctx context.Context, daysThreshold int) (json.RawMessage, error) {
	if daysThreshold < 1 {
		daysThreshold = 1
	}
	return s.repo.PurgeOldDeletedOrganizations(ctx, daysThreshold)
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

// AuditPub exposes the local control-plane bus publisher so the GDPR handlers
// can emit durable velion.audit.v1.* events to audit-core. Returns nil when the
// local NATS connection is disabled.
func (s *Service) AuditPub() AuditPublisher {
	return s.auditPublisher
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

func (s *Service) publishPlanChanged(ctx context.Context, orgID, orgName, previousPlan, newPlan, changedBy, reason string) {
	event := map[string]any{
		"organization_id":   orgID,
		"organization_name": orgName,
		"previous_plan":     previousPlan,
		"new_plan":          newPlan,
		"changed_by":        changedBy,
		"change_reason":     reason,
	}

	if s.publisher != nil {
		s.publisher.Publish(ctx, "organization.plan.changed", event)
	}
	if s.sharedPublisher != nil {
		s.sharedPublisher.PublishPlanChanged(ctx, orgID, orgName, previousPlan, newPlan, changedBy, reason)
		// Notify the user who initiated the plan change (changedBy = user ID).
		if changedBy != "" {
			s.sharedPublisher.PublishPlain("notifications.billing.plan_changed", map[string]any{
				"subscriberId": changedBy,
				"orgId":        orgID,
				"orgName":      orgName,
				"previousPlan": previousPlan,
				"newPlan":      newPlan,
			})
		}
	}
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
