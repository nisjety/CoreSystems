package http

import (
	"context"
	"encoding/json"
	"errors"
	"log"
	"net/http"
	"strings"

	orgcore "github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/org"
	"github.com/gin-gonic/gin"
)

// SSO policy and provider administration (WS05-S7). The routes sit behind the
// same ORG_DOMAIN_VERIFICATION_ENABLED gate as the domain routes: an SSO policy
// is meaningless without a proven domain.

// identitySSOService is the seam the SSO routes depend on. *org.Service
// satisfies it; handler tests use a fake.
type identitySSOService interface {
	GetSSOPolicy(ctx context.Context, orgID, actorID string) (*orgcore.SSOPolicyView, error)
	PutSSOPolicy(ctx context.Context, orgID, actorID, actorRole string, in orgcore.SSOPolicyInput) (*orgcore.SSOPolicyView, error)
	AddSSOProvider(ctx context.Context, orgID, actorID, actorRole string, in orgcore.SSOProviderInput) (*orgcore.SSOProvider, error)
	RemoveSSOProvider(ctx context.Context, orgID, actorID, actorRole, providerID string) error
	LookupSSOPolicyByDomain(ctx context.Context, domain string) (*orgcore.SSOPolicyByDomain, error)
}

// SetIdentitySSO wires the SSO service. A server without it answers 503.
func (s *Server) SetIdentitySSO(service identitySSOService) {
	s.sso = service
}

func (s *Server) ssoAdminRequest(c *gin.Context) (orgID, actorID, actorRole string, ok bool) {
	orgID, actorID, actorRole, ok = s.identityAdminRequest(c)
	if !ok {
		return "", "", "", false
	}
	if s.sso == nil {
		writeIdentityError(c, http.StatusServiceUnavailable, "identity_unavailable", "identity administration unavailable", nil)
		return "", "", "", false
	}
	return orgID, actorID, actorRole, true
}

func decodeSSOBody(c *gin.Context, into any) bool {
	decoder := json.NewDecoder(http.MaxBytesReader(c.Writer, c.Request.Body, identityMaxBodyBytes))
	decoder.DisallowUnknownFields()
	err := decoder.Decode(into)
	if err == nil {
		err = ensureJSONEOF(decoder)
	}
	if err != nil {
		writeIdentityError(c, http.StatusBadRequest, "invalid_request", "invalid request body", nil)
		return false
	}
	return true
}

func writeSSOError(c *gin.Context, orgID, operation string, err error) {
	var bad *orgcore.SSORequestError
	var enforce *orgcore.SSOEnforceError
	switch {
	case errors.As(err, &bad):
		writeIdentityError(c, http.StatusBadRequest, "invalid_request", "the request is not valid", gin.H{"reason": bad.Reason})
	case errors.As(err, &enforce):
		writeIdentityError(c, http.StatusUnprocessableEntity, enforce.Code, "enforcement preconditions are not met", nil)
	case errors.Is(err, orgcore.ErrSSOPolicyRevisionConflict):
		writeIdentityError(c, http.StatusConflict, "policy_revision_conflict", "the policy changed; reload and retry", nil)
	case errors.Is(err, orgcore.ErrSSOProviderExists):
		writeIdentityError(c, http.StatusConflict, "provider_exists", "a provider with this id or domain already exists", nil)
	case errors.Is(err, orgcore.ErrSSOInvalidIssuer):
		writeIdentityError(c, http.StatusBadRequest, "invalid_issuer", "the issuer must be an https url", nil)
	case errors.Is(err, orgcore.ErrSSODomainNotVerified):
		writeIdentityError(c, http.StatusUnprocessableEntity, "domain_not_verified", "the domain is not verified for this organization", nil)
	case errors.Is(err, orgcore.ErrSSOLastProviderEnforced):
		writeIdentityError(c, http.StatusConflict, "last_provider_enforced", "the last provider of an enforced policy cannot be removed", nil)
	case errors.Is(err, orgcore.ErrSSOProviderNotFound):
		writeIdentityError(c, http.StatusNotFound, "provider_not_found", "provider not found", nil)
	case errors.Is(err, orgcore.ErrSSOUnavailable), errors.Is(err, orgcore.ErrDomainVerificationUnavailable):
		writeIdentityError(c, http.StatusServiceUnavailable, "identity_unavailable", "sso administration unavailable", nil)
	case errors.Is(err, orgcore.ErrNotFound):
		writeIdentityError(c, http.StatusNotFound, "organization_not_found", "organization not found", nil)
	default:
		log.Printf("identity sso: %s failed for org=%s: %v", operation, orgID, err)
		writeIdentityError(c, http.StatusInternalServerError, "internal_error", "failed to "+operation, nil)
	}
}

// getSSOPolicy serves GET /orgs/:id/sso/policy.
func (s *Server) getSSOPolicy(c *gin.Context) {
	orgID, actorID, _, ok := s.ssoAdminRequest(c)
	if !ok {
		return
	}
	ctx, cancel := context.WithTimeout(c.Request.Context(), identityRequestTimeout)
	defer cancel()
	view, err := s.sso.GetSSOPolicy(ctx, orgID, actorID)
	if err != nil {
		writeSSOError(c, orgID, "read sso policy", err)
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": view})
}

// putSSOPolicy serves PUT /orgs/:id/sso/policy.
func (s *Server) putSSOPolicy(c *gin.Context) {
	orgID, actorID, actorRole, ok := s.ssoAdminRequest(c)
	if !ok {
		return
	}
	var req struct {
		Mode              string   `json:"mode"`
		BreakGlassUserIDs []string `json:"break_glass_user_ids"`
		ExpectedRevision  *int64   `json:"expected_revision"`
	}
	if !decodeSSOBody(c, &req) {
		return
	}
	if req.ExpectedRevision == nil {
		writeIdentityError(c, http.StatusBadRequest, "invalid_request", "expected_revision is required", nil)
		return
	}
	// The SSO flows can take a few round trips to auth-core.
	ctx, cancel := context.WithTimeout(c.Request.Context(), 4*identityRequestTimeout)
	defer cancel()
	view, err := s.sso.PutSSOPolicy(ctx, orgID, actorID, actorRole, orgcore.SSOPolicyInput{
		Mode: strings.ToLower(strings.TrimSpace(req.Mode)), BreakGlassUserIDs: req.BreakGlassUserIDs,
		ExpectedRevision: *req.ExpectedRevision,
	})
	if err != nil {
		writeSSOError(c, orgID, "update sso policy", err)
		return
	}
	log.Printf("identity sso: org=%s policy mode=%s revision=%d set by actor=%s", orgID, view.Mode, view.Revision, actorID)
	c.JSON(http.StatusOK, gin.H{"data": view})
}

// addSSOProvider serves POST /orgs/:id/sso/providers. The client secret is
// write-only: it is never logged and never echoed.
func (s *Server) addSSOProvider(c *gin.Context) {
	orgID, actorID, actorRole, ok := s.ssoAdminRequest(c)
	if !ok {
		return
	}
	var req struct {
		ProviderID   string `json:"provider_id"`
		Domain       string `json:"domain"`
		Type         string `json:"type"`
		Issuer       string `json:"issuer"`
		ClientID     string `json:"client_id"`
		ClientSecret string `json:"client_secret"`
	}
	if !decodeSSOBody(c, &req) {
		return
	}
	ctx, cancel := context.WithTimeout(c.Request.Context(), 2*identityRequestTimeout)
	defer cancel()
	provider, err := s.sso.AddSSOProvider(ctx, orgID, actorID, actorRole, orgcore.SSOProviderInput{
		ProviderID: req.ProviderID, Domain: req.Domain, Type: req.Type, Issuer: req.Issuer,
		ClientID: req.ClientID, ClientSecret: req.ClientSecret,
	})
	if err != nil {
		writeSSOError(c, orgID, "add sso provider", err)
		return
	}
	log.Printf("identity sso: org=%s provider=%s domain=%s added by actor=%s", orgID, provider.ProviderID, provider.Domain, actorID)
	c.JSON(http.StatusCreated, gin.H{"data": provider})
}

// removeSSOProvider serves DELETE /orgs/:id/sso/providers/:providerId.
func (s *Server) removeSSOProvider(c *gin.Context) {
	orgID, actorID, actorRole, ok := s.ssoAdminRequest(c)
	if !ok {
		return
	}
	ctx, cancel := context.WithTimeout(c.Request.Context(), 2*identityRequestTimeout)
	defer cancel()
	if err := s.sso.RemoveSSOProvider(ctx, orgID, actorID, actorRole, c.Param("providerId")); err != nil {
		writeSSOError(c, orgID, "remove sso provider", err)
		return
	}
	log.Printf("identity sso: org=%s provider=%s removed by actor=%s", orgID, strings.ToLower(c.Param("providerId")), actorID)
	c.JSON(http.StatusOK, gin.H{"data": gin.H{"status": "removed"}})
}

// getSSOPolicyByDomainInternal serves GET /internal/orgs/by-domain/:domain/sso-policy
// for auth-core's enforcement hook (scope org:domain:read:any). A domain with no
// verified owner is a 404 and means "nothing to enforce". No member or provider data.
func (s *Server) getSSOPolicyByDomainInternal(c *gin.Context) {
	c.Header("Cache-Control", "no-store")
	if s.sso == nil {
		writeIdentityError(c, http.StatusServiceUnavailable, "identity_unavailable", "identity administration unavailable", nil)
		return
	}
	ctx, cancel := context.WithTimeout(c.Request.Context(), identityRequestTimeout)
	defer cancel()
	policy, err := s.sso.LookupSSOPolicyByDomain(ctx, c.Param("domain"))
	if err != nil {
		writeIdentityDomainError(c, "", "look up sso policy", err)
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": policy})
}
