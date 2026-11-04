package http

import (
	"context"
	"encoding/json"
	"errors"
	"log"
	"net/http"
	"strconv"
	"strings"
	"time"

	orgcore "github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/org"
	"github.com/gin-gonic/gin"
)

// webPolicyService is the seam the web policy routes depend on. *org.Service
// satisfies it; the handler tests use a fake or an in-memory implementation, so the
// real router and the real service-auth middleware run without a database.
type webPolicyService interface {
	GetWebPolicyPosture(ctx context.Context, orgID string) (*orgcore.WebPolicyView, error)
	SetWebPolicy(ctx context.Context, orgID, changedBy, actorRole string, doc orgcore.WebPolicyDoc, ifMatch int64) (*orgcore.WebPolicyPutResult, error)
	ListWebPolicyHistory(ctx context.Context, orgID string, limit int, cursor int64) ([]orgcore.WebPolicyHistoryEntry, int64, error)
}

// SetWebPolicyService wires the web policy service. A server without it answers
// the routes 503 instead of serving a policy it cannot read.
func (s *Server) SetWebPolicyService(service webPolicyService) {
	s.webPolicy = service
}

const (
	// webPolicyTimeout bounds one admin read or write.
	webPolicyTimeout = 5 * time.Second
	// webPolicyInternalTimeout bounds Auth Core's pull: its token mint waits on it.
	webPolicyInternalTimeout = 3 * time.Second
	// webPolicyMaxBodyBytes bounds a PUT body; the document itself is capped at
	// orgcore.WebPolicyMaxBytes after normalisation.
	webPolicyMaxBodyBytes = 16 << 10
	webPolicyDefaultLimit = 50
	webPolicyMaxLimit     = 200
)

// Error codes of the web policy routes, in the typed envelope
// {"error":{"code","message","details"}}.
const (
	webPolicyCodeUnauthenticated   = "unauthenticated"
	webPolicyCodeForbidden         = "forbidden"
	webPolicyCodeAdminRequired     = "admin_required"
	webPolicyCodeNotFound          = "not_found"
	webPolicyCodeUnavailable       = "unavailable"
	webPolicyCodeInternal          = "internal_error"
	webPolicyCodeInvalidBody       = "invalid_body"
	webPolicyCodeBodyTooLarge      = "too_large"
	webPolicyCodePreconditionReq   = "precondition_required"
	webPolicyCodeInvalidIfMatch    = "invalid_if_match"
	webPolicyCodeRevisionConflict  = "revision_conflict"
	webPolicyCodeIllegalMode       = "illegal_mode"
	webPolicyCodeUnknownCapability = "unknown_capability"
	webPolicyCodeInvalidDomain     = "invalid_domain"
	webPolicyCodeNotEntitled       = "not_entitled"
	webPolicyCodeInvalidLimit      = "invalid_limit"
	webPolicyCodeInvalidCursor     = "invalid_cursor"
)

func writeWebPolicyError(c *gin.Context, status int, code, message string, details map[string]any) {
	if details == nil {
		details = map[string]any{}
	}
	c.JSON(status, gin.H{"error": gin.H{"code": code, "message": message, "details": details}})
}

// authorizeWebPolicyCaller resolves the caller and requires an active member of the
// organization. The role is the one in org-core's membership table: the role header
// the gateway forwards is a hint and is never authority. admin reports whether that
// role is owner or admin; adminOnly makes any other role a 403. It writes the
// refusal itself and returns ok=false when the request must stop.
func (s *Server) authorizeWebPolicyCaller(c *gin.Context, orgID string, adminOnly bool) (callerID, role string, admin, ok bool) {
	callerID = strings.TrimSpace(c.GetHeader("x-user-id"))
	if callerID == "" {
		writeWebPolicyError(c, http.StatusUnauthorized, webPolicyCodeUnauthenticated, "user not authenticated", nil)
		return "", "", false, false
	}
	if s.callerRoles == nil {
		writeWebPolicyError(c, http.StatusServiceUnavailable, webPolicyCodeUnavailable, "web policy unavailable", nil)
		return "", "", false, false
	}
	role, err := s.callerRoles.CallerRole(c.Request.Context(), orgID, callerID)
	if err != nil {
		log.Printf("web policy: caller role lookup failed for org=%s: %v", orgID, err)
		writeWebPolicyError(c, http.StatusInternalServerError, webPolicyCodeInternal, "failed to resolve caller role", nil)
		return "", "", false, false
	}
	if strings.TrimSpace(role) == "" {
		writeWebPolicyError(c, http.StatusForbidden, webPolicyCodeForbidden, "not a member of this organization", nil)
		return "", "", false, false
	}
	admin = modelPolicyAdminRole(role)
	if adminOnly && !admin {
		writeWebPolicyError(c, http.StatusForbidden, webPolicyCodeAdminRequired, "organization owner or admin role required", nil)
		return "", "", false, false
	}
	return callerID, role, admin, true
}

// getWebPolicy serves GET /orgs/:id/web-policy and the /api/v1/organizations/:id
// twin: {data:{policy, effective, revision, rev, entitlements, updated_by,
// updated_at}}. An owner or admin sees the stored document; any other active member
// sees only what the product needs to decide what to offer (the effective document
// and the entitlements), with policy, updated_by and updated_at null.
func (s *Server) getWebPolicy(c *gin.Context) {
	c.Header("Cache-Control", "no-store")
	orgID := strings.TrimSpace(c.Param("id"))
	if orgID == "" {
		writeWebPolicyError(c, http.StatusBadRequest, webPolicyCodeInvalidBody, "organization id is required", nil)
		return
	}
	_, _, admin, ok := s.authorizeWebPolicyCaller(c, orgID, false)
	if !ok {
		return
	}
	if s.webPolicy == nil {
		writeWebPolicyError(c, http.StatusServiceUnavailable, webPolicyCodeUnavailable, "web policy unavailable", nil)
		return
	}
	ctx, cancel := context.WithTimeout(c.Request.Context(), webPolicyTimeout)
	defer cancel()
	view, err := s.webPolicy.GetWebPolicyPosture(ctx, orgID)
	if err != nil {
		if errors.Is(err, orgcore.ErrNotFound) {
			writeWebPolicyError(c, http.StatusNotFound, webPolicyCodeNotFound, "organization not found", nil)
			return
		}
		log.Printf("web policy: read failed for org=%s: %v", orgID, err)
		writeWebPolicyError(c, http.StatusInternalServerError, webPolicyCodeInternal, "failed to read web policy", nil)
		return
	}
	data := gin.H{
		"policy":       nil,
		"effective":    view.Effective,
		"revision":     view.Revision,
		"rev":          view.Rev,
		"entitlements": view.Entitlements,
		"updated_by":   nil,
		"updated_at":   nil,
	}
	if admin {
		data["policy"] = view.Policy
		if view.UpdatedBy != nil {
			data["updated_by"] = *view.UpdatedBy
		}
		if view.UpdatedAt != nil {
			data["updated_at"] = *view.UpdatedAt
		}
	}
	c.JSON(http.StatusOK, gin.H{"data": data})
}

// putWebPolicy serves PUT /orgs/:id/web-policy. The body is the policy document
// ({caps, robots_max, proxy_max, domains}); the If-Match header carries the
// revision it was edited from (0 for the first write). A stale revision is 409 with
// the current one, never a silent overwrite. Owner or admin only. 200
// {data:{revision, rev}}.
func (s *Server) putWebPolicy(c *gin.Context) {
	c.Header("Cache-Control", "no-store")
	orgID := strings.TrimSpace(c.Param("id"))
	if orgID == "" {
		writeWebPolicyError(c, http.StatusBadRequest, webPolicyCodeInvalidBody, "organization id is required", nil)
		return
	}
	actorID, role, _, ok := s.authorizeWebPolicyCaller(c, orgID, true)
	if !ok {
		return
	}
	if s.webPolicy == nil {
		writeWebPolicyError(c, http.StatusServiceUnavailable, webPolicyCodeUnavailable, "web policy unavailable", nil)
		return
	}
	ifMatch, ok := parseWebPolicyIfMatch(c)
	if !ok {
		return
	}
	var doc orgcore.WebPolicyDoc
	if !decodeWebPolicyBody(c, &doc) {
		return
	}
	ctx, cancel := context.WithTimeout(c.Request.Context(), webPolicyTimeout)
	defer cancel()
	result, err := s.webPolicy.SetWebPolicy(ctx, orgID, actorID, role, doc, ifMatch)
	if err != nil {
		s.writeWebPolicyPutError(c, orgID, err)
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": gin.H{"revision": result.Revision, "rev": result.Rev}})
}

func (s *Server) writeWebPolicyPutError(c *gin.Context, orgID string, err error) {
	var conflict *orgcore.RevisionConflictError
	var refusal *orgcore.WebPolicyError
	switch {
	case errors.As(err, &conflict):
		writeWebPolicyError(c, http.StatusConflict, webPolicyCodeRevisionConflict,
			"the web policy changed since it was read", map[string]any{"current": conflict.Current})
	case errors.As(err, &refusal):
		details := map[string]any{}
		if refusal.Key != "" {
			details["key"] = refusal.Key
		}
		switch {
		case errors.Is(err, orgcore.ErrNotEntitled):
			details["capability"] = refusal.Capability
			writeWebPolicyError(c, http.StatusPaymentRequired, webPolicyCodeNotEntitled, refusal.Message, details)
		case errors.Is(err, orgcore.ErrUnknownCapability):
			writeWebPolicyError(c, http.StatusUnprocessableEntity, webPolicyCodeUnknownCapability, refusal.Message, details)
		case errors.Is(err, orgcore.ErrInvalidDomain):
			writeWebPolicyError(c, http.StatusUnprocessableEntity, webPolicyCodeInvalidDomain, refusal.Message, details)
		case errors.Is(err, orgcore.ErrWebPolicyTooLarge):
			writeWebPolicyError(c, http.StatusUnprocessableEntity, webPolicyCodeBodyTooLarge, refusal.Message, details)
		case errors.Is(err, orgcore.ErrRevisionConflict):
			writeWebPolicyError(c, http.StatusBadRequest, webPolicyCodeInvalidIfMatch, refusal.Message, details)
		default:
			writeWebPolicyError(c, http.StatusUnprocessableEntity, webPolicyCodeIllegalMode, refusal.Message, details)
		}
	case errors.Is(err, orgcore.ErrNotFound):
		writeWebPolicyError(c, http.StatusNotFound, webPolicyCodeNotFound, "organization not found", nil)
	default:
		log.Printf("web policy: write failed for org=%s: %v", orgID, err)
		writeWebPolicyError(c, http.StatusInternalServerError, webPolicyCodeInternal, "failed to write web policy", nil)
	}
}

// listWebPolicyHistory serves GET /orgs/:id/web-policy/history?limit=50&cursor=:
// {data:[{revision, rev, policy, changed_by, changed_at, tightened}], meta:{cursor}},
// newest first. Owner or admin only.
func (s *Server) listWebPolicyHistory(c *gin.Context) {
	c.Header("Cache-Control", "no-store")
	orgID := strings.TrimSpace(c.Param("id"))
	if orgID == "" {
		writeWebPolicyError(c, http.StatusBadRequest, webPolicyCodeInvalidBody, "organization id is required", nil)
		return
	}
	if _, _, _, ok := s.authorizeWebPolicyCaller(c, orgID, true); !ok {
		return
	}
	if s.webPolicy == nil {
		writeWebPolicyError(c, http.StatusServiceUnavailable, webPolicyCodeUnavailable, "web policy unavailable", nil)
		return
	}
	limit := webPolicyDefaultLimit
	if raw, present := c.GetQuery("limit"); present {
		parsed, err := strconv.Atoi(strings.TrimSpace(raw))
		if err != nil || parsed < 1 || parsed > webPolicyMaxLimit {
			writeWebPolicyError(c, http.StatusBadRequest, webPolicyCodeInvalidLimit, "limit must be between 1 and 200", nil)
			return
		}
		limit = parsed
	}
	var cursor int64
	if raw, present := c.GetQuery("cursor"); present && strings.TrimSpace(raw) != "" {
		parsed, err := strconv.ParseInt(strings.TrimSpace(raw), 10, 64)
		if err != nil || parsed < 1 {
			writeWebPolicyError(c, http.StatusBadRequest, webPolicyCodeInvalidCursor, "cursor must be a revision number", nil)
			return
		}
		cursor = parsed
	}
	ctx, cancel := context.WithTimeout(c.Request.Context(), webPolicyTimeout)
	defer cancel()
	entries, next, err := s.webPolicy.ListWebPolicyHistory(ctx, orgID, limit, cursor)
	if err != nil {
		if errors.Is(err, orgcore.ErrNotFound) {
			writeWebPolicyError(c, http.StatusNotFound, webPolicyCodeNotFound, "organization not found", nil)
			return
		}
		log.Printf("web policy: history read failed for org=%s: %v", orgID, err)
		writeWebPolicyError(c, http.StatusInternalServerError, webPolicyCodeInternal, "failed to read web policy history", nil)
		return
	}
	if entries == nil {
		entries = []orgcore.WebPolicyHistoryEntry{}
	}
	var nextCursor any
	if next > 0 {
		nextCursor = strconv.FormatInt(next, 10)
	}
	c.JSON(http.StatusOK, gin.H{"data": entries, "meta": gin.H{"cursor": nextCursor}})
}

// getWebPolicyInternal serves GET /internal/orgs/:orgId/web-policy, Auth Core's
// pull for the web_policy token claim (WS7.I). Access requires the
// org:web-policy:read:any scope (serviceScopesForRequest), held by Auth Core alone
// (validateServiceCredential), so no other principal reads it and the holder reads
// nothing else about the org.
//
// {data:{revision, rev, policy, entitlements}} where policy is the EFFECTIVE
// document (the stored one folded with the plan's entitlements, tightening only)
// and rev is the stored policy's digest, "0" when the organization never wrote one.
// The answer is computed from the database on every call and marked no-store: it
// decides what an agent may do. An unknown or soft-deleted org is a definitive 404;
// any other failure is a 5xx with no detail, which the issuer treats as "policy
// unknown" and never as "no policy".
func (s *Server) getWebPolicyInternal(c *gin.Context) {
	c.Header("Cache-Control", "no-store")
	orgID := strings.TrimSpace(c.Param("orgId"))
	if orgID == "" {
		writeWebPolicyError(c, http.StatusBadRequest, webPolicyCodeInvalidBody, "organization id is required", nil)
		return
	}
	if s.webPolicy == nil {
		log.Printf("web policy: no service configured for org=%s", orgID)
		writeWebPolicyError(c, http.StatusServiceUnavailable, webPolicyCodeUnavailable, "web policy unavailable", nil)
		return
	}
	ctx, cancel := context.WithTimeout(c.Request.Context(), webPolicyInternalTimeout)
	defer cancel()
	view, err := s.webPolicy.GetWebPolicyPosture(ctx, orgID)
	if err != nil {
		if errors.Is(err, orgcore.ErrNotFound) {
			writeWebPolicyError(c, http.StatusNotFound, webPolicyCodeNotFound, "organization not found", nil)
			return
		}
		log.Printf("web policy: internal read failed for org=%s: %v", orgID, err)
		writeWebPolicyError(c, http.StatusInternalServerError, webPolicyCodeInternal, "failed to read web policy", nil)
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": gin.H{
		"revision":     view.Revision,
		"rev":          view.Rev,
		"policy":       view.Effective,
		"entitlements": view.Entitlements,
	}})
}

// parseWebPolicyIfMatch reads the If-Match header as a revision: 0 for the first
// write, then the revision last read. The quoted entity-tag form ("3" and W/"3") is
// accepted. It writes the refusal itself: 428 when absent, 400 when malformed.
func parseWebPolicyIfMatch(c *gin.Context) (int64, bool) {
	values := c.Request.Header.Values("If-Match")
	if len(values) == 0 {
		writeWebPolicyError(c, http.StatusPreconditionRequired, webPolicyCodePreconditionReq,
			"If-Match with the revision you read (0 for the first write) is required", nil)
		return 0, false
	}
	if len(values) != 1 {
		writeWebPolicyError(c, http.StatusBadRequest, webPolicyCodeInvalidIfMatch, "exactly one If-Match revision is required", nil)
		return 0, false
	}
	raw := strings.TrimSpace(values[0])
	raw = strings.TrimPrefix(raw, "W/")
	raw = strings.Trim(raw, `"`)
	revision, err := strconv.ParseInt(raw, 10, 64)
	if err != nil || revision < 0 || strings.TrimSpace(raw) != raw || raw == "" {
		writeWebPolicyError(c, http.StatusBadRequest, webPolicyCodeInvalidIfMatch, "If-Match must be a revision number", nil)
		return 0, false
	}
	return revision, true
}

// decodeWebPolicyBody reads one strict JSON document under the size limit. It
// writes the refusal itself and reports false when the request must stop.
func decodeWebPolicyBody(c *gin.Context, into any) bool {
	decoder := json.NewDecoder(http.MaxBytesReader(c.Writer, c.Request.Body, webPolicyMaxBodyBytes))
	decoder.DisallowUnknownFields()
	err := decoder.Decode(into)
	if err == nil {
		err = ensureJSONEOF(decoder)
	}
	if err != nil {
		var tooLarge *http.MaxBytesError
		if errors.As(err, &tooLarge) {
			writeWebPolicyError(c, http.StatusRequestEntityTooLarge, webPolicyCodeBodyTooLarge, "request body too large", nil)
			return false
		}
		writeWebPolicyError(c, http.StatusBadRequest, webPolicyCodeInvalidBody, "invalid request body", nil)
		return false
	}
	return true
}
