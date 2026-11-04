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
	"github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/retention"
	"github.com/gin-gonic/gin"
)

// retentionService is the seam the retention routes depend on (EP-9 C1).
// *retention.Service satisfies it; the handler tests use the real service over an
// in-memory store, so the real routes and service-auth middleware run without a
// database.
type retentionService interface {
	Policy(ctx context.Context, orgID string) (*retention.Policy, error)
	PutPolicy(ctx context.Context, orgID string, actor retention.Actor, req retention.PutRequest) (*retention.Policy, error)
	Holds(ctx context.Context, orgID string) (retention.HoldList, error)
	PlaceHold(ctx context.Context, orgID string, actor retention.Actor, req retention.PlaceHoldRequest) (retention.PlaceResult, error)
	ReleaseHold(ctx context.Context, orgID, holdID string, actor retention.Actor) (retention.Hold, error)
	Check(ctx context.Context, orgID string, req retention.CheckRequest) (retention.CheckResult, error)
}

// SetRetention wires the retention service (EP-9 C1). With the switch on and no
// service the routes answer 503 instead of serving a policy they cannot read.
func (s *Server) SetRetention(service retentionService) {
	s.retention = service
}

const (
	// retentionTimeout bounds one admin read or write.
	retentionTimeout = 5 * time.Second
	// retentionCheckTimeout bounds the sweeper's check, as the other internal
	// reads are bounded, so a slow database fails the pass instead of holding it.
	retentionCheckTimeout = 3 * time.Second
)

// The roles each retention act needs, read from org-core's membership table. An
// admin sets the window; a hold is placed and released by an owner or a
// compliance member, so an admin who could be the subject of a matter cannot lift
// it; anyone who may act on holds may read them.
var (
	retentionPolicyRoles = []string{"owner", "admin"}
	legalHoldReadRoles   = []string{"owner", "admin", "compliance"}
	legalHoldWriteRoles  = []string{"owner", "compliance"}
)

// registerRetentionRoutes mounts the routes only when ORG_RETENTION_POLICY_ENABLED
// is "true": off, nothing is mounted and every path answers as it did before.
func (s *Server) registerRetentionRoutes(v1, internal *gin.RouterGroup, guard gin.HandlerFunc) {
	if !s.retentionOn {
		return
	}
	// Admin routes, on behalf of a signed user. The handlers check the role against
	// the membership table; the guard is the same second layer the other mutations
	// use.
	v1.GET("/organizations/:id/retention", s.getRetentionPolicy)
	v1.PUT("/organizations/:id/retention", guard, s.putRetentionPolicy)
	v1.GET("/organizations/:id/legal-holds", s.listLegalHolds)
	v1.POST("/organizations/:id/legal-holds", guard, s.placeLegalHold)
	v1.POST("/organizations/:id/legal-holds/:holdId/release", guard, s.releaseLegalHold)
	// The sweeper's question (EP-9 C2/C3): the window and which threads are held.
	// Own scope org:retention-policy:read:any, held by session-core alone; no-store.
	internal.POST("/orgs/:orgId/retention/check", s.checkRetentionInternal)
}

// writeRetentionError answers with the typed envelope. details is always an
// object; a list goes under a named key.
func writeRetentionError(c *gin.Context, status int, code, message string, details gin.H) {
	if details == nil {
		details = gin.H{}
	}
	c.AbortWithStatusJSON(status, gin.H{"error": gin.H{"code": code, "message": message, "details": details}})
}

func retentionRoleIn(role string, allowed []string) bool {
	normalized := modelentitlement.NormalizeRole(role)
	for _, candidate := range allowed {
		if normalized == candidate {
			return true
		}
	}
	return false
}

// authorizeRetention resolves the caller and their role from org-core's
// membership table, never from the forwarded header, and requires one of allowed.
func (s *Server) authorizeRetention(c *gin.Context, orgID string, allowed []string) (retention.Actor, bool) {
	callerID := strings.TrimSpace(c.GetHeader("x-user-id"))
	if callerID == "" {
		writeRetentionError(c, http.StatusUnauthorized, "unauthenticated", "user not authenticated", nil)
		return retention.Actor{}, false
	}
	if s.callerRoles == nil || s.retention == nil {
		writeRetentionError(c, http.StatusServiceUnavailable, "retention_unavailable", "retention policy unavailable", nil)
		return retention.Actor{}, false
	}
	role, err := s.callerRoles.CallerRole(c.Request.Context(), orgID, callerID)
	if err != nil {
		log.Printf("retention: caller role lookup failed for org=%s: %v", orgID, err)
		writeRetentionError(c, http.StatusInternalServerError, "internal_error", "failed to resolve caller role", nil)
		return retention.Actor{}, false
	}
	if strings.TrimSpace(role) == "" {
		writeRetentionError(c, http.StatusForbidden, "forbidden", "not a member of this organization", nil)
		return retention.Actor{}, false
	}
	if !retentionRoleIn(role, allowed) {
		writeRetentionError(c, http.StatusForbidden, "forbidden", "this needs one of these organization roles", gin.H{"roles": allowed})
		return retention.Actor{}, false
	}
	return retention.Actor{ID: callerID, Role: modelentitlement.NormalizeRole(role)}, true
}

func retentionOrgID(c *gin.Context, param string) (string, bool) {
	orgID := strings.TrimSpace(c.Param(param))
	if orgID == "" {
		writeRetentionError(c, http.StatusBadRequest, "validation_error", "organization id is required", nil)
		return "", false
	}
	return orgID, true
}

// writeRetentionFailure maps a service error to its typed answer.
func writeRetentionFailure(c *gin.Context, op, orgID string, err error) {
	var invalid *retention.ValidationError
	var conflict *retention.RevisionConflictError
	switch {
	case errors.As(err, &invalid):
		writeRetentionError(c, http.StatusBadRequest, "invalid_retention_request", "invalid retention request", gin.H{"errors": invalid.Errors})
	case errors.As(err, &conflict):
		writeRetentionError(c, http.StatusConflict, "revision_conflict", "the retention policy changed since it was read", gin.H{"current_revision": conflict.Current})
	case errors.Is(err, retention.ErrNothingRetained):
		writeRetentionError(c, http.StatusUnprocessableEntity, retention.CodeNothingRetained,
			"Nothing is kept for this thread under zero data retention, so there is nothing to hold.",
			gin.H{"feature": retention.ZDRFeature})
	case errors.Is(err, retention.ErrHoldNotFound):
		writeRetentionError(c, http.StatusNotFound, "legal_hold_not_found", "legal hold not found", nil)
	case errors.Is(err, retention.ErrHoldAlreadyReleased):
		writeRetentionError(c, http.StatusConflict, "legal_hold_already_released", "the legal hold was already released", nil)
	case errors.Is(err, retention.ErrOrganizationNotFound):
		writeRetentionError(c, http.StatusNotFound, "not_found", "organization not found", nil)
	default:
		log.Printf("retention: %s failed for org=%s: %v", op, orgID, err)
		writeRetentionError(c, http.StatusInternalServerError, "internal_error", "failed to "+op, nil)
	}
}

// getRetentionPolicy serves GET /api/v1/organizations/:id/retention. Owner or admin.
func (s *Server) getRetentionPolicy(c *gin.Context) {
	c.Header("Cache-Control", "no-store")
	orgID, ok := retentionOrgID(c, "id")
	if !ok {
		return
	}
	if _, ok := s.authorizeRetention(c, orgID, retentionPolicyRoles); !ok {
		return
	}
	ctx, cancel := context.WithTimeout(c.Request.Context(), retentionTimeout)
	defer cancel()
	policy, err := s.retention.Policy(ctx, orgID)
	if err != nil {
		writeRetentionFailure(c, "read retention policy", orgID, err)
		return
	}
	c.JSON(http.StatusOK, policy)
}

// putRetentionPolicy serves PUT /api/v1/organizations/:id/retention. The body
// carries the revision it was edited from: a stale editor gets 409 with the
// current revision, never a silent overwrite.
func (s *Server) putRetentionPolicy(c *gin.Context) {
	c.Header("Cache-Control", "no-store")
	orgID, ok := retentionOrgID(c, "id")
	if !ok {
		return
	}
	actor, ok := s.authorizeRetention(c, orgID, retentionPolicyRoles)
	if !ok {
		return
	}
	var req retention.PutRequest
	if !decodeRetentionBody(c, &req) {
		return
	}
	ctx, cancel := context.WithTimeout(c.Request.Context(), retentionTimeout)
	defer cancel()
	saved, err := s.retention.PutPolicy(ctx, orgID, actor, req)
	if err != nil {
		writeRetentionFailure(c, "write retention policy", orgID, err)
		return
	}
	log.Printf("retention: org=%s policy revision=%d written by actor=%s", orgID, saved.Revision, actor.ID)
	c.JSON(http.StatusOK, saved)
}

// listLegalHolds serves GET /api/v1/organizations/:id/legal-holds.
func (s *Server) listLegalHolds(c *gin.Context) {
	c.Header("Cache-Control", "no-store")
	orgID, ok := retentionOrgID(c, "id")
	if !ok {
		return
	}
	if _, ok := s.authorizeRetention(c, orgID, legalHoldReadRoles); !ok {
		return
	}
	ctx, cancel := context.WithTimeout(c.Request.Context(), retentionTimeout)
	defer cancel()
	holds, err := s.retention.Holds(ctx, orgID)
	if err != nil {
		writeRetentionFailure(c, "read legal holds", orgID, err)
		return
	}
	c.JSON(http.StatusOK, holds)
}

// placeLegalHold serves POST /api/v1/organizations/:id/legal-holds. A hold on a
// target already held answers 200 with the existing hold; a new one 201.
func (s *Server) placeLegalHold(c *gin.Context) {
	c.Header("Cache-Control", "no-store")
	orgID, ok := retentionOrgID(c, "id")
	if !ok {
		return
	}
	actor, ok := s.authorizeRetention(c, orgID, legalHoldWriteRoles)
	if !ok {
		return
	}
	var req retention.PlaceHoldRequest
	if !decodeRetentionBody(c, &req) {
		return
	}
	ctx, cancel := context.WithTimeout(c.Request.Context(), retentionTimeout)
	defer cancel()
	result, err := s.retention.PlaceHold(ctx, orgID, actor, req)
	if err != nil {
		writeRetentionFailure(c, "place legal hold", orgID, err)
		return
	}
	status := http.StatusCreated
	if result.Existed {
		status = http.StatusOK
	} else {
		log.Printf("retention: org=%s hold=%s scope=%s placed by actor=%s", orgID, result.Hold.ID, result.Hold.Scope, actor.ID)
	}
	c.JSON(status, result)
}

// releaseLegalHold serves POST /api/v1/organizations/:id/legal-holds/:holdId/release.
func (s *Server) releaseLegalHold(c *gin.Context) {
	c.Header("Cache-Control", "no-store")
	orgID, ok := retentionOrgID(c, "id")
	if !ok {
		return
	}
	actor, ok := s.authorizeRetention(c, orgID, legalHoldWriteRoles)
	if !ok {
		return
	}
	ctx, cancel := context.WithTimeout(c.Request.Context(), retentionTimeout)
	defer cancel()
	hold, err := s.retention.ReleaseHold(ctx, orgID, c.Param("holdId"), actor)
	if err != nil {
		writeRetentionFailure(c, "release legal hold", orgID, err)
		return
	}
	log.Printf("retention: org=%s hold=%s released by actor=%s", orgID, hold.ID, actor.ID)
	c.JSON(http.StatusOK, hold)
}

// checkRetentionInternal serves POST /internal/orgs/:orgId/retention/check, the
// sweeper's question. Access requires org:retention-policy:read:any, which only
// session-core holds. The answer is computed on every call and marked no-store. A
// missing or deleted organisation is a definitive 404; any other failure is a 5xx,
// which the sweeper treats as "unknown" and sweeps nothing for that org.
func (s *Server) checkRetentionInternal(c *gin.Context) {
	c.Header("Cache-Control", "no-store")
	orgID, ok := retentionOrgID(c, "orgId")
	if !ok {
		return
	}
	if s.retention == nil {
		log.Printf("retention: no service configured for org=%s", orgID)
		writeRetentionError(c, http.StatusServiceUnavailable, "retention_unavailable", "retention policy unavailable", nil)
		return
	}
	var req retention.CheckRequest
	if !decodeRetentionBody(c, &req) {
		return
	}
	ctx, cancel := context.WithTimeout(c.Request.Context(), retentionCheckTimeout)
	defer cancel()
	result, err := s.retention.Check(ctx, orgID, req)
	if err != nil {
		writeRetentionFailure(c, "check retention", orgID, err)
		return
	}
	c.JSON(http.StatusOK, result)
}

// decodeRetentionBody reads one strict JSON body under the size limit. It writes
// the refusal itself and reports false when the request must stop.
func decodeRetentionBody(c *gin.Context, into any) bool {
	decoder := json.NewDecoder(http.MaxBytesReader(c.Writer, c.Request.Body, retention.MaxBodyBytes))
	decoder.DisallowUnknownFields()
	err := decoder.Decode(into)
	if err == nil {
		err = ensureJSONEOF(decoder)
	}
	if err != nil {
		var tooLarge *http.MaxBytesError
		if errors.As(err, &tooLarge) {
			writeRetentionError(c, http.StatusRequestEntityTooLarge, "body_too_large", "request body too large", nil)
			return false
		}
		writeRetentionError(c, http.StatusBadRequest, "invalid_body", "invalid request body", nil)
		return false
	}
	return true
}
