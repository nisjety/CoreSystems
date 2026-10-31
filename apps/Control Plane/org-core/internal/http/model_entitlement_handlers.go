package http

import (
	"context"
	"encoding/json"
	"errors"
	"log"
	"net/http"
	"strings"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/modelentitlement"
	"github.com/gin-gonic/gin"
)

// modelEntitlementService is the seam the model policy routes depend on.
// *modelentitlement.Service satisfies it; the handler tests use a fake so the
// real router and the real service-auth middleware run without a database.
type modelEntitlementService interface {
	Get(ctx context.Context, orgID string) (*modelentitlement.PolicySet, error)
	Put(ctx context.Context, orgID, actorID string, req modelentitlement.PutRequest) (*modelentitlement.PolicySet, error)
	Resolve(ctx context.Context, orgID, userID string) (modelentitlement.Resolution, error)
}

// callerRoleReader is how a handler learns the caller's role. The role always
// comes from org-core's own membership table: the role header the gateway
// forwards is a hint and is never authority (WS10 D13).
type callerRoleReader interface {
	CallerRole(ctx context.Context, orgID, userID string) (string, error)
}

// SetModelEntitlements wires the model policy service. It is a setter beside
// retentionPosture so NewServer keeps its signature, and a server without it
// answers the routes 503 instead of serving a policy it cannot read.
func (s *Server) SetModelEntitlements(service modelEntitlementService) {
	s.modelEntitlements = service
}

// modelEntitlementTimeout bounds one policy read or write so a slow database
// cannot hold the handler open; inference-core's own client gives up after 1 s
// and treats that as "unknown", never as "unrestricted".
const modelEntitlementTimeout = 5 * time.Second

// maxModelEntitlementUserIDLength matches the delegation header limit.
const maxModelEntitlementUserIDLength = 128

// authorizeModelPolicyReader resolves the caller and requires an active member.
// It writes the 401/403/500 itself and returns ok=false when the request must
// stop. The role is the one from the membership table.
func (s *Server) authorizeModelPolicyReader(c *gin.Context, orgID string) (callerID, role string, ok bool) {
	callerID = strings.TrimSpace(c.GetHeader("x-user-id"))
	if callerID == "" {
		c.JSON(http.StatusUnauthorized, gin.H{"error": "user not authenticated"})
		return "", "", false
	}
	if s.callerRoles == nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "model entitlement unavailable"})
		return "", "", false
	}
	role, err := s.callerRoles.CallerRole(c.Request.Context(), orgID, callerID)
	if err != nil {
		log.Printf("model entitlement: caller role lookup failed for org=%s: %v", orgID, err)
		c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to resolve caller role"})
		return "", "", false
	}
	if strings.TrimSpace(role) == "" {
		c.JSON(http.StatusForbidden, gin.H{"error": "forbidden: not a member of this organization"})
		return "", "", false
	}
	return callerID, role, true
}

// authorizeModelPolicyAdmin additionally requires the owner or admin role. The
// BFF checks the same thing first; this is the second check, against the
// membership table, because the forwarded role is not authority.
func (s *Server) authorizeModelPolicyAdmin(c *gin.Context, orgID string) (callerID string, ok bool) {
	callerID, role, ok := s.authorizeModelPolicyReader(c, orgID)
	if !ok {
		return "", false
	}
	if !modelPolicyAdminRole(role) {
		c.JSON(http.StatusForbidden, gin.H{"error": "organization owner or admin role required"})
		return "", false
	}
	return callerID, true
}

func modelPolicyAdminRole(role string) bool {
	switch modelentitlement.NormalizeRole(role) {
	case "owner", "admin":
		return true
	}
	return false
}

// getModelEntitlement serves GET /api/v1/organizations/:id/model-entitlement
// (and the /orgs/:id proxy form): the stored policy set, for the admin section.
func (s *Server) getModelEntitlement(c *gin.Context) {
	c.Header("Cache-Control", "no-store")
	orgID := strings.TrimSpace(c.Param("id"))
	if orgID == "" {
		c.JSON(http.StatusBadRequest, gin.H{"error": "organization id is required"})
		return
	}
	if _, ok := s.authorizeModelPolicyAdmin(c, orgID); !ok {
		return
	}
	if s.modelEntitlements == nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "model entitlement unavailable"})
		return
	}
	ctx, cancel := context.WithTimeout(c.Request.Context(), modelEntitlementTimeout)
	defer cancel()
	set, err := s.modelEntitlements.Get(ctx, orgID)
	if err != nil {
		log.Printf("model entitlement: read failed for org=%s: %v", orgID, err)
		c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to read model entitlement"})
		return
	}
	c.JSON(http.StatusOK, set)
}

// putModelEntitlement serves PUT /api/v1/organizations/:id/model-entitlement.
// The body replaces the whole set and must carry the revision it was edited
// from: a stale editor gets 409 with the current revision, never a silent
// overwrite. The history row is written in the same transaction as the policy.
func (s *Server) putModelEntitlement(c *gin.Context) {
	c.Header("Cache-Control", "no-store")
	orgID := strings.TrimSpace(c.Param("id"))
	if orgID == "" {
		c.JSON(http.StatusBadRequest, gin.H{"error": "organization id is required"})
		return
	}
	actorID, ok := s.authorizeModelPolicyAdmin(c, orgID)
	if !ok {
		return
	}
	if s.modelEntitlements == nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "model entitlement unavailable"})
		return
	}

	var req modelentitlement.PutRequest
	decoder := json.NewDecoder(http.MaxBytesReader(c.Writer, c.Request.Body, modelentitlement.MaxBodyBytes))
	decoder.DisallowUnknownFields()
	err := decoder.Decode(&req)
	if err == nil {
		err = ensureJSONEOF(decoder)
	}
	if err != nil {
		var tooLarge *http.MaxBytesError
		if errors.As(err, &tooLarge) {
			c.JSON(http.StatusRequestEntityTooLarge, gin.H{"error": "request body too large", "code": "body_too_large"})
			return
		}
		c.JSON(http.StatusBadRequest, gin.H{"error": "invalid request body", "code": "invalid_body"})
		return
	}

	ctx, cancel := context.WithTimeout(c.Request.Context(), modelEntitlementTimeout)
	defer cancel()
	saved, err := s.modelEntitlements.Put(ctx, orgID, actorID, req)
	if err != nil {
		var invalid *modelentitlement.ValidationError
		var conflict *modelentitlement.RevisionConflictError
		switch {
		case errors.As(err, &invalid):
			c.JSON(http.StatusBadRequest, gin.H{"error": "invalid model policy", "code": "invalid_model_policy", "details": invalid.Errors})
		case errors.As(err, &conflict):
			c.JSON(http.StatusConflict, gin.H{"error": "the model policy changed since it was read", "code": "revision_conflict", "current_revision": conflict.Current})
		case errors.Is(err, modelentitlement.ErrOrganizationNotFound):
			c.JSON(http.StatusNotFound, gin.H{"error": "organization not found"})
		default:
			log.Printf("model entitlement: write failed for org=%s: %v", orgID, err)
			c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to write model entitlement"})
		}
		return
	}
	log.Printf("model entitlement: org=%s revision=%d written by actor=%s", orgID, saved.Revision, actorID)
	c.JSON(http.StatusOK, saved)
}

// modelEntitlementExplanation is the explain body: one resolution, flattened,
// with the organisation and the user it was computed for.
type modelEntitlementExplanation struct {
	OrganizationID string `json:"organization_id"`
	UserID         string `json:"user_id"`
	modelentitlement.Resolution
}

// explainModelEntitlement serves GET .../model-entitlement/explain[?user_id=].
// Any active member may explain their own access. Explaining someone else is an
// owner or admin act, because it discloses that person's role and policy.
func (s *Server) explainModelEntitlement(c *gin.Context) {
	c.Header("Cache-Control", "no-store")
	orgID := strings.TrimSpace(c.Param("id"))
	if orgID == "" {
		c.JSON(http.StatusBadRequest, gin.H{"error": "organization id is required"})
		return
	}
	callerID, role, ok := s.authorizeModelPolicyReader(c, orgID)
	if !ok {
		return
	}
	target := callerID
	if requested, present := c.GetQuery("user_id"); present {
		requested = strings.TrimSpace(requested)
		if requested == "" || len(requested) > maxModelEntitlementUserIDLength {
			c.JSON(http.StatusBadRequest, gin.H{"error": "user_id is invalid"})
			return
		}
		if requested != callerID {
			if !modelPolicyAdminRole(role) {
				c.JSON(http.StatusForbidden, gin.H{"error": "organization owner or admin role required"})
				return
			}
			target = requested
		}
	}
	if s.modelEntitlements == nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "model entitlement unavailable"})
		return
	}

	ctx, cancel := context.WithTimeout(c.Request.Context(), modelEntitlementTimeout)
	defer cancel()
	resolution, err := s.modelEntitlements.Resolve(ctx, orgID, target)
	if err != nil {
		log.Printf("model entitlement: explain failed for org=%s: %v", orgID, err)
		c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to explain model entitlement"})
		return
	}
	c.JSON(http.StatusOK, modelEntitlementExplanation{OrganizationID: orgID, UserID: target, Resolution: resolution})
}

// getModelEntitlementInternal serves
// GET /internal/orgs/:orgId/model-entitlement?user_id=, inference-core's pull.
//
// Access requires org:model-entitlement:read:any (serviceScopesForRequest),
// which no other credential holds except model-gateway's explain use. The answer
// is computed from the database on every call and marked no-store: it decides
// which model a person may use, and the only bounded cache is inference-core's
// own (30 s TTL, stale bounds in ADR-0005).
//
// user_id is optional. Absent, the organisation default applies (a service
// call). Present but blank is refused rather than read as absent, so a caller
// bug cannot widen a person's access to the organisation default. A 5xx is
// "unknown" to the caller, never "unrestricted".
func (s *Server) getModelEntitlementInternal(c *gin.Context) {
	c.Header("Cache-Control", "no-store")
	orgID := strings.TrimSpace(c.Param("orgId"))
	if orgID == "" {
		c.JSON(http.StatusBadRequest, gin.H{"error": "organization id is required"})
		return
	}
	userID := ""
	if requested, present := c.GetQuery("user_id"); present {
		userID = strings.TrimSpace(requested)
		if userID == "" || len(userID) > maxModelEntitlementUserIDLength {
			c.JSON(http.StatusBadRequest, gin.H{"error": "user_id is invalid"})
			return
		}
	}
	if s.modelEntitlements == nil {
		log.Printf("model entitlement: no service configured for org=%s", orgID)
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "model entitlement unavailable"})
		return
	}

	ctx, cancel := context.WithTimeout(c.Request.Context(), modelEntitlementTimeout)
	defer cancel()
	resolution, err := s.modelEntitlements.Resolve(ctx, orgID, userID)
	if err != nil {
		log.Printf("model entitlement: resolve failed for org=%s: %v", orgID, err)
		c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to resolve model entitlement"})
		return
	}
	c.JSON(http.StatusOK, resolution)
}
