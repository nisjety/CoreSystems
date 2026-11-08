package org

import (
	"context"
	"crypto/rand"
	"crypto/sha256"
	"encoding/base64"
	"encoding/hex"
	"errors"
	"fmt"
	"net/url"
	"regexp"
	"strings"
	"time"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgconn"
)

// SCIM Users (WS05-S8).
//
// org-core owns the SCIM bearer tokens and the SCIM user records; auth-core
// owns the member table, so every membership change goes through
// ScimMembershipProvisioner. The organisation of a request comes from the token
// row, never from a header. Only identity keys are stored: the e-mail (userName),
// the external id and the auth-core user id. SCIM can never grant more than
// `member`; a re-activation restores the role held at deactivation, capped at
// `admin` (an owner comes back as admin).

const (
	// SCIMTokenPrefix starts every bearer token so a leaked one is recognisable.
	SCIMTokenPrefix = "scim_"
	// MaxActiveSCIMTokens bounds live tokens per organisation.
	MaxActiveSCIMTokens = 3

	scimTokenRandomBytes = 32 // 43 base64url characters
	scimTokenMinDays     = 1
	scimTokenMaxDays     = 365
	scimTokenNameMax     = 80
	scimLastUsedEvery    = time.Minute

	IdentityEventSCIMTokenCreated      = "org_scim_token_created"
	IdentityEventSCIMTokenRevoked      = "org_scim_token_revoked"
	IdentityEventSCIMUserProvisioned   = "org_scim_user_provisioned"
	IdentityEventSCIMUserDeprovisioned = "org_scim_user_deprovisioned"
	IdentityEventSCIMUserReactivated   = "org_scim_user_reactivated"
	IdentityEventSCIMRefusedLastOwner  = "org_scim_deprovision_refused_last_owner"

	scimActorRole = "scim"
)

var (
	// ErrSCIMUnavailable means SCIM is not enabled or auth-core cannot be reached.
	ErrSCIMUnavailable = errors.New("scim is unavailable")
	// ErrSCIMUnauthorized is any token that does not resolve: unknown, revoked or expired.
	ErrSCIMUnauthorized = errors.New("scim token is not valid")
	// ErrSCIMNoVerifiedDomain refuses a token for an organisation with no proven domain.
	ErrSCIMNoVerifiedDomain = errors.New("organization has no verified domain")
	// ErrSCIMTokenLimit is MaxActiveSCIMTokens reached.
	ErrSCIMTokenLimit = errors.New("scim token limit reached")
	// ErrSCIMTokenNotFound is a token id of another organisation or an unknown one.
	ErrSCIMTokenNotFound = errors.New("scim token not found")
	// ErrSCIMUserNotFound is a SCIM id of another organisation or an unknown one.
	ErrSCIMUserNotFound = errors.New("scim user not found")
	// ErrSCIMUniqueness is a userName already managed in this organisation.
	ErrSCIMUniqueness = errors.New("scim userName already exists")
	// ErrSCIMDomainNotVerified is a userName whose domain this organisation has not proven.
	ErrSCIMDomainNotVerified = errors.New("scim userName domain is not verified for the organization")
	// ErrSCIMLastOwner refuses to deprovision the only owner.
	ErrSCIMLastOwner = errors.New("cannot deprovision the only owner")
	// ErrSCIMMembershipConflict is a concurrent membership change; the IdP retries.
	ErrSCIMMembershipConflict = errors.New("membership changed concurrently")
	// ErrSCIMInvalidFilter is a filter other than userName eq / externalId eq.
	ErrSCIMInvalidFilter = errors.New("scim filter is not supported")

	scimEmailPattern = regexp.MustCompile(`^[^\s@]{1,64}@[^\s@]{1,255}\.[^\s@.]{2,63}$`) // the last label is 2-63 characters with no dot, so a trailing dot is refused here, not dropped by NormalizeIdentityDomain
)

// SCIMRequestError is a 400 invalidValue with a closed reason.
type SCIMRequestError struct{ Reason string }

func (e *SCIMRequestError) Error() string { return "invalid scim request: " + e.Reason }

// ScimMembershipProvisioner is the seam to auth-core's internal SCIM surface.
// *AuthCoreIdentityClient satisfies it; tests use a fake.
type ScimMembershipProvisioner interface {
	ProvisionMember(ctx context.Context, req SCIMProvisionRequest) (*SCIMProvisionResult, error)
	DeprovisionMember(ctx context.Context, req SCIMDeprovisionRequest) (*SCIMDeprovisionResult, error)
}

// SCIMProvisionRequest asks auth-core to find or create the user and add the member.
type SCIMProvisionRequest struct {
	OperationID    string `json:"operationId"`
	OrganizationID string `json:"organizationId"`
	ActorUserID    string `json:"actorUserId"`
	Email          string `json:"email"`
	Role           string `json:"role"`
}

// SCIMProvisionResult is auth-core's answer.
type SCIMProvisionResult struct {
	UserID        string `json:"userId"`
	MemberID      string `json:"memberId"`
	Role          string `json:"role"`
	MemberCreated bool   `json:"memberCreated"`
	UserCreated   bool   `json:"userCreated"`
}

// SCIMDeprovisionRequest asks auth-core to remove the member and end its sessions.
type SCIMDeprovisionRequest struct {
	OperationID    string `json:"operationId"`
	OrganizationID string `json:"organizationId"`
	ActorUserID    string `json:"actorUserId"`
	UserID         string `json:"userId"`
}

// SCIMDeprovisionResult is auth-core's answer. Removed is false when the user
// was already not a member.
type SCIMDeprovisionResult struct {
	Removed  bool   `json:"removed"`
	Replay   bool   `json:"replay"`
	Role     string `json:"role"`
	Revision int64  `json:"revision"`
}

// SCIMTokenAuth is what a valid bearer token resolves to.
type SCIMTokenAuth struct {
	TokenID   string
	OrgID     string
	CreatedBy string
}

// SCIMToken is one token as the admin API shows it. The secret is never here.
type SCIMToken struct {
	ID         string     `json:"id"`
	Name       string     `json:"name"`
	Prefix     string     `json:"prefix"`
	CreatedBy  string     `json:"created_by"`
	CreatedAt  time.Time  `json:"created_at"`
	ExpiresAt  time.Time  `json:"expires_at"`
	LastUsedAt *time.Time `json:"last_used_at,omitempty"`
	RevokedAt  *time.Time `json:"revoked_at,omitempty"`
}

// SCIMTokenIssued is the one response that carries the bearer token.
type SCIMTokenIssued struct {
	Token       SCIMToken `json:"token"`
	BearerToken string    `json:"bearer_token"`
}

// SCIMStatus is the admin panel's summary.
type SCIMStatus struct {
	Enabled         bool       `json:"enabled"`
	VerifiedDomains int        `json:"verified_domains"`
	ActiveTokens    int        `json:"active_tokens"`
	MaxTokens       int        `json:"max_tokens"`
	ManagedUsers    int        `json:"managed_users"`
	ActiveUsers     int        `json:"active_users"`
	LastUsedAt      *time.Time `json:"last_used_at,omitempty"`
}

// SCIMUserRecord is one managed identity.
type SCIMUserRecord struct {
	SCIMID             string
	ExternalID         string
	UserName           string
	UserID             string
	Active             bool
	RoleAtDeactivation string
	DeprovisionedAt    *time.Time
	DeletedAt          *time.Time
	Revision           int64
	CreatedAt          time.Time
	UpdatedAt          time.Time
}

// SCIMUserInput is POST /Users reduced to the identity keys.
type SCIMUserInput struct {
	UserName   string
	ExternalID string
	Active     *bool
}

// SCIMFilter is the one filter SCIM accepts: attribute is "userName" or "externalId".
type SCIMFilter struct{ Attribute, Value string }

// SetScimProvisioner enables the SCIM surface by wiring auth-core. Without it
// every SCIM method answers ErrSCIMUnavailable.
func (s *Service) SetScimProvisioner(p ScimMembershipProvisioner) { s.scimProvisioner = p }

// SCIMEnabled reports whether SetScimProvisioner was called.
func (s *Service) SCIMEnabled() bool { return s.scimProvisioner != nil }

func (s *Service) requireSCIM() error {
	if s.scimProvisioner == nil {
		return ErrSCIMUnavailable
	}
	return nil
}

func hashSCIMToken(token string) string {
	sum := sha256.Sum256([]byte(token))
	return hex.EncodeToString(sum[:])
}

func newSCIMToken() (string, error) {
	raw := make([]byte, scimTokenRandomBytes)
	if _, err := rand.Read(raw); err != nil {
		return "", fmt.Errorf("generate scim token: %w", err)
	}
	return SCIMTokenPrefix + base64.RawURLEncoding.EncodeToString(raw), nil
}

// scimOperationID derives the auth-core operation id from the state transition,
// so a retried PATCH carries the same id and cannot apply twice.
func scimOperationID(orgID, scimID string, targetActive bool, revision int64) string {
	seed := fmt.Sprintf("verevon-scim-op\x00%s\x00%s\x00%t\x00%d", orgID, scimID, targetActive, revision)
	sum := sha256.Sum256([]byte(seed))
	return formatUUID(sum[:16], 5)
}

var scimUUIDPattern = regexp.MustCompile(`^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$`)

// formatUUID renders 16 bytes as a UUID of the given version (RFC 4122 variant).
func formatUUID(b []byte, version byte) string {
	id := make([]byte, 16)
	copy(id, b)
	id[6] = (id[6] & 0x0f) | (version << 4)
	id[8] = (id[8] & 0x3f) | 0x80
	h := hex.EncodeToString(id)
	return h[0:8] + "-" + h[8:12] + "-" + h[12:16] + "-" + h[16:20] + "-" + h[20:]
}

func newUUID() (string, error) {
	raw := make([]byte, 16)
	if _, err := rand.Read(raw); err != nil {
		return "", fmt.Errorf("generate uuid: %w", err)
	}
	return formatUUID(raw, 4), nil
}

func validUUID(s string) bool { return scimUUIDPattern.MatchString(strings.ToLower(s)) }

// NormalizeSCIMUserName lowercases and checks the e-mail used as the identity key.
func NormalizeSCIMUserName(raw string) (email, domain string, err error) {
	email = strings.ToLower(strings.TrimSpace(raw))
	if len(email) > 254 || !scimEmailPattern.MatchString(email) {
		return "", "", &SCIMRequestError{Reason: "userName"}
	}
	at := strings.LastIndex(email, "@")
	domain, derr := NormalizeIdentityDomain(email[at+1:])
	if derr != nil {
		return "", "", &SCIMRequestError{Reason: "userName"}
	}
	return email, domain, nil
}

// --- tokens ---

// CreateSCIMToken mints a bearer token for the organisation's identity provider.
func (s *Service) CreateSCIMToken(ctx context.Context, orgID, actorID, actorRole, name string, expiresInDays int) (*SCIMTokenIssued, error) {
	if err := s.requireSCIM(); err != nil {
		return nil, err
	}
	name = strings.TrimSpace(name)
	if name == "" || len(name) > scimTokenNameMax {
		return nil, &SCIMRequestError{Reason: "name"}
	}
	if expiresInDays < scimTokenMinDays || expiresInDays > scimTokenMaxDays {
		return nil, &SCIMRequestError{Reason: "expires_in_days"}
	}
	bearer, err := newSCIMToken()
	if err != nil {
		return nil, err
	}
	now := time.Now().UTC()
	tokenID, err := newUUID()
	if err != nil {
		return nil, err
	}
	token := SCIMToken{
		ID: tokenID, Name: name, Prefix: bearer[:len(SCIMTokenPrefix)+4], CreatedBy: actorID,
		CreatedAt: now, ExpiresAt: now.Add(time.Duration(expiresInDays) * 24 * time.Hour),
	}
	if err := s.repo.insertSCIMToken(ctx, orgID, actorRole, token, hashSCIMToken(bearer)); err != nil {
		return nil, err
	}
	return &SCIMTokenIssued{Token: token, BearerToken: bearer}, nil
}

// ListSCIMTokens lists the organisation's tokens, newest first, secrets never.
func (s *Service) ListSCIMTokens(ctx context.Context, orgID string) ([]SCIMToken, error) {
	if err := s.requireSCIM(); err != nil {
		return nil, err
	}
	return s.repo.listSCIMTokens(ctx, orgID)
}

// RevokeSCIMToken revokes one token. Revoking twice is a no-op.
func (s *Service) RevokeSCIMToken(ctx context.Context, orgID, actorID, actorRole, tokenID string) error {
	if err := s.requireSCIM(); err != nil {
		return err
	}
	if !validUUID(tokenID) {
		return ErrSCIMTokenNotFound
	}
	return s.repo.revokeSCIMToken(ctx, orgID, actorID, actorRole, tokenID)
}

// GetSCIMStatus summarises SCIM for the admin panel.
func (s *Service) GetSCIMStatus(ctx context.Context, orgID string) (*SCIMStatus, error) {
	if err := s.requireSCIM(); err != nil {
		return nil, err
	}
	return s.repo.scimStatus(ctx, orgID)
}

// ResolveSCIMToken maps a bearer token to its organisation. Unknown, revoked
// and expired tokens are indistinguishable.
func (s *Service) ResolveSCIMToken(ctx context.Context, bearer string) (*SCIMTokenAuth, error) {
	if err := s.requireSCIM(); err != nil {
		return nil, err
	}
	bearer = strings.TrimSpace(bearer)
	if !strings.HasPrefix(bearer, SCIMTokenPrefix) || len(bearer) > 128 {
		return nil, ErrSCIMUnauthorized
	}
	return s.repo.resolveSCIMToken(ctx, hashSCIMToken(bearer))
}

// --- SCIM users ---

func (s *Service) domainVerifiedFor(ctx context.Context, orgID, domain string) error {
	verified, err := s.repo.listVerifiedDomainNames(ctx, orgID)
	if err != nil {
		return err
	}
	for _, d := range verified {
		if d == domain {
			return nil
		}
	}
	return ErrSCIMDomainNotVerified
}

// ListSCIMUsers lists the organisation's managed users, optionally by one filter.
func (s *Service) ListSCIMUsers(ctx context.Context, auth *SCIMTokenAuth, filter *SCIMFilter, startIndex, count int) ([]SCIMUserRecord, int, error) {
	if err := s.requireSCIM(); err != nil {
		return nil, 0, err
	}
	if filter != nil {
		switch filter.Attribute {
		case "userName":
			filter.Value = strings.ToLower(strings.TrimSpace(filter.Value))
		case "externalId":
			filter.Value = strings.TrimSpace(filter.Value)
		default:
			return nil, 0, ErrSCIMInvalidFilter
		}
	}
	if startIndex < 1 {
		startIndex = 1
	}
	if count < 0 {
		count = 0
	}
	if count > 100 {
		count = 100
	}
	return s.repo.listSCIMUsers(ctx, auth.OrgID, filter, startIndex-1, count)
}

// GetSCIMUser reads one managed user of the token's organisation.
func (s *Service) GetSCIMUser(ctx context.Context, auth *SCIMTokenAuth, scimID string) (*SCIMUserRecord, error) {
	if err := s.requireSCIM(); err != nil {
		return nil, err
	}
	if !validUUID(scimID) {
		return nil, ErrSCIMUserNotFound
	}
	scimID = strings.ToLower(scimID)
	rec, err := s.repo.readSCIMUser(ctx, auth.OrgID, scimID)
	if err != nil {
		return nil, err
	}
	return rec, nil
}

// CreateSCIMUser is POST /Users. The role is always `member`.
func (s *Service) CreateSCIMUser(ctx context.Context, auth *SCIMTokenAuth, in SCIMUserInput) (*SCIMUserRecord, error) {
	if err := s.requireSCIM(); err != nil {
		return nil, err
	}
	email, domain, err := NormalizeSCIMUserName(in.UserName)
	if err != nil {
		return nil, err
	}
	externalID := strings.TrimSpace(in.ExternalID)
	if len(externalID) > 255 {
		return nil, &SCIMRequestError{Reason: "externalId"}
	}
	if err := s.domainVerifiedFor(ctx, auth.OrgID, domain); err != nil {
		return nil, err
	}
	active := in.Active == nil || *in.Active
	existing, err := s.repo.findSCIMUserByName(ctx, auth.OrgID, email)
	if err != nil {
		return nil, err
	}
	if existing != nil && existing.DeletedAt == nil {
		return nil, ErrSCIMUniqueness
	}
	scimID, err := newUUID()
	if err != nil {
		return nil, err
	}
	revision := int64(1)
	if existing != nil { // a tombstone is revived under its old id
		scimID, revision = existing.SCIMID, existing.Revision+1
	}
	userID := ""
	if active {
		res, err := s.scimProvisioner.ProvisionMember(ctx, SCIMProvisionRequest{
			OperationID: scimOperationID(auth.OrgID, scimID, true, revision), OrganizationID: auth.OrgID,
			ActorUserID: auth.CreatedBy, Email: email, Role: "member",
		})
		if err != nil {
			return nil, err
		}
		userID = res.UserID
	}
	rec, err := s.repo.upsertSCIMUser(ctx, auth, scimUserWrite{
		SCIMID: scimID, ExternalID: externalID, UserName: email, UserID: userID, Active: active,
		Revision: revision, Revive: existing != nil,
		Event: IdentityEventSCIMUserProvisioned, EventOn: active,
	})
	if err != nil {
		return nil, err
	}
	return rec, nil
}

// SetSCIMUserActive applies active=true or active=false. It compares state
// first, so a retried or repeated request is a no-op.
func (s *Service) SetSCIMUserActive(ctx context.Context, auth *SCIMTokenAuth, scimID string, active bool) (*SCIMUserRecord, error) {
	if err := s.requireSCIM(); err != nil {
		return nil, err
	}
	rec, err := s.GetSCIMUser(ctx, auth, scimID)
	if err != nil {
		return nil, err
	}
	if rec.DeletedAt != nil {
		return nil, ErrSCIMUserNotFound
	}
	if rec.Active == active {
		return rec, nil
	}
	if active {
		return s.reactivateSCIMUser(ctx, auth, rec)
	}
	return s.deactivateSCIMUser(ctx, auth, rec, false)
}

func (s *Service) deactivateSCIMUser(ctx context.Context, auth *SCIMTokenAuth, rec *SCIMUserRecord, tombstone bool) (*SCIMUserRecord, error) {
	role := rec.RoleAtDeactivation
	if rec.UserID != "" {
		res, err := s.scimProvisioner.DeprovisionMember(ctx, SCIMDeprovisionRequest{
			OperationID:    scimOperationID(auth.OrgID, rec.SCIMID, false, rec.Revision),
			OrganizationID: auth.OrgID, ActorUserID: auth.CreatedBy, UserID: rec.UserID,
		})
		if err != nil {
			if errors.Is(err, ErrSCIMLastOwner) {
				_ = s.repo.recordIdentityAudit(ctx, identityAuditChange{
					Event: IdentityEventSCIMRefusedLastOwner, OrgID: auth.OrgID, ActorID: auth.CreatedBy,
					ActorRole: scimActorRole, Resource: rec.SCIMID,
					Details: map[string]any{"scim_id": rec.SCIMID, "user_id": rec.UserID, "token_id": auth.TokenID},
				})
			}
			return nil, err
		}
		if res.Role != "" {
			role = res.Role
		}
	}
	if role == "" {
		role = "member"
	}
	return s.repo.upsertSCIMUser(ctx, auth, scimUserWrite{
		SCIMID: rec.SCIMID, ExternalID: rec.ExternalID, UserName: rec.UserName, UserID: rec.UserID,
		Active: false, Revision: rec.Revision + 1, Update: true, RoleAtDeactivation: role,
		Tombstone: tombstone, Event: IdentityEventSCIMUserDeprovisioned, EventOn: true,
	})
}

func (s *Service) reactivateSCIMUser(ctx context.Context, auth *SCIMTokenAuth, rec *SCIMUserRecord) (*SCIMUserRecord, error) {
	_, domain, err := NormalizeSCIMUserName(rec.UserName)
	if err != nil {
		return nil, err
	}
	if err := s.domainVerifiedFor(ctx, auth.OrgID, domain); err != nil {
		return nil, err
	}
	role := rec.RoleAtDeactivation
	switch role {
	case "owner": // SCIM never restores an owner
		role = "admin"
	case "admin", "member", "viewer":
	default:
		role = "member"
	}
	res, err := s.scimProvisioner.ProvisionMember(ctx, SCIMProvisionRequest{
		OperationID:    scimOperationID(auth.OrgID, rec.SCIMID, true, rec.Revision),
		OrganizationID: auth.OrgID, ActorUserID: auth.CreatedBy, Email: rec.UserName, Role: role,
	})
	if err != nil {
		return nil, err
	}
	return s.repo.upsertSCIMUser(ctx, auth, scimUserWrite{
		SCIMID: rec.SCIMID, ExternalID: rec.ExternalID, UserName: rec.UserName, UserID: res.UserID,
		Active: true, Revision: rec.Revision + 1, Update: true,
		Event: IdentityEventSCIMUserReactivated, EventOn: true,
	})
}

// DeleteSCIMUser is DELETE /Users/{id}: deprovision if active, keep a tombstone.
// Deleting an unknown or already deleted user succeeds.
func (s *Service) DeleteSCIMUser(ctx context.Context, auth *SCIMTokenAuth, scimID string) error {
	if err := s.requireSCIM(); err != nil {
		return err
	}
	rec, err := s.GetSCIMUser(ctx, auth, scimID)
	if err != nil {
		if errors.Is(err, ErrSCIMUserNotFound) {
			return nil
		}
		return err
	}
	if rec.DeletedAt != nil {
		return nil
	}
	if rec.Active {
		_, err = s.deactivateSCIMUser(ctx, auth, rec, true)
		return err
	}
	_, err = s.repo.upsertSCIMUser(ctx, auth, scimUserWrite{
		SCIMID: rec.SCIMID, ExternalID: rec.ExternalID, UserName: rec.UserName, UserID: rec.UserID,
		Active: false, Revision: rec.Revision + 1, Update: true, RoleAtDeactivation: rec.RoleAtDeactivation,
		Tombstone: true,
	})
	return err
}

// --- auth-core client ---

func (c *AuthCoreIdentityClient) ProvisionMember(ctx context.Context, req SCIMProvisionRequest) (*SCIMProvisionResult, error) {
	var out SCIMProvisionResult
	if err := c.doAt(ctx, "/api/v1/internal/scim/memberships", "POST", "/provision", url.Values{}, req, &out); err != nil {
		return nil, err
	}
	return &out, nil
}

func (c *AuthCoreIdentityClient) DeprovisionMember(ctx context.Context, req SCIMDeprovisionRequest) (*SCIMDeprovisionResult, error) {
	var out SCIMDeprovisionResult
	if err := c.doAt(ctx, "/api/v1/internal/scim/memberships", "POST", "/deprovision", url.Values{}, req, &out); err != nil {
		return nil, err
	}
	return &out, nil
}

// --- repository ---

func (r *Repository) insertSCIMToken(ctx context.Context, orgID, actorRole string, token SCIMToken, hash string) error {
	return r.db.WithOrgScope(ctx, orgID, func(tx pgx.Tx) error {
		if err := lockOrganizationLifecycle(ctx, tx, orgID); err != nil {
			return err
		}
		var verified, active int
		if err := tx.QueryRow(ctx, `SELECT COUNT(*) FROM organization_domains WHERE org_id = $1 AND status = 'verified'`, orgID).Scan(&verified); err != nil {
			return fmt.Errorf("count verified domains: %w", err)
		}
		if verified == 0 {
			return ErrSCIMNoVerifiedDomain
		}
		if err := tx.QueryRow(ctx, `
SELECT COUNT(*) FROM organization_scim_tokens
WHERE org_id = $1 AND revoked_at IS NULL AND expires_at > NOW()`, orgID).Scan(&active); err != nil {
			return fmt.Errorf("count scim tokens: %w", err)
		}
		if active >= MaxActiveSCIMTokens {
			return ErrSCIMTokenLimit
		}
		if _, err := tx.Exec(ctx, `
INSERT INTO organization_scim_tokens (id, org_id, name, token_prefix, token_hash, created_by, expires_at, created_at)
VALUES ($1, $2, $3, $4, $5, $6, $7, $8)`,
			token.ID, orgID, token.Name, token.Prefix, hash, token.CreatedBy, token.ExpiresAt, token.CreatedAt); err != nil {
			return fmt.Errorf("insert scim token: %w", err)
		}
		return enqueueIdentityAudit(ctx, tx, identityAuditChange{
			Event: IdentityEventSCIMTokenCreated, OrgID: orgID, ActorID: token.CreatedBy, ActorRole: actorRole,
			Resource: token.ID, Details: map[string]any{"token_id": token.ID, "expires_at": token.ExpiresAt.Format(time.RFC3339)},
		})
	})
}

func (r *Repository) listSCIMTokens(ctx context.Context, orgID string) ([]SCIMToken, error) {
	out := []SCIMToken{}
	err := r.db.WithOrgScope(ctx, orgID, func(tx pgx.Tx) error {
		rows, err := tx.Query(ctx, `
SELECT id, name, token_prefix, created_by, created_at, expires_at, last_used_at, revoked_at
FROM organization_scim_tokens WHERE org_id = $1 ORDER BY created_at DESC, id LIMIT 50`, orgID)
		if err != nil {
			return fmt.Errorf("list scim tokens: %w", err)
		}
		defer rows.Close()
		for rows.Next() {
			var t SCIMToken
			if err := rows.Scan(&t.ID, &t.Name, &t.Prefix, &t.CreatedBy, &t.CreatedAt, &t.ExpiresAt, &t.LastUsedAt, &t.RevokedAt); err != nil {
				return fmt.Errorf("scan scim token: %w", err)
			}
			out = append(out, t)
		}
		return rows.Err()
	})
	return out, err
}

func (r *Repository) revokeSCIMToken(ctx context.Context, orgID, actorID, actorRole, tokenID string) error {
	return r.db.WithOrgScope(ctx, orgID, func(tx pgx.Tx) error {
		var revokedAt *time.Time
		err := tx.QueryRow(ctx, `
SELECT revoked_at FROM organization_scim_tokens WHERE org_id = $1 AND id = $2 FOR UPDATE`, orgID, tokenID).Scan(&revokedAt)
		if errors.Is(err, pgx.ErrNoRows) {
			return ErrSCIMTokenNotFound
		}
		if err != nil {
			return fmt.Errorf("read scim token: %w", err)
		}
		if revokedAt != nil {
			return nil
		}
		if _, err := tx.Exec(ctx, `
UPDATE organization_scim_tokens SET revoked_at = NOW(), revoked_by = $3 WHERE org_id = $1 AND id = $2`,
			orgID, tokenID, actorID); err != nil {
			return fmt.Errorf("revoke scim token: %w", err)
		}
		return enqueueIdentityAudit(ctx, tx, identityAuditChange{
			Event: IdentityEventSCIMTokenRevoked, OrgID: orgID, ActorID: actorID, ActorRole: actorRole,
			Resource: tokenID, Details: map[string]any{"token_id": tokenID},
		})
	})
}

func (r *Repository) scimStatus(ctx context.Context, orgID string) (*SCIMStatus, error) {
	st := &SCIMStatus{Enabled: true, MaxTokens: MaxActiveSCIMTokens}
	err := r.db.WithOrgScope(ctx, orgID, func(tx pgx.Tx) error {
		if err := tx.QueryRow(ctx, `SELECT COUNT(*) FROM organization_domains WHERE org_id = $1 AND status = 'verified'`, orgID).Scan(&st.VerifiedDomains); err != nil {
			return fmt.Errorf("count verified domains: %w", err)
		}
		if err := tx.QueryRow(ctx, `
SELECT COUNT(*) FILTER (WHERE revoked_at IS NULL AND expires_at > NOW()), MAX(last_used_at)
FROM organization_scim_tokens WHERE org_id = $1`, orgID).Scan(&st.ActiveTokens, &st.LastUsedAt); err != nil {
			return fmt.Errorf("count scim tokens: %w", err)
		}
		if err := tx.QueryRow(ctx, `
SELECT COUNT(*), COUNT(*) FILTER (WHERE active)
FROM organization_scim_users WHERE org_id = $1 AND deleted_at IS NULL`, orgID).Scan(&st.ManagedUsers, &st.ActiveUsers); err != nil {
			return fmt.Errorf("count scim users: %w", err)
		}
		return nil
	})
	return st, err
}

// resolveSCIMToken runs on the owner pool: the organisation is what is being
// looked up. last_used_at is touched at most once a minute.
func (r *Repository) resolveSCIMToken(ctx context.Context, hash string) (*SCIMTokenAuth, error) {
	var auth SCIMTokenAuth
	var lastUsed *time.Time
	err := r.pool.QueryRow(ctx, `
SELECT t.id, t.org_id, t.created_by, t.last_used_at
FROM organization_scim_tokens t
JOIN organizations o ON o.id = t.org_id
WHERE t.token_hash = $1 AND t.revoked_at IS NULL AND t.expires_at > NOW() AND o.deleted_at IS NULL`, hash).
		Scan(&auth.TokenID, &auth.OrgID, &auth.CreatedBy, &lastUsed)
	if errors.Is(err, pgx.ErrNoRows) {
		return nil, ErrSCIMUnauthorized
	}
	if err != nil {
		return nil, fmt.Errorf("resolve scim token: %w", err)
	}
	if lastUsed == nil || time.Since(*lastUsed) > scimLastUsedEvery {
		if _, err := r.pool.Exec(ctx, `UPDATE organization_scim_tokens SET last_used_at = NOW() WHERE id = $1`, auth.TokenID); err != nil {
			return nil, fmt.Errorf("touch scim token: %w", err)
		}
	}
	return &auth, nil
}

const scimUserColumns = `scim_id::text, COALESCE(external_id, ''), user_name, COALESCE(user_id, ''), active,
  COALESCE(role_at_deactivation, ''), deprovisioned_at, deleted_at, revision, created_at, updated_at`

func scanSCIMUser(row pgx.Row) (SCIMUserRecord, error) {
	var u SCIMUserRecord
	err := row.Scan(&u.SCIMID, &u.ExternalID, &u.UserName, &u.UserID, &u.Active, &u.RoleAtDeactivation,
		&u.DeprovisionedAt, &u.DeletedAt, &u.Revision, &u.CreatedAt, &u.UpdatedAt)
	return u, err
}

func (r *Repository) readSCIMUser(ctx context.Context, orgID, scimID string) (*SCIMUserRecord, error) {
	var rec SCIMUserRecord
	err := r.db.WithOrgScope(ctx, orgID, func(tx pgx.Tx) error {
		var err error
		rec, err = scanSCIMUser(tx.QueryRow(ctx, `SELECT `+scimUserColumns+` FROM organization_scim_users WHERE org_id = $1 AND scim_id = $2::uuid`, orgID, scimID))
		return err
	})
	if errors.Is(err, pgx.ErrNoRows) {
		return nil, ErrSCIMUserNotFound
	}
	if err != nil {
		return nil, fmt.Errorf("read scim user: %w", err)
	}
	return &rec, nil
}

func (r *Repository) findSCIMUserByName(ctx context.Context, orgID, userName string) (*SCIMUserRecord, error) {
	var rec SCIMUserRecord
	err := r.db.WithOrgScope(ctx, orgID, func(tx pgx.Tx) error {
		var err error
		rec, err = scanSCIMUser(tx.QueryRow(ctx, `SELECT `+scimUserColumns+` FROM organization_scim_users WHERE org_id = $1 AND user_name = $2`, orgID, userName))
		return err
	})
	if errors.Is(err, pgx.ErrNoRows) {
		return nil, nil
	}
	if err != nil {
		return nil, fmt.Errorf("find scim user: %w", err)
	}
	return &rec, nil
}

func (r *Repository) listSCIMUsers(ctx context.Context, orgID string, filter *SCIMFilter, offset, count int) ([]SCIMUserRecord, int, error) {
	out := []SCIMUserRecord{}
	total := 0
	column := ""
	value := ""
	if filter != nil {
		value = filter.Value
		if filter.Attribute == "userName" {
			column = "user_name"
		} else {
			column = "external_id"
		}
	}
	err := r.db.WithOrgScope(ctx, orgID, func(tx pgx.Tx) error {
		where := `org_id = $1 AND deleted_at IS NULL`
		args := []any{orgID}
		if column != "" {
			where += ` AND ` + column + ` = $2`
			args = append(args, value)
		}
		if err := tx.QueryRow(ctx, `SELECT COUNT(*) FROM organization_scim_users WHERE `+where, args...).Scan(&total); err != nil {
			return fmt.Errorf("count scim users: %w", err)
		}
		if count == 0 {
			return nil
		}
		args = append(args, count, offset)
		rows, err := tx.Query(ctx, fmt.Sprintf(`SELECT `+scimUserColumns+` FROM organization_scim_users WHERE %s ORDER BY created_at, scim_id LIMIT $%d OFFSET $%d`,
			where, len(args)-1, len(args)), args...)
		if err != nil {
			return fmt.Errorf("list scim users: %w", err)
		}
		defer rows.Close()
		for rows.Next() {
			u, err := scanSCIMUser(rows)
			if err != nil {
				return fmt.Errorf("scan scim user: %w", err)
			}
			out = append(out, u)
		}
		return rows.Err()
	})
	return out, total, err
}

// scimUserWrite is one state change of a SCIM user record plus its audit event.
type scimUserWrite struct {
	SCIMID, ExternalID, UserName, UserID string
	Active                               bool
	RoleAtDeactivation                   string
	Revision                             int64 // the revision the row holds after the write
	Update                               bool  // row exists (revision CAS on Revision-1)
	Revive                               bool  // row is a tombstone being reused
	Tombstone                            bool
	Event                                string
	EventOn                              bool
}

func (r *Repository) upsertSCIMUser(ctx context.Context, auth *SCIMTokenAuth, w scimUserWrite) (*SCIMUserRecord, error) {
	var rec SCIMUserRecord
	err := r.db.WithOrgScope(ctx, auth.OrgID, func(tx pgx.Tx) error {
		var row pgx.Row
		switch {
		case w.Update || w.Revive:
			row = tx.QueryRow(ctx, `
UPDATE organization_scim_users
SET external_id = NULLIF($3, ''), user_id = NULLIF($4, ''), active = $5,
    role_at_deactivation = CASE WHEN $5 THEN NULL ELSE COALESCE(NULLIF($6, ''), role_at_deactivation) END,
    deprovisioned_at = CASE WHEN $5 THEN NULL ELSE COALESCE(deprovisioned_at, NOW()) END,
    deleted_at = CASE WHEN $7 THEN NOW() ELSE NULL END,
    revision = $8, updated_at = NOW()
WHERE org_id = $1 AND scim_id = $2::uuid AND revision = $8 - 1
RETURNING `+scimUserColumns,
				auth.OrgID, w.SCIMID, w.ExternalID, w.UserID, w.Active, w.RoleAtDeactivation, w.Tombstone, w.Revision)
		default:
			row = tx.QueryRow(ctx, `
INSERT INTO organization_scim_users (org_id, scim_id, external_id, user_name, user_id, active, role_at_deactivation, deprovisioned_at, revision)
VALUES ($1, $2::uuid, NULLIF($3, ''), $4, NULLIF($5, ''), $6, NULL, CASE WHEN $6 THEN NULL ELSE NOW() END, $7)
RETURNING `+scimUserColumns,
				auth.OrgID, w.SCIMID, w.ExternalID, w.UserName, w.UserID, w.Active, w.Revision)
		}
		var err error
		rec, err = scanSCIMUser(row)
		if err != nil {
			return err
		}
		if w.Event == "" || !w.EventOn {
			return nil
		}
		return enqueueIdentityAudit(ctx, tx, identityAuditChange{
			Event: w.Event, OrgID: auth.OrgID, ActorID: auth.CreatedBy, ActorRole: scimActorRole,
			Resource: w.SCIMID, Details: map[string]any{"scim_id": w.SCIMID, "user_id": w.UserID, "token_id": auth.TokenID},
		})
	})
	if err != nil {
		var pgErr *pgconn.PgError
		if errors.As(err, &pgErr) && pgErr.Code == "23505" {
			return nil, ErrSCIMUniqueness
		}
		if errors.Is(err, pgx.ErrNoRows) {
			// A concurrent change moved the revision; the IdP retries.
			return nil, ErrSCIMMembershipConflict
		}
		return nil, fmt.Errorf("write scim user: %w", err)
	}
	return &rec, nil
}
