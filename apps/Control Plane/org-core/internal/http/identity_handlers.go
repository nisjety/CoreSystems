package http

import (
	"context"
	"encoding/json"
	"errors"
	"log"
	"net/http"
	"strings"
	"time"

	orgcore "github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/org"
	"github.com/gin-gonic/gin"
)

// domainVerificationFlagEnv switches the domain verification routes on. Anything
// but "1" or "true" leaves them unmounted (404), the default.
const domainVerificationFlagEnv = "ORG_DOMAIN_VERIFICATION_ENABLED"

// domainDNSResolversEnv optionally pins the resolvers used for the TXT proof, a
// comma separated list of host[:port]. Unset uses the system resolvers.
const domainDNSResolversEnv = "ORG_DOMAIN_DNS_RESOLVERS"

func domainVerificationEnabled(getenv func(string) string) bool {
	switch getenv(domainVerificationFlagEnv) {
	case "1", "true":
		return true
	}
	return false
}

func domainDNSResolvers(getenv func(string) string) []string {
	raw := strings.TrimSpace(getenv(domainDNSResolversEnv))
	if raw == "" {
		return nil
	}
	return strings.Split(raw, ",")
}

// identityDomainService is the seam the identity routes depend on. *org.Service
// satisfies it; handler tests use a fake so the real router and the real
// service-auth middleware run without a database.
type identityDomainService interface {
	ListIdentityDomains(ctx context.Context, orgID string) ([]orgcore.IdentityDomain, error)
	AddIdentityDomain(ctx context.Context, orgID, actorID, actorRole, domain string) (*orgcore.IdentityDomain, error)
	VerifyIdentityDomain(ctx context.Context, orgID, actorID, actorRole, domain string) (*orgcore.DomainVerifyResult, error)
	RevokeIdentityDomain(ctx context.Context, orgID, actorID, actorRole, domain string) error
	LookupVerifiedDomainOwner(ctx context.Context, domain string) (*orgcore.VerifiedDomainOwner, error)
}

// SetIdentityDomains wires the domain service. A server without it answers the
// routes 503 instead of serving a store it cannot read.
func (s *Server) SetIdentityDomains(service identityDomainService) {
	s.identity = service
}

// requireIdentityEnabled answers 404 on every identity route until the feature
// is on, so a deployment that never enables it exposes nothing.
func (s *Server) requireIdentityEnabled() gin.HandlerFunc {
	return func(c *gin.Context) {
		if !s.identityEnabled {
			c.AbortWithStatusJSON(http.StatusNotFound, gin.H{"error": gin.H{"code": "not_found", "message": "not found"}})
			return
		}
		c.Next()
	}
}

const (
	identityRequestTimeout = 8 * time.Second
	identityMaxBodyBytes   = 4 << 10
)

// writeIdentityError writes the typed envelope {error:{code,message,details}}.
func writeIdentityError(c *gin.Context, status int, code, message string, details gin.H) {
	if details == nil {
		details = gin.H{}
	}
	c.JSON(status, gin.H{"error": gin.H{"code": code, "message": message, "details": details}})
}

// authorizeIdentityAdmin resolves the caller and requires the owner or admin
// role from org-core's own membership table; the role header the gateway
// forwards is a hint, never authority, and a platform role is not admin here.
func (s *Server) authorizeIdentityAdmin(c *gin.Context, orgID string) (callerID, role string, ok bool) {
	callerID = strings.TrimSpace(c.GetHeader("x-user-id"))
	if callerID == "" {
		writeIdentityError(c, http.StatusUnauthorized, "unauthenticated", "user not authenticated", nil)
		return "", "", false
	}
	if s.callerRoles == nil {
		writeIdentityError(c, http.StatusServiceUnavailable, "identity_unavailable", "identity administration unavailable", nil)
		return "", "", false
	}
	role, err := s.callerRoles.CallerRole(c.Request.Context(), orgID, callerID)
	if err != nil {
		log.Printf("identity domains: caller role lookup failed for org=%s: %v", orgID, err)
		writeIdentityError(c, http.StatusInternalServerError, "internal_error", "failed to resolve caller role", nil)
		return "", "", false
	}
	switch normalized := strings.ToLower(strings.TrimSpace(role)); normalized {
	case "owner", "admin":
		return callerID, normalized, true
	default:
		writeIdentityError(c, http.StatusForbidden, "identity_admin_required", "organization owner or admin role required", nil)
	}
	return "", "", false
}

func (s *Server) identityAdminRequest(c *gin.Context) (orgID, actorID, actorRole string, ok bool) {
	c.Header("Cache-Control", "no-store")
	orgID = strings.TrimSpace(c.Param("id"))
	if orgID == "" {
		writeIdentityError(c, http.StatusBadRequest, "invalid_request", "organization id is required", nil)
		return "", "", "", false
	}
	actorID, actorRole, ok = s.authorizeIdentityAdmin(c, orgID)
	if !ok {
		return "", "", "", false
	}
	if s.identity == nil {
		writeIdentityError(c, http.StatusServiceUnavailable, "identity_unavailable", "identity administration unavailable", nil)
		return "", "", "", false
	}
	return orgID, actorID, actorRole, true
}

func writeIdentityDomainError(c *gin.Context, orgID, operation string, err error) {
	var invalid *orgcore.DomainValidationError
	switch {
	case errors.As(err, &invalid):
		writeIdentityError(c, http.StatusBadRequest, "invalid_domain", "the domain is not valid", gin.H{"reason": invalid.Reason})
	case errors.Is(err, orgcore.ErrDomainVerifiedElsewhere):
		writeIdentityError(c, http.StatusConflict, "domain_verified_elsewhere", "the domain is already verified by another organization", nil)
	case errors.Is(err, orgcore.ErrDomainLimitReached):
		writeIdentityError(c, http.StatusConflict, "domain_limit_reached", "an organization can have at most 10 domains", gin.H{"max": orgcore.MaxIdentityDomainsPerOrg})
	case errors.Is(err, orgcore.ErrDomainInUse):
		writeIdentityError(c, http.StatusConflict, "domain_in_use", "the domain backs the organization's enforced sso policy", nil)
	case errors.Is(err, orgcore.ErrDomainNotFound):
		writeIdentityError(c, http.StatusNotFound, "domain_not_found", "domain not found", nil)
	case errors.Is(err, orgcore.ErrDomainVerificationUnavailable):
		writeIdentityError(c, http.StatusServiceUnavailable, "identity_unavailable", "domain verification unavailable", nil)
	case errors.Is(err, orgcore.ErrNotFound):
		writeIdentityError(c, http.StatusNotFound, "organization_not_found", "organization not found", nil)
	default:
		log.Printf("identity domains: %s failed for org=%s: %v", operation, orgID, err)
		writeIdentityError(c, http.StatusInternalServerError, "internal_error", "failed to "+operation+" domain", nil)
	}
}

// listIdentityDomains serves GET /orgs/:id/domains.
func (s *Server) listIdentityDomains(c *gin.Context) {
	orgID, _, _, ok := s.identityAdminRequest(c)
	if !ok {
		return
	}
	ctx, cancel := context.WithTimeout(c.Request.Context(), identityRequestTimeout)
	defer cancel()
	domains, err := s.identity.ListIdentityDomains(ctx, orgID)
	if err != nil {
		writeIdentityDomainError(c, orgID, "list", err)
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": gin.H{"domains": domains}})
}

// addIdentityDomain serves POST /orgs/:id/domains.
func (s *Server) addIdentityDomain(c *gin.Context) {
	orgID, actorID, actorRole, ok := s.identityAdminRequest(c)
	if !ok {
		return
	}
	var req struct {
		Domain string `json:"domain"`
	}
	decoder := json.NewDecoder(http.MaxBytesReader(c.Writer, c.Request.Body, identityMaxBodyBytes))
	decoder.DisallowUnknownFields()
	err := decoder.Decode(&req)
	if err == nil {
		err = ensureJSONEOF(decoder)
	}
	if err != nil {
		writeIdentityError(c, http.StatusBadRequest, "invalid_request", "invalid request body", nil)
		return
	}
	ctx, cancel := context.WithTimeout(c.Request.Context(), identityRequestTimeout)
	defer cancel()
	domain, err := s.identity.AddIdentityDomain(ctx, orgID, actorID, actorRole, req.Domain)
	if err != nil {
		writeIdentityDomainError(c, orgID, "add", err)
		return
	}
	status := http.StatusCreated
	if domain.Status != "pending" {
		status = http.StatusOK
	}
	log.Printf("identity domains: org=%s domain=%s status=%s added by actor=%s", orgID, domain.Domain, domain.Status, actorID)
	c.JSON(status, gin.H{"data": domain})
}

// verifyIdentityDomain serves POST /orgs/:id/domains/:domain/verify.
func (s *Server) verifyIdentityDomain(c *gin.Context) {
	orgID, actorID, actorRole, ok := s.identityAdminRequest(c)
	if !ok {
		return
	}
	ctx, cancel := context.WithTimeout(c.Request.Context(), identityRequestTimeout)
	defer cancel()
	result, err := s.identity.VerifyIdentityDomain(ctx, orgID, actorID, actorRole, c.Param("domain"))
	if err != nil {
		writeIdentityDomainError(c, orgID, "verify", err)
		return
	}
	log.Printf("identity domains: org=%s domain=%s verify status=%s reason=%s by actor=%s", orgID, result.Domain, result.Status, result.Reason, actorID)
	c.JSON(http.StatusOK, gin.H{"data": result})
}

// revokeIdentityDomain serves DELETE /orgs/:id/domains/:domain.
func (s *Server) revokeIdentityDomain(c *gin.Context) {
	orgID, actorID, actorRole, ok := s.identityAdminRequest(c)
	if !ok {
		return
	}
	ctx, cancel := context.WithTimeout(c.Request.Context(), identityRequestTimeout)
	defer cancel()
	if err := s.identity.RevokeIdentityDomain(ctx, orgID, actorID, actorRole, c.Param("domain")); err != nil {
		writeIdentityDomainError(c, orgID, "revoke", err)
		return
	}
	log.Printf("identity domains: org=%s domain=%s revoked by actor=%s", orgID, strings.ToLower(c.Param("domain")), actorID)
	c.JSON(http.StatusOK, gin.H{"data": gin.H{"status": "revoked"}})
}

// getVerifiedDomainOwnerInternal serves GET /internal/orgs/by-domain/:domain.
// Access requires org:domain:read:any (serviceScopesForRequest), held by
// auth-core alone. It answers only for a verified claim and carries no member
// or provider data.
func (s *Server) getVerifiedDomainOwnerInternal(c *gin.Context) {
	c.Header("Cache-Control", "no-store")
	if s.identity == nil {
		writeIdentityError(c, http.StatusServiceUnavailable, "identity_unavailable", "identity administration unavailable", nil)
		return
	}
	ctx, cancel := context.WithTimeout(c.Request.Context(), identityRequestTimeout)
	defer cancel()
	owner, err := s.identity.LookupVerifiedDomainOwner(ctx, c.Param("domain"))
	if err != nil {
		writeIdentityDomainError(c, "", "look up", err)
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": owner})
}
