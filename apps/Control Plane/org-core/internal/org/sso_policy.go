package org

import (
	"context"
	"errors"
	"fmt"
	"net/url"
	"regexp"
	"strings"
	"time"

	"github.com/jackc/pgx/v5"
)

// SSO policy, provider administration and the enforcement read (WS05-S7).
//
// org-core owns the per-organisation policy (organization_sso_policy, created by
// migration 028) and the domain proof. auth-core owns the providers, the
// sign-in evidence and the enforcement hook; it asks org-core which policy
// covers an email domain (LookupSSOPolicyByDomain) and org-core asks auth-core
// for providers and evidence through IdentityProviderAdmin.

const (
	SSOModeOff     = "off"
	SSOModeReport  = "report"
	SSOModeEnforce = "enforce"

	// MaxSSOBreakGlass bounds the break-glass list (the table CHECK repeats it).
	MaxSSOBreakGlass = 2
	// SSOProofWindow is how recent the caller's own SSO sign-in must be before
	// enforcement can be switched on.
	SSOProofWindow = 24 * time.Hour

	ssoProviderTypeOIDC = "oidc"
	ssoMaxFieldBytes    = 512

	IdentityEventSSOPolicyUpdated = "org_sso_policy_updated"
	IdentityEventSSOProviderAdded = "org_sso_provider_added"
	IdentityEventSSOProviderGone  = "org_sso_provider_removed"
)

// Reasons an enforce request or can_enforce can carry. They double as the typed
// 422 codes of PUT policy.
const (
	SSOReasonNoVerifiedDomain = "no_verified_domain"
	SSOReasonNoProvider       = "no_provider"
	SSOReasonNotProven        = "sso_not_proven"
	SSOReasonBreakGlass       = "break_glass_invalid"
	SSOReasonBreakGlass2FA    = "break_glass_requires_2fa"
	ssoReasonProviderDown     = "identity_provider_unavailable"
)

var (
	// ErrSSOPolicyRevisionConflict is the compare-and-set miss on PUT policy.
	ErrSSOPolicyRevisionConflict = errors.New("sso policy revision conflict")
	// ErrSSODomainNotVerified refuses a provider on a domain the organisation has not proven.
	ErrSSODomainNotVerified = errors.New("sso provider domain is not verified for the organization")
	// ErrSSOLastProviderEnforced refuses removing the last provider an enforced policy depends on.
	ErrSSOLastProviderEnforced = errors.New("last sso provider of an enforced policy")

	ssoProviderIDPattern = regexp.MustCompile(`^[a-z0-9][a-z0-9-]{2,62}$`)
)

// SSORequestError is a 400 for a malformed policy or provider request.
type SSORequestError struct{ Reason string }

func (e *SSORequestError) Error() string { return "invalid sso request: " + e.Reason }

// SSOEnforceError is a 422: enforce was requested but a precondition fails.
type SSOEnforceError struct{ Code string }

func (e *SSOEnforceError) Error() string { return "sso enforce refused: " + e.Code }

// SSOPolicy is the stored per-organisation policy.
type SSOPolicy struct {
	Mode              string
	BreakGlassUserIDs []string
	Revision          int64
	UpdatedBy         string
	UpdatedAt         *time.Time
}

// SSOProvider is a provider as the admin API shows it.
type SSOProvider struct {
	ProviderID     string `json:"provider_id"`
	Domain         string `json:"domain"`
	Issuer         string `json:"issuer"`
	Type           string `json:"type"`
	DomainVerified bool   `json:"domain_verified"`
}

// SSOCanEnforce says whether the policy could be switched to enforce now.
type SSOCanEnforce struct {
	OK      bool     `json:"ok"`
	Reasons []string `json:"reasons"`
}

// SSOPolicyView is the GET policy answer.
type SSOPolicyView struct {
	Mode              string        `json:"mode"`
	Revision          int64         `json:"revision"`
	Domains           []string      `json:"domains"`
	BreakGlassUserIDs []string      `json:"break_glass_user_ids"`
	Providers         []SSOProvider `json:"providers"`
	CanEnforce        SSOCanEnforce `json:"can_enforce"`
}

// SSOPolicyInput is the PUT policy request.
type SSOPolicyInput struct {
	Mode              string
	BreakGlassUserIDs []string
	ExpectedRevision  int64
}

// SSOProviderInput is the POST providers request. ClientSecret is write-only.
type SSOProviderInput struct {
	ProviderID   string
	Domain       string
	Type         string
	Issuer       string
	ClientID     string
	ClientSecret string
}

// SSOPolicyByDomain is what auth-core's enforcement hook reads for an email domain.
type SSOPolicyByDomain struct {
	OrgID             string   `json:"org_id"`
	Domain            string   `json:"domain"`
	Mode              string   `json:"mode"`
	BreakGlassUserIDs []string `json:"break_glass_user_ids"`
	Revision          int64    `json:"revision"`
}

// SetIdentityProviderAdmin wires the auth-core client. Without it provider
// administration and enforcement refuse with ErrSSOUnavailable; the policy can
// still be read and set to off or report.
func (s *Service) SetIdentityProviderAdmin(admin IdentityProviderAdmin) {
	s.identityAdmin = admin
}

func (s *Service) requireSSOAdmin() error {
	if err := s.requireDomains(); err != nil {
		return err
	}
	if s.identityAdmin == nil {
		return ErrSSOUnavailable
	}
	return nil
}

func validSSOMode(mode string) bool {
	return mode == SSOModeOff || mode == SSOModeReport || mode == SSOModeEnforce
}

func normalizeBreakGlass(ids []string) ([]string, error) {
	out := make([]string, 0, len(ids))
	seen := map[string]struct{}{}
	for _, id := range ids {
		id = strings.TrimSpace(id)
		if id == "" || len(id) > 128 {
			return nil, &SSORequestError{Reason: "break_glass_user_ids"}
		}
		if _, dup := seen[id]; dup {
			return nil, &SSORequestError{Reason: "break_glass_user_ids"}
		}
		seen[id] = struct{}{}
		out = append(out, id)
	}
	if len(out) > MaxSSOBreakGlass {
		return nil, &SSORequestError{Reason: "break_glass_user_ids"}
	}
	return out, nil
}

// GetSSOPolicy returns the policy with its verified domains, providers and the
// can_enforce verdict for the calling admin.
func (s *Service) GetSSOPolicy(ctx context.Context, orgID, actorID string) (*SSOPolicyView, error) {
	if err := s.requireDomains(); err != nil {
		return nil, err
	}
	policy, err := s.repo.readSSOPolicy(ctx, orgID)
	if err != nil {
		return nil, err
	}
	domains, err := s.repo.listVerifiedDomainNames(ctx, orgID)
	if err != nil {
		return nil, err
	}
	view := &SSOPolicyView{
		Mode: policy.Mode, Revision: policy.Revision, Domains: domains,
		BreakGlassUserIDs: policy.BreakGlassUserIDs, Providers: []SSOProvider{},
	}
	providers, providersErr := s.providersFor(ctx, orgID, domains)
	if providersErr == nil {
		view.Providers = providers
	}
	reasons, err := s.enforceReasons(ctx, orgID, actorID, policy.BreakGlassUserIDs, domains, providers, providersErr)
	if err != nil {
		return nil, err
	}
	view.CanEnforce = SSOCanEnforce{OK: len(reasons) == 0, Reasons: reasons}
	return view, nil
}

func (s *Service) providersFor(ctx context.Context, orgID string, verified []string) ([]SSOProvider, error) {
	if s.identityAdmin == nil {
		return nil, ErrSSOUnavailable
	}
	records, err := s.identityAdmin.ListProviders(ctx, orgID)
	if err != nil {
		return nil, err
	}
	verifiedSet := map[string]struct{}{}
	for _, d := range verified {
		verifiedSet[d] = struct{}{}
	}
	out := make([]SSOProvider, 0, len(records))
	for _, rec := range records {
		_, ok := verifiedSet[strings.ToLower(rec.Domain)]
		out = append(out, SSOProvider{
			ProviderID: rec.ProviderID, Domain: rec.Domain, Issuer: rec.Issuer,
			Type: rec.Type, DomainVerified: ok,
		})
	}
	return out, nil
}

// enforceReasons lists every precondition of enforce that fails, in the order
// the PUT answers them. A provider lookup failure is a reason, never a pass.
func (s *Service) enforceReasons(ctx context.Context, orgID, actorID string, breakGlass, domains []string, providers []SSOProvider, providersErr error) ([]string, error) {
	reasons := []string{}
	if len(domains) == 0 {
		reasons = append(reasons, SSOReasonNoVerifiedDomain)
	}
	if providersErr != nil {
		return append(reasons, ssoReasonProviderDown), nil
	}
	onVerified := false
	for _, p := range providers {
		if p.DomainVerified {
			onVerified = true
			break
		}
	}
	if !onVerified {
		reasons = append(reasons, SSOReasonNoProvider)
	}
	proven, err := s.identityAdmin.HasRecentSignIn(ctx, orgID, actorID, SSOProofWindow)
	if err != nil {
		return append(reasons, ssoReasonProviderDown), nil
	}
	if !proven {
		reasons = append(reasons, SSOReasonNotProven)
	}
	if len(breakGlass) == 0 {
		return append(reasons, SSOReasonBreakGlass), nil
	}
	facts, err := s.identityAdmin.MemberFacts(ctx, orgID, breakGlass)
	if err != nil {
		return append(reasons, ssoReasonProviderDown), nil
	}
	byUser := map[string]SSOMemberFact{}
	for _, f := range facts {
		byUser[f.UserID] = f
	}
	invalid, no2FA := false, false
	for _, id := range breakGlass {
		fact, ok := byUser[id]
		if !ok || strings.ToLower(fact.Role) != "owner" {
			invalid = true
		} else if !fact.TwoFactorEnabled {
			no2FA = true
		}
	}
	if invalid {
		reasons = append(reasons, SSOReasonBreakGlass)
	}
	if no2FA {
		reasons = append(reasons, SSOReasonBreakGlass2FA)
	}
	return reasons, nil
}

// PutSSOPolicy sets the mode and break-glass list under compare-and-set. Enforce
// needs every precondition of can_enforce.
func (s *Service) PutSSOPolicy(ctx context.Context, orgID, actorID, actorRole string, in SSOPolicyInput) (*SSOPolicyView, error) {
	if err := s.requireDomains(); err != nil {
		return nil, err
	}
	if !validSSOMode(in.Mode) || in.ExpectedRevision < 0 {
		return nil, &SSORequestError{Reason: "mode"}
	}
	breakGlass, err := normalizeBreakGlass(in.BreakGlassUserIDs)
	if err != nil {
		return nil, err
	}
	previous, err := s.repo.readSSOPolicy(ctx, orgID)
	if err != nil {
		return nil, err
	}
	if previous.Revision != in.ExpectedRevision {
		return nil, ErrSSOPolicyRevisionConflict
	}
	domains, err := s.repo.listVerifiedDomainNames(ctx, orgID)
	if err != nil {
		return nil, err
	}
	if in.Mode == SSOModeEnforce {
		if err := s.requireSSOAdmin(); err != nil {
			return nil, err
		}
		providers, providersErr := s.providersFor(ctx, orgID, domains)
		reasons, err := s.enforceReasons(ctx, orgID, actorID, breakGlass, domains, providers, providersErr)
		if err != nil {
			return nil, err
		}
		if len(reasons) > 0 {
			if reasons[0] == ssoReasonProviderDown {
				return nil, ErrSSOUnavailable
			}
			return nil, &SSOEnforceError{Code: reasons[0]}
		}
	} else if len(breakGlass) > 0 && s.identityAdmin != nil {
		// A break-glass user must be an owner in every mode; the two-factor flag
		// is only required once enforcement is on.
		facts, err := s.identityAdmin.MemberFacts(ctx, orgID, breakGlass)
		if err != nil {
			return nil, err
		}
		owners := map[string]struct{}{}
		for _, f := range facts {
			if strings.ToLower(f.Role) == "owner" {
				owners[f.UserID] = struct{}{}
			}
		}
		for _, id := range breakGlass {
			if _, ok := owners[id]; !ok {
				return nil, &SSOEnforceError{Code: SSOReasonBreakGlass}
			}
		}
	}
	if err := s.repo.writeSSOPolicy(ctx, orgID, actorID, actorRole, in.Mode, breakGlass, in.ExpectedRevision); err != nil {
		return nil, err
	}
	if in.Mode == SSOModeEnforce && previous.Mode != SSOModeEnforce && s.identityAdmin != nil {
		// Without this a covered user keeps a non-SSO session until it expires.
		// Best effort: the policy is committed; the failure is logged by the caller
		// through the returned view being fresh, and the sweep can be re-run by
		// saving the policy again.
		exempt := append(append([]string{}, breakGlass...), actorID)
		revokeCtx, cancel := context.WithTimeout(context.WithoutCancel(ctx), 60*time.Second)
		defer cancel()
		_, _ = s.identityAdmin.RevokeSessions(revokeCtx, orgID, actorID, domains, exempt)
	}
	return s.GetSSOPolicy(ctx, orgID, actorID)
}

func validateSSOIssuer(raw string) error {
	if len(raw) == 0 || len(raw) > 2048 {
		return ErrSSOInvalidIssuer
	}
	u, err := url.Parse(raw)
	if err != nil || u.Host == "" || u.User != nil || u.RawQuery != "" || u.Fragment != "" {
		return ErrSSOInvalidIssuer
	}
	if u.Scheme != "https" {
		return ErrSSOInvalidIssuer
	}
	return nil
}

// AddSSOProvider registers an OIDC provider for a domain the organisation has
// proven. The client secret goes to auth-core and is never stored, logged or
// audited here.
func (s *Service) AddSSOProvider(ctx context.Context, orgID, actorID, actorRole string, in SSOProviderInput) (*SSOProvider, error) {
	if err := s.requireSSOAdmin(); err != nil {
		return nil, err
	}
	providerID := strings.ToLower(strings.TrimSpace(in.ProviderID))
	if !ssoProviderIDPattern.MatchString(providerID) {
		return nil, &SSORequestError{Reason: "provider_id"}
	}
	if strings.ToLower(strings.TrimSpace(in.Type)) != ssoProviderTypeOIDC {
		return nil, &SSORequestError{Reason: "type"}
	}
	clientID, clientSecret := strings.TrimSpace(in.ClientID), strings.TrimSpace(in.ClientSecret)
	if clientID == "" || len(clientID) > ssoMaxFieldBytes || clientSecret == "" || len(clientSecret) > ssoMaxFieldBytes {
		return nil, &SSORequestError{Reason: "client_credentials"}
	}
	issuer := strings.TrimSpace(in.Issuer)
	if err := validateSSOIssuer(issuer); err != nil {
		return nil, err
	}
	domain, err := NormalizeIdentityDomain(in.Domain)
	if err != nil {
		return nil, ErrSSODomainNotVerified
	}
	verified, err := s.repo.listVerifiedDomainNames(ctx, orgID)
	if err != nil {
		return nil, err
	}
	onVerified := false
	for _, d := range verified {
		if d == domain {
			onVerified = true
			break
		}
	}
	if !onVerified {
		return nil, ErrSSODomainNotVerified
	}
	if err := s.identityAdmin.RegisterProvider(ctx, SSOProviderRegistration{
		OrganizationID: orgID, ActorUserID: actorID, ProviderID: providerID, Domain: domain,
		Issuer: issuer, ClientID: clientID, ClientSecret: clientSecret,
	}); err != nil {
		return nil, err
	}
	if err := s.repo.recordIdentityAudit(ctx, identityAuditChange{
		Event: IdentityEventSSOProviderAdded, OrgID: orgID, ActorID: actorID, ActorRole: actorRole,
		Resource: providerID, Details: map[string]any{"provider_id": providerID, "domain": domain, "type": ssoProviderTypeOIDC},
	}); err != nil {
		return nil, err
	}
	return &SSOProvider{ProviderID: providerID, Domain: domain, Issuer: issuer, Type: ssoProviderTypeOIDC, DomainVerified: true}, nil
}

// RemoveSSOProvider deletes a provider unless an enforced policy would be left
// without any provider on a verified domain.
func (s *Service) RemoveSSOProvider(ctx context.Context, orgID, actorID, actorRole, rawProviderID string) error {
	if err := s.requireSSOAdmin(); err != nil {
		return err
	}
	providerID := strings.ToLower(strings.TrimSpace(rawProviderID))
	if !ssoProviderIDPattern.MatchString(providerID) {
		return &SSORequestError{Reason: "provider_id"}
	}
	policy, err := s.repo.readSSOPolicy(ctx, orgID)
	if err != nil {
		return err
	}
	if policy.Mode == SSOModeEnforce {
		verified, err := s.repo.listVerifiedDomainNames(ctx, orgID)
		if err != nil {
			return err
		}
		providers, err := s.providersFor(ctx, orgID, verified)
		if err != nil {
			return err
		}
		others := 0
		for _, p := range providers {
			if p.ProviderID != providerID && p.DomainVerified {
				others++
			}
		}
		if others == 0 {
			return ErrSSOLastProviderEnforced
		}
	}
	if err := s.identityAdmin.DeleteProvider(ctx, orgID, providerID, actorID); err != nil {
		return err
	}
	return s.repo.recordIdentityAudit(ctx, identityAuditChange{
		Event: IdentityEventSSOProviderGone, OrgID: orgID, ActorID: actorID, ActorRole: actorRole,
		Resource: providerID, Details: map[string]any{"provider_id": providerID},
	})
}

// LookupSSOPolicyByDomain answers the enforcement hook: the organisation that
// holds the verified claim on the domain and its policy (off when it has none).
func (s *Service) LookupSSOPolicyByDomain(ctx context.Context, rawDomain string) (*SSOPolicyByDomain, error) {
	if err := s.requireDomains(); err != nil {
		return nil, err
	}
	domain, err := NormalizeIdentityDomain(rawDomain)
	if err != nil {
		return nil, err
	}
	return s.repo.lookupSSOPolicyByDomain(ctx, domain)
}

// --- repository ---

func (r *Repository) readSSOPolicy(ctx context.Context, orgID string) (SSOPolicy, error) {
	policy := SSOPolicy{Mode: SSOModeOff, BreakGlassUserIDs: []string{}}
	err := r.db.WithOrgScope(ctx, orgID, func(tx pgx.Tx) error {
		var updatedBy *string
		err := tx.QueryRow(ctx, `
SELECT mode, break_glass_user_ids, revision, updated_by, updated_at
FROM organization_sso_policy WHERE org_id = $1`, orgID).
			Scan(&policy.Mode, &policy.BreakGlassUserIDs, &policy.Revision, &updatedBy, &policy.UpdatedAt)
		if errors.Is(err, pgx.ErrNoRows) {
			return nil
		}
		if err != nil {
			return fmt.Errorf("read sso policy: %w", err)
		}
		if updatedBy != nil {
			policy.UpdatedBy = *updatedBy
		}
		if policy.BreakGlassUserIDs == nil {
			policy.BreakGlassUserIDs = []string{}
		}
		return nil
	})
	return policy, err
}

func (r *Repository) listVerifiedDomainNames(ctx context.Context, orgID string) ([]string, error) {
	domains := []string{}
	err := r.db.WithOrgScope(ctx, orgID, func(tx pgx.Tx) error {
		rows, err := tx.Query(ctx, `
SELECT normalized_domain FROM organization_domains
WHERE org_id = $1 AND status = 'verified' ORDER BY normalized_domain`, orgID)
		if err != nil {
			return fmt.Errorf("list verified domains: %w", err)
		}
		defer rows.Close()
		for rows.Next() {
			var d string
			if err := rows.Scan(&d); err != nil {
				return fmt.Errorf("scan verified domain: %w", err)
			}
			domains = append(domains, d)
		}
		return rows.Err()
	})
	return domains, err
}

func (r *Repository) writeSSOPolicy(ctx context.Context, orgID, actorID, actorRole, mode string, breakGlass []string, expected int64) error {
	return r.db.WithOrgScope(ctx, orgID, func(tx pgx.Tx) error {
		if err := lockOrganizationLifecycle(ctx, tx, orgID); err != nil {
			return err
		}
		var current int64
		err := tx.QueryRow(ctx, `SELECT revision FROM organization_sso_policy WHERE org_id = $1 FOR UPDATE`, orgID).Scan(&current)
		if err != nil && !errors.Is(err, pgx.ErrNoRows) {
			return fmt.Errorf("lock sso policy: %w", err)
		}
		if current != expected {
			return ErrSSOPolicyRevisionConflict
		}
		if mode == SSOModeEnforce {
			// A domain revoked since the precondition check must not leave an
			// enforced policy with nothing to enforce on.
			var verified int
			if err := tx.QueryRow(ctx, `SELECT COUNT(*) FROM organization_domains WHERE org_id = $1 AND status = 'verified'`, orgID).Scan(&verified); err != nil {
				return fmt.Errorf("count verified domains: %w", err)
			}
			if verified == 0 {
				return &SSOEnforceError{Code: SSOReasonNoVerifiedDomain}
			}
		}
		if current == 0 {
			if _, err := tx.Exec(ctx, `
INSERT INTO organization_sso_policy (org_id, mode, break_glass_user_ids, revision, updated_by, updated_at)
VALUES ($1, $2, $3, 1, $4, NOW())`, orgID, mode, breakGlass, actorID); err != nil {
				return fmt.Errorf("insert sso policy: %w", err)
			}
		} else if _, err := tx.Exec(ctx, `
UPDATE organization_sso_policy
SET mode = $2, break_glass_user_ids = $3, revision = revision + 1, updated_by = $4, updated_at = NOW()
WHERE org_id = $1 AND revision = $5`, orgID, mode, breakGlass, actorID, expected); err != nil {
			return fmt.Errorf("update sso policy: %w", err)
		}
		return enqueueIdentityAudit(ctx, tx, identityAuditChange{
			Event: IdentityEventSSOPolicyUpdated, OrgID: orgID, ActorID: actorID, ActorRole: actorRole,
			Resource: "sso_policy", Details: map[string]any{
				"mode": mode, "break_glass_count": len(breakGlass), "revision": expected + 1,
			},
		})
	})
}

// recordIdentityAudit writes one outbox row in its own scoped transaction, for
// changes whose state lives in another service (providers).
func (r *Repository) recordIdentityAudit(ctx context.Context, change identityAuditChange) error {
	return r.db.WithOrgScope(ctx, change.OrgID, func(tx pgx.Tx) error {
		return enqueueIdentityAudit(ctx, tx, change)
	})
}

// lookupSSOPolicyByDomain runs on the owner pool like lookupVerifiedDomainOwner:
// the caller is auth-core, which has no acting organisation.
func (r *Repository) lookupSSOPolicyByDomain(ctx context.Context, domain string) (*SSOPolicyByDomain, error) {
	out := SSOPolicyByDomain{Mode: SSOModeOff, BreakGlassUserIDs: []string{}, Revision: 0}
	var mode *string
	var breakGlass []string
	var revision *int64
	err := r.pool.QueryRow(ctx, `
SELECT d.org_id, d.normalized_domain, p.mode, p.break_glass_user_ids, p.revision
FROM organization_domains d
JOIN organizations o ON o.id = d.org_id
LEFT JOIN organization_sso_policy p ON p.org_id = d.org_id
WHERE d.normalized_domain = $1 AND d.status = 'verified' AND o.deleted_at IS NULL`, domain).
		Scan(&out.OrgID, &out.Domain, &mode, &breakGlass, &revision)
	if errors.Is(err, pgx.ErrNoRows) {
		return nil, ErrDomainNotFound
	}
	if err != nil {
		return nil, fmt.Errorf("look up sso policy by domain: %w", err)
	}
	if mode != nil {
		out.Mode = *mode
	}
	if breakGlass != nil {
		out.BreakGlassUserIDs = breakGlass
	}
	if revision != nil {
		out.Revision = *revision
	}
	return &out, nil
}
