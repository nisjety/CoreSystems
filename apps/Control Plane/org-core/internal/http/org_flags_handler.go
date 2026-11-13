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

// orgFlagsEnvEnabled switches the organisation flag routes on (W5-079-b). Only the
// literal "true" turns it on. Off, every route answers 404 and no value is read or
// written.
const orgFlagsEnvEnabled = "ORG_FLAGS_ENABLED"

// orgFlagsEnabled reads orgFlagsEnvEnabled strictly, as retention.Enabled does: a typo
// or "1" leaves the routes unmounted.
func orgFlagsEnabled(getenv func(string) string) bool {
	return getenv(orgFlagsEnvEnabled) == "true"
}

// orgFlagService is the seam the organisation flag routes depend on. *org.Service
// satisfies it. Handler tests use a fake, so the real router and the real service-auth
// middleware run without a database.
type orgFlagService interface {
	ListOrgFlags(ctx context.Context, orgID string) ([]orgcore.OrgFlagView, error)
	SetOrgFlag(ctx context.Context, orgID, name, value, changedBy, actorRole string) (*orgcore.OrgFlagPutResult, error)
}

// SetOrgFlags wires the organisation flag service. A server without it answers the
// routes 503 instead of serving values it cannot read.
func (s *Server) SetOrgFlags(service orgFlagService) {
	s.orgFlags = service
}

// requireOrgFlagsEnabled answers 404 on every organisation flag route until the switch
// is on, so a deployment that never enables it exposes nothing and reveals nothing.
func (s *Server) requireOrgFlagsEnabled() gin.HandlerFunc {
	return func(c *gin.Context) {
		if !s.orgFlagsOn {
			c.AbortWithStatusJSON(http.StatusNotFound, gin.H{"error": gin.H{"code": "not_found", "message": "not found"}})
			return
		}
		c.Next()
	}
}

const (
	// orgFlagTimeout bounds one read or write.
	orgFlagTimeout = 5 * time.Second
	// orgFlagMaxBodyBytes bounds a PUT body. A value is a switch or a short choice.
	orgFlagMaxBodyBytes = 1 << 10
)

// Error codes of the organisation flag routes, in the typed envelope
// {"error":{"code","message","details"}}.
const (
	orgFlagCodeUnauthenticated = "unauthenticated"
	orgFlagCodeForbidden       = "forbidden"
	orgFlagCodeAdminRequired   = "admin_required"
	orgFlagCodeNotFound        = "not_found"
	orgFlagCodeUnavailable     = "unavailable"
	orgFlagCodeInternal        = "internal_error"
	orgFlagCodeInvalidBody     = "invalid_body"
	orgFlagCodeBodyTooLarge    = "too_large"
	orgFlagCodeUnknownFlag     = "unknown_flag"
	orgFlagCodeInvalidValue    = "invalid_value"
)

func writeOrgFlagError(c *gin.Context, status int, code, message string, details map[string]any) {
	if details == nil {
		details = map[string]any{}
	}
	c.JSON(status, gin.H{"error": gin.H{"code": code, "message": message, "details": details}})
}

// authorizeOrgFlagCaller resolves the caller and requires an active member of the
// organisation. The role is the one in org-core's membership table. The role header the
// gateway forwards is a hint and is never authority. admin reports whether that role is
// owner or admin, and adminOnly makes any other role a 403. It writes the refusal itself
// and returns ok=false when the request must stop.
func (s *Server) authorizeOrgFlagCaller(c *gin.Context, orgID string, adminOnly bool) (callerID, role string, admin, ok bool) {
	callerID = strings.TrimSpace(c.GetHeader("x-user-id"))
	if callerID == "" {
		writeOrgFlagError(c, http.StatusUnauthorized, orgFlagCodeUnauthenticated, "user not authenticated", nil)
		return "", "", false, false
	}
	if s.callerRoles == nil {
		writeOrgFlagError(c, http.StatusServiceUnavailable, orgFlagCodeUnavailable, "org flags unavailable", nil)
		return "", "", false, false
	}
	role, err := s.callerRoles.CallerRole(c.Request.Context(), orgID, callerID)
	if err != nil {
		log.Printf("org flags: caller role lookup failed for org=%s: %v", orgID, err)
		writeOrgFlagError(c, http.StatusInternalServerError, orgFlagCodeInternal, "failed to resolve caller role", nil)
		return "", "", false, false
	}
	if strings.TrimSpace(role) == "" {
		writeOrgFlagError(c, http.StatusForbidden, orgFlagCodeForbidden, "not a member of this organization", nil)
		return "", "", false, false
	}
	admin = modelPolicyAdminRole(role)
	if adminOnly && !admin {
		writeOrgFlagError(c, http.StatusForbidden, orgFlagCodeAdminRequired, "organization owner or admin role required", nil)
		return "", "", false, false
	}
	return callerID, role, admin, true
}

// orgFlagsJSON renders the flag views. updated_by and updated_at are included only for an
// owner or admin, as the web policy keeps its writer from other members.
func orgFlagsJSON(views []orgcore.OrgFlagView, admin bool) []gin.H {
	out := make([]gin.H, 0, len(views))
	for _, view := range views {
		values := view.Values
		if values == nil {
			values = []string{}
		}
		entry := gin.H{
			"name":       view.Name,
			"kind":       string(view.Kind),
			"values":     values,
			"default":    view.Default,
			"value":      view.Value,
			"stored":     view.Stored,
			"revision":   view.Revision,
			"updated_by": nil,
			"updated_at": nil,
		}
		if admin && view.Stored {
			entry["updated_by"] = view.UpdatedBy
			entry["updated_at"] = view.UpdatedAt
		}
		out = append(out, entry)
	}
	return out
}

// writeOrgFlagReadError maps a read failure to the typed envelope.
func writeOrgFlagReadError(c *gin.Context, orgID string, err error) {
	if errors.Is(err, orgcore.ErrNotFound) {
		writeOrgFlagError(c, http.StatusNotFound, orgFlagCodeNotFound, "organization not found", nil)
		return
	}
	log.Printf("org flags: read failed for org=%s: %v", orgID, err)
	writeOrgFlagError(c, http.StatusInternalServerError, orgFlagCodeInternal, "failed to read org flags", nil)
}

// getOrgFlags serves GET /orgs/:id/flags: {data:{flags:[...]}}, one entry per
// org-overridable flag. Any active member reads them.
func (s *Server) getOrgFlags(c *gin.Context) {
	c.Header("Cache-Control", "no-store")
	orgID := strings.TrimSpace(c.Param("id"))
	if orgID == "" {
		writeOrgFlagError(c, http.StatusBadRequest, orgFlagCodeInvalidBody, "organization id is required", nil)
		return
	}
	_, _, admin, ok := s.authorizeOrgFlagCaller(c, orgID, false)
	if !ok {
		return
	}
	if s.orgFlags == nil {
		writeOrgFlagError(c, http.StatusServiceUnavailable, orgFlagCodeUnavailable, "org flags unavailable", nil)
		return
	}
	ctx, cancel := context.WithTimeout(c.Request.Context(), orgFlagTimeout)
	defer cancel()
	views, err := s.orgFlags.ListOrgFlags(ctx, orgID)
	if err != nil {
		writeOrgFlagReadError(c, orgID, err)
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": gin.H{"flags": orgFlagsJSON(views, admin)}})
}

// putOrgFlag serves PUT /orgs/:id/flags/:name with the body {"value": ...}. A switch takes
// true or false, as a JSON boolean or as the strings "true" and "false". A choice takes
// one of its closed values as a string. Owner or admin only. 200 {data:{name, value,
// revision}}.
func (s *Server) putOrgFlag(c *gin.Context) {
	c.Header("Cache-Control", "no-store")
	orgID := strings.TrimSpace(c.Param("id"))
	if orgID == "" {
		writeOrgFlagError(c, http.StatusBadRequest, orgFlagCodeInvalidBody, "organization id is required", nil)
		return
	}
	actorID, role, _, ok := s.authorizeOrgFlagCaller(c, orgID, true)
	if !ok {
		return
	}
	if s.orgFlags == nil {
		writeOrgFlagError(c, http.StatusServiceUnavailable, orgFlagCodeUnavailable, "org flags unavailable", nil)
		return
	}
	value, ok := decodeOrgFlagBody(c)
	if !ok {
		return
	}
	ctx, cancel := context.WithTimeout(c.Request.Context(), orgFlagTimeout)
	defer cancel()
	result, err := s.orgFlags.SetOrgFlag(ctx, orgID, strings.TrimSpace(c.Param("name")), value, actorID, role)
	if err != nil {
		s.writeOrgFlagPutError(c, orgID, err)
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": gin.H{
		"name":     result.Name,
		"value":    result.Value,
		"revision": result.Revision,
	}})
}

// decodeOrgFlagBody reads one strict {"value": ...} document under the size limit and
// returns the value as text. A JSON boolean becomes "true" or "false", and a JSON string
// is kept as written for the service to check. It writes the refusal itself and reports
// false when the request must stop.
func decodeOrgFlagBody(c *gin.Context) (string, bool) {
	var body struct {
		Value any `json:"value"`
	}
	decoder := json.NewDecoder(http.MaxBytesReader(c.Writer, c.Request.Body, orgFlagMaxBodyBytes))
	decoder.DisallowUnknownFields()
	err := decoder.Decode(&body)
	if err == nil {
		err = ensureJSONEOF(decoder)
	}
	if err != nil {
		var tooLarge *http.MaxBytesError
		if errors.As(err, &tooLarge) {
			writeOrgFlagError(c, http.StatusRequestEntityTooLarge, orgFlagCodeBodyTooLarge, "request body too large", nil)
			return "", false
		}
		writeOrgFlagError(c, http.StatusBadRequest, orgFlagCodeInvalidBody, "invalid request body", nil)
		return "", false
	}
	switch value := body.Value.(type) {
	case bool:
		if value {
			return "true", true
		}
		return "false", true
	case string:
		return value, true
	}
	writeOrgFlagError(c, http.StatusUnprocessableEntity, orgFlagCodeInvalidValue,
		"value must be true, false or one of the flag's values", nil)
	return "", false
}

func (s *Server) writeOrgFlagPutError(c *gin.Context, orgID string, err error) {
	var refusal *orgcore.OrgFlagError
	switch {
	case errors.As(err, &refusal):
		details := map[string]any{"name": refusal.Name}
		if errors.Is(err, orgcore.ErrUnknownOrgFlag) {
			writeOrgFlagError(c, http.StatusUnprocessableEntity, orgFlagCodeUnknownFlag, refusal.Message, details)
			return
		}
		writeOrgFlagError(c, http.StatusUnprocessableEntity, orgFlagCodeInvalidValue, refusal.Message, details)
	case errors.Is(err, orgcore.ErrNotFound):
		writeOrgFlagError(c, http.StatusNotFound, orgFlagCodeNotFound, "organization not found", nil)
	default:
		log.Printf("org flags: write failed for org=%s: %v", orgID, err)
		writeOrgFlagError(c, http.StatusInternalServerError, orgFlagCodeInternal, "failed to write org flag", nil)
	}
}

// getOrgFlagsInternal serves GET /internal/orgs/:orgId/flags for a backend reader. The
// org:flags:read:any scope (serviceScopesForRequest) gates it, and there is no acting
// user, so updated_by and updated_at are omitted. The answer is no-store.
func (s *Server) getOrgFlagsInternal(c *gin.Context) {
	c.Header("Cache-Control", "no-store")
	orgID := strings.TrimSpace(c.Param("orgId"))
	if orgID == "" {
		writeOrgFlagError(c, http.StatusBadRequest, orgFlagCodeInvalidBody, "organization id is required", nil)
		return
	}
	if s.orgFlags == nil {
		writeOrgFlagError(c, http.StatusServiceUnavailable, orgFlagCodeUnavailable, "org flags unavailable", nil)
		return
	}
	ctx, cancel := context.WithTimeout(c.Request.Context(), orgFlagTimeout)
	defer cancel()
	views, err := s.orgFlags.ListOrgFlags(ctx, orgID)
	if err != nil {
		writeOrgFlagReadError(c, orgID, err)
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": gin.H{"flags": orgFlagsJSON(views, false)}})
}
