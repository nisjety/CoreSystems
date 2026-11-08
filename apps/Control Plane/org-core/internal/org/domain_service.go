package org

import (
	"context"
	"crypto/rand"
	"encoding/base64"
	"errors"
	"fmt"
	"net/netip"
	"strings"
	"time"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgconn"
	"golang.org/x/net/idna"
	"golang.org/x/net/publicsuffix"
)

const (
	// MaxIdentityDomainsPerOrg counts pending and verified domains; revoked
	// rows are history and do not count.
	MaxIdentityDomainsPerOrg = 10

	// DomainChallengeTTL is how long a published challenge stays valid.
	DomainChallengeTTL = 7 * 24 * time.Hour

	domainTokenBytes    = 24
	domainMaxBytes      = 253
	domainMaxLabelBytes = 63

	domainMethodDNSTXT = "dns_txt"

	// The verified-owner index that arbitrates a contested domain (migration 011).
	domainVerifiedOwnerIndex = "organization_domains_one_verified_owner"
)

var (
	// ErrDomainVerificationUnavailable is returned until EnableDomainVerification
	// has been called (ORG_DOMAIN_VERIFICATION_ENABLED).
	ErrDomainVerificationUnavailable = errors.New("domain verification is not enabled")
	// ErrDomainNotFound is returned when the organisation has no such domain
	// claim. Another organisation's claim is indistinguishable.
	ErrDomainNotFound = errors.New("organization domain not found")
	// ErrDomainVerifiedElsewhere is returned when another organisation already
	// holds the verified claim. It never says which one.
	ErrDomainVerifiedElsewhere = errors.New("domain is verified by another organization")
	// ErrDomainLimitReached is returned at MaxIdentityDomainsPerOrg.
	ErrDomainLimitReached = errors.New("organization domain limit reached")
	// ErrDomainInUse is returned when revoking would strand an enforced SSO policy.
	ErrDomainInUse = errors.New("domain is in use by the organization sso policy")
)

// DomainValidationError carries the closed reason a domain was refused.
type DomainValidationError struct{ Reason string }

func (e *DomainValidationError) Error() string { return "invalid domain: " + e.Reason }

// DomainChallenge is the DNS record an admin publishes to prove a domain.
type DomainChallenge struct {
	Type      string    `json:"type"`
	Name      string    `json:"name"`
	Value     string    `json:"value"`
	ExpiresAt time.Time `json:"expires_at"`
}

// IdentityDomain is one domain claim of an organisation as the admin API shows it.
type IdentityDomain struct {
	Domain             string           `json:"domain"`
	Status             string           `json:"status"`
	VerificationMethod string           `json:"verification_method,omitempty"`
	VerifiedAt         *time.Time       `json:"verified_at,omitempty"`
	AutoInviteEnabled  bool             `json:"auto_invite_enabled"`
	LastCheckAt        *time.Time       `json:"last_check_at,omitempty"`
	LastCheckReason    string           `json:"last_check_reason,omitempty"`
	Challenge          *DomainChallenge `json:"challenge,omitempty"`
}

// DomainVerifyResult is the outcome of one verification attempt.
type DomainVerifyResult struct {
	Domain    string    `json:"domain"`
	Status    string    `json:"status"`
	CheckedAt time.Time `json:"checked_at"`
	Reason    string    `json:"reason,omitempty"`
}

// VerifiedDomainOwner is the internal by-domain answer for a verified claim.
type VerifiedDomainOwner struct {
	OrgID      string    `json:"org_id"`
	Domain     string    `json:"domain"`
	Status     string    `json:"status"`
	VerifiedAt time.Time `json:"verified_at"`
}

// EnableDomainVerification switches the feature on by giving the service the
// resolver that checks the proof. Until it is called every domain method
// refuses, so a service without it behaves exactly as before.
func (s *Service) EnableDomainVerification(resolver DomainTXTResolver) {
	s.domainResolver = resolver
}

// DomainVerificationEnabled reports whether EnableDomainVerification was called.
func (s *Service) DomainVerificationEnabled() bool { return s.domainResolver != nil }

func (s *Service) requireDomains() error {
	if s.domainResolver == nil {
		return ErrDomainVerificationUnavailable
	}
	return nil
}

// NormalizeIdentityDomain returns the lower-case punycode form of a domain or
// a DomainValidationError. It refuses IP literals, a single label, a public
// suffix itself and the free-mail providers that can never prove a company.
func NormalizeIdentityDomain(raw string) (string, error) {
	domain := strings.ToLower(strings.TrimSpace(raw))
	domain = strings.TrimSuffix(domain, ".")
	if domain == "" {
		return "", &DomainValidationError{Reason: "empty"}
	}
	if len(domain) > domainMaxBytes*4 {
		return "", &DomainValidationError{Reason: "too_long"}
	}
	if strings.ContainsAny(domain, "/:@ \t\r\n\\?#%*_") {
		if _, err := netip.ParseAddr(strings.Trim(domain, "[]")); err == nil {
			return "", &DomainValidationError{Reason: "ip_literal"}
		}
		return "", &DomainValidationError{Reason: "malformed"}
	}
	if _, err := netip.ParseAddr(domain); err == nil {
		return "", &DomainValidationError{Reason: "ip_literal"}
	}
	ascii, err := idna.Lookup.ToASCII(domain)
	if err != nil {
		return "", &DomainValidationError{Reason: "malformed"}
	}
	ascii = strings.ToLower(ascii)
	if len(ascii) > domainMaxBytes {
		return "", &DomainValidationError{Reason: "too_long"}
	}
	labels := strings.Split(ascii, ".")
	if len(labels) < 2 {
		return "", &DomainValidationError{Reason: "malformed"}
	}
	for _, label := range labels {
		if label == "" || len(label) > domainMaxLabelBytes ||
			strings.HasPrefix(label, "-") || strings.HasSuffix(label, "-") {
			return "", &DomainValidationError{Reason: "malformed"}
		}
		for i := 0; i < len(label); i++ {
			c := label[i]
			if !(c >= 'a' && c <= 'z' || c >= '0' && c <= '9' || c == '-') {
				return "", &DomainValidationError{Reason: "malformed"}
			}
		}
	}
	// An all-numeric last label is an address or a typo, never a registrable name.
	last := labels[len(labels)-1]
	numeric := true
	for i := 0; i < len(last); i++ {
		if last[i] < '0' || last[i] > '9' {
			numeric = false
			break
		}
	}
	if numeric {
		return "", &DomainValidationError{Reason: "ip_literal"}
	}
	if suffix, _ := publicsuffix.PublicSuffix(ascii); suffix == ascii {
		return "", &DomainValidationError{Reason: "public_suffix"}
	}
	if _, blocked := blockedAutoInviteDomains[ascii]; blocked {
		return "", &DomainValidationError{Reason: "free_mail"}
	}
	return ascii, nil
}

func newDomainToken() (string, error) {
	buf := make([]byte, domainTokenBytes)
	if _, err := rand.Read(buf); err != nil {
		return "", fmt.Errorf("generate domain challenge: %w", err)
	}
	return base64.RawURLEncoding.EncodeToString(buf), nil
}

func domainChallenge(domain, token string, expires *time.Time) *DomainChallenge {
	if token == "" || expires == nil {
		return nil
	}
	return &DomainChallenge{
		Type: domainMethodDNSTXT, Name: DomainChallengeName(domain),
		Value: DomainChallengeValue(token), ExpiresAt: expires.UTC(),
	}
}

// ListIdentityDomains returns the organisation's pending and verified domains
// (revoked claims are history and are left out), each pending one with the
// challenge to publish.
func (s *Service) ListIdentityDomains(ctx context.Context, orgID string) ([]IdentityDomain, error) {
	if err := s.requireDomains(); err != nil {
		return nil, err
	}
	return s.repo.listIdentityDomains(ctx, orgID)
}

// AddIdentityDomain records a pending claim and its challenge. Adding a domain
// the organisation already holds is a no-op that returns the current claim.
func (s *Service) AddIdentityDomain(ctx context.Context, orgID, actorID, actorRole, rawDomain string) (*IdentityDomain, error) {
	if err := s.requireDomains(); err != nil {
		return nil, err
	}
	domain, err := NormalizeIdentityDomain(rawDomain)
	if err != nil {
		return nil, err
	}
	token, err := newDomainToken()
	if err != nil {
		return nil, err
	}
	return s.repo.addIdentityDomain(ctx, orgID, actorID, actorRole, domain, token)
}

// VerifyIdentityDomain checks the DNS proof once. The lookup runs outside any
// transaction so a slow resolver never holds a row lock; the result is applied
// only if the claim is still the one that was checked.
func (s *Service) VerifyIdentityDomain(ctx context.Context, orgID, actorID, actorRole, rawDomain string) (*DomainVerifyResult, error) {
	if err := s.requireDomains(); err != nil {
		return nil, err
	}
	domain, err := NormalizeIdentityDomain(rawDomain)
	if err != nil {
		return nil, err
	}
	claim, err := s.repo.readDomainClaim(ctx, orgID, domain)
	if err != nil {
		return nil, err
	}
	now := time.Now().UTC()
	if claim.Status == "verified" {
		return &DomainVerifyResult{Domain: domain, Status: "verified", CheckedAt: now}, nil
	}
	if claim.Token == "" || claim.ExpiresAt == nil || !claim.ExpiresAt.After(now) {
		return s.repo.recordDomainCheck(ctx, orgID, domain, claim.Token, now, DomainReasonChallengeExpire)
	}
	ok, reason := checkDomainChallenge(ctx, s.domainResolver, domain, claim.Token)
	if !ok {
		return s.repo.recordDomainCheck(ctx, orgID, domain, claim.Token, now, reason)
	}
	return s.repo.markDomainVerified(ctx, orgID, actorID, actorRole, domain, claim.Token, now)
}

// RevokeIdentityDomain withdraws a claim. A verified domain that is the last
// one backing an enforced SSO policy cannot be revoked.
func (s *Service) RevokeIdentityDomain(ctx context.Context, orgID, actorID, actorRole, rawDomain string) error {
	if err := s.requireDomains(); err != nil {
		return err
	}
	domain, err := NormalizeIdentityDomain(rawDomain)
	if err != nil {
		return err
	}
	return s.repo.revokeIdentityDomain(ctx, orgID, actorID, actorRole, domain)
}

// LookupVerifiedDomainOwner answers which organisation holds the verified claim
// on a domain, for the internal by-domain read. Unverified, revoked and unknown
// domains are all ErrDomainNotFound.
func (s *Service) LookupVerifiedDomainOwner(ctx context.Context, rawDomain string) (*VerifiedDomainOwner, error) {
	if err := s.requireDomains(); err != nil {
		return nil, err
	}
	domain, err := NormalizeIdentityDomain(rawDomain)
	if err != nil {
		return nil, err
	}
	return s.repo.lookupVerifiedDomainOwner(ctx, domain)
}

// --- repository ---

type domainClaim struct {
	Status    string
	Token     string
	ExpiresAt *time.Time
}

const identityDomainColumns = `normalized_domain, status, COALESCE(verification_method, ''), verified_at,
  auto_invite_enabled, last_check_at, COALESCE(last_check_reason, ''),
  COALESCE(verification_token, ''), verification_expires_at`

func scanIdentityDomain(row pgx.Row) (IdentityDomain, error) {
	var d IdentityDomain
	var token string
	var expires *time.Time
	if err := row.Scan(&d.Domain, &d.Status, &d.VerificationMethod, &d.VerifiedAt,
		&d.AutoInviteEnabled, &d.LastCheckAt, &d.LastCheckReason, &token, &expires); err != nil {
		return IdentityDomain{}, err
	}
	if d.Status == "pending" {
		d.Challenge = domainChallenge(d.Domain, token, expires)
	}
	return d, nil
}

func (r *Repository) listIdentityDomains(ctx context.Context, orgID string) ([]IdentityDomain, error) {
	out := []IdentityDomain{}
	err := r.db.WithOrgScope(ctx, orgID, func(tx pgx.Tx) error {
		rows, err := tx.Query(ctx, `
SELECT `+identityDomainColumns+`
FROM organization_domains
WHERE org_id = $1 AND status <> 'revoked'
ORDER BY normalized_domain`, orgID)
		if err != nil {
			return fmt.Errorf("list organization domains: %w", err)
		}
		defer rows.Close()
		for rows.Next() {
			d, err := scanIdentityDomain(rows)
			if err != nil {
				return fmt.Errorf("scan organization domain: %w", err)
			}
			out = append(out, d)
		}
		return rows.Err()
	})
	if err != nil {
		return nil, err
	}
	return out, nil
}

// verifiedElsewhere reads on the owner pool, not a scoped transaction: row
// level security would hide the other organisation's claim, and the answer is
// exactly "does one exist". It never returns which organisation.
func (r *Repository) domainVerifiedElsewhere(ctx context.Context, orgID, domain string) (bool, error) {
	var exists bool
	if err := r.pool.QueryRow(ctx, `
SELECT EXISTS (
  SELECT 1 FROM organization_domains
  WHERE normalized_domain = $1 AND status = 'verified' AND org_id <> $2
)`, domain, orgID).Scan(&exists); err != nil {
		return false, fmt.Errorf("check domain owner: %w", err)
	}
	return exists, nil
}

func (r *Repository) addIdentityDomain(ctx context.Context, orgID, actorID, actorRole, domain, newToken string) (*IdentityDomain, error) {
	elsewhere, err := r.domainVerifiedElsewhere(ctx, orgID, domain)
	if err != nil {
		return nil, err
	}
	if elsewhere {
		return nil, ErrDomainVerifiedElsewhere
	}
	var result IdentityDomain
	err = r.db.WithOrgScope(ctx, orgID, func(tx pgx.Tx) error {
		if err := lockOrganizationLifecycle(ctx, tx, orgID); err != nil {
			return err
		}
		var exists bool
		if err := tx.QueryRow(ctx, `SELECT EXISTS (SELECT 1 FROM organizations WHERE id = $1 AND deleted_at IS NULL)`, orgID).Scan(&exists); err != nil {
			return fmt.Errorf("read organization: %w", err)
		}
		if !exists {
			return ErrNotFound
		}
		now := time.Now().UTC()
		current, err := scanIdentityDomain(tx.QueryRow(ctx, `
SELECT `+identityDomainColumns+` FROM organization_domains
WHERE org_id = $1 AND normalized_domain = $2 FOR UPDATE`, orgID, domain))
		found := err == nil
		if err != nil && !errors.Is(err, pgx.ErrNoRows) {
			return fmt.Errorf("read organization domain: %w", err)
		}
		if found && current.Status == "verified" {
			result = current
			return nil
		}
		if found && current.Status == "pending" && current.Challenge != nil && current.Challenge.ExpiresAt.After(now) {
			result = current
			return nil
		}
		if !found || current.Status == "revoked" {
			var count int
			if err := tx.QueryRow(ctx, `SELECT COUNT(*) FROM organization_domains WHERE org_id = $1 AND status <> 'revoked'`, orgID).Scan(&count); err != nil {
				return fmt.Errorf("count organization domains: %w", err)
			}
			if count >= MaxIdentityDomainsPerOrg {
				return ErrDomainLimitReached
			}
		}
		expires := now.Add(DomainChallengeTTL)
		created, err := scanIdentityDomain(tx.QueryRow(ctx, `
INSERT INTO organization_domains (org_id, normalized_domain, status, verification_method,
  verification_token, verification_expires_at, auto_invite_enabled)
VALUES ($1, $2, 'pending', 'dns_txt', $3, $4, false)
ON CONFLICT (org_id, normalized_domain) DO UPDATE SET
  status = 'pending', verification_method = 'dns_txt',
  verification_token = EXCLUDED.verification_token,
  verification_expires_at = EXCLUDED.verification_expires_at,
  verified_at = NULL, verified_by = NULL, revoked_at = NULL, revoked_by = NULL,
  last_check_at = NULL, last_check_reason = NULL, updated_at = NOW()
RETURNING `+identityDomainColumns, orgID, domain, newToken, expires))
		if err != nil {
			return fmt.Errorf("record organization domain: %w", err)
		}
		result = created
		return enqueueIdentityAudit(ctx, tx, identityAuditChange{
			Event: IdentityEventDomainAdded, OrgID: orgID, ActorID: actorID, ActorRole: actorRole,
			Resource: domain, Details: map[string]any{"domain": domain, "method": domainMethodDNSTXT},
		})
	})
	if err != nil {
		return nil, err
	}
	return &result, nil
}

func (r *Repository) readDomainClaim(ctx context.Context, orgID, domain string) (domainClaim, error) {
	var claim domainClaim
	err := r.db.WithOrgScope(ctx, orgID, func(tx pgx.Tx) error {
		return tx.QueryRow(ctx, `
SELECT status, COALESCE(verification_token, ''), verification_expires_at
FROM organization_domains
WHERE org_id = $1 AND normalized_domain = $2 AND status <> 'revoked'`, orgID, domain).
			Scan(&claim.Status, &claim.Token, &claim.ExpiresAt)
	})
	if errors.Is(err, pgx.ErrNoRows) {
		return domainClaim{}, ErrDomainNotFound
	}
	if err != nil {
		return domainClaim{}, fmt.Errorf("read organization domain: %w", err)
	}
	return claim, nil
}

// recordDomainCheck stores a failed attempt. The token guard keeps a stale
// result from landing on a challenge that was refreshed in the meantime.
func (r *Repository) recordDomainCheck(ctx context.Context, orgID, domain, token string, at time.Time, reason string) (*DomainVerifyResult, error) {
	err := r.db.WithOrgScope(ctx, orgID, func(tx pgx.Tx) error {
		_, err := tx.Exec(ctx, `
UPDATE organization_domains
SET last_check_at = $3, last_check_reason = $4, updated_at = NOW()
WHERE org_id = $1 AND normalized_domain = $2 AND status = 'pending'
  AND COALESCE(verification_token, '') = $5`, orgID, domain, at, reason, token)
		return err
	})
	if err != nil {
		return nil, fmt.Errorf("record domain check: %w", err)
	}
	return &DomainVerifyResult{Domain: domain, Status: "pending", CheckedAt: at, Reason: reason}, nil
}

func (r *Repository) markDomainVerified(ctx context.Context, orgID, actorID, actorRole, domain, token string, at time.Time) (*DomainVerifyResult, error) {
	var result *DomainVerifyResult
	err := r.db.WithOrgScope(ctx, orgID, func(tx pgx.Tx) error {
		if err := lockOrganizationLifecycle(ctx, tx, orgID); err != nil {
			return err
		}
		tag, err := tx.Exec(ctx, `
UPDATE organization_domains
SET status = 'verified', verification_method = 'dns_txt', verified_at = $3, verified_by = $4,
    last_check_at = $3, last_check_reason = NULL, verification_token = NULL,
    verification_expires_at = NULL, updated_at = NOW()
WHERE org_id = $1 AND normalized_domain = $2 AND status = 'pending'
  AND COALESCE(verification_token, '') = $5`, orgID, domain, at, actorID, token)
		if err != nil {
			return err
		}
		if tag.RowsAffected() != 1 {
			// The claim changed while the lookup ran; report the new state
			// rather than a verification that did not happen.
			result = &DomainVerifyResult{Domain: domain, Status: "pending", CheckedAt: at, Reason: DomainReasonTXTMismatch}
			return nil
		}
		result = &DomainVerifyResult{Domain: domain, Status: "verified", CheckedAt: at}
		return enqueueIdentityAudit(ctx, tx, identityAuditChange{
			Event: IdentityEventDomainVerified, OrgID: orgID, ActorID: actorID, ActorRole: actorRole,
			Resource: domain, Details: map[string]any{"domain": domain, "method": domainMethodDNSTXT},
		})
	})
	if err != nil {
		var pgErr *pgconn.PgError
		if errors.As(err, &pgErr) && pgErr.Code == "23505" && pgErr.ConstraintName == domainVerifiedOwnerIndex {
			return nil, ErrDomainVerifiedElsewhere
		}
		return nil, fmt.Errorf("verify organization domain: %w", err)
	}
	return result, nil
}

func (r *Repository) revokeIdentityDomain(ctx context.Context, orgID, actorID, actorRole, domain string) error {
	return r.db.WithOrgScope(ctx, orgID, func(tx pgx.Tx) error {
		if err := lockOrganizationLifecycle(ctx, tx, orgID); err != nil {
			return err
		}
		var status string
		if err := tx.QueryRow(ctx, `
SELECT status FROM organization_domains
WHERE org_id = $1 AND normalized_domain = $2 FOR UPDATE`, orgID, domain).Scan(&status); err != nil {
			if errors.Is(err, pgx.ErrNoRows) {
				return ErrDomainNotFound
			}
			return fmt.Errorf("read organization domain: %w", err)
		}
		if status == "revoked" {
			return ErrDomainNotFound
		}
		if status == "verified" {
			var mode string
			err := tx.QueryRow(ctx, `SELECT mode FROM organization_sso_policy WHERE org_id = $1`, orgID).Scan(&mode)
			if err != nil && !errors.Is(err, pgx.ErrNoRows) {
				return fmt.Errorf("read sso policy: %w", err)
			}
			if err == nil && mode == "enforce" {
				var others int
				if err := tx.QueryRow(ctx, `
SELECT COUNT(*) FROM organization_domains
WHERE org_id = $1 AND status = 'verified' AND normalized_domain <> $2`, orgID, domain).Scan(&others); err != nil {
					return fmt.Errorf("count verified domains: %w", err)
				}
				if others == 0 {
					return ErrDomainInUse
				}
			}
		}
		if _, err := tx.Exec(ctx, `
UPDATE organization_domains
SET status = 'revoked', revoked_at = NOW(), revoked_by = $3, verification_token = NULL,
    verification_expires_at = NULL, auto_invite_enabled = false, updated_at = NOW()
WHERE org_id = $1 AND normalized_domain = $2`, orgID, domain, actorID); err != nil {
			return fmt.Errorf("revoke organization domain: %w", err)
		}
		return enqueueIdentityAudit(ctx, tx, identityAuditChange{
			Event: IdentityEventDomainRevoked, OrgID: orgID, ActorID: actorID, ActorRole: actorRole,
			Resource: domain, Details: map[string]any{"domain": domain, "was_verified": status == "verified"},
		})
	})
}

// lookupVerifiedDomainOwner runs on the owner pool: the caller is a service
// that has no acting organisation, like the other /internal cross-org reads.
func (r *Repository) lookupVerifiedDomainOwner(ctx context.Context, domain string) (*VerifiedDomainOwner, error) {
	var owner VerifiedDomainOwner
	err := r.pool.QueryRow(ctx, `
SELECT d.org_id, d.normalized_domain, d.status, d.verified_at
FROM organization_domains d
JOIN organizations o ON o.id = d.org_id
WHERE d.normalized_domain = $1 AND d.status = 'verified' AND o.deleted_at IS NULL`, domain).
		Scan(&owner.OrgID, &owner.Domain, &owner.Status, &owner.VerifiedAt)
	if errors.Is(err, pgx.ErrNoRows) {
		return nil, ErrDomainNotFound
	}
	if err != nil {
		return nil, fmt.Errorf("look up verified domain owner: %w", err)
	}
	return &owner, nil
}
