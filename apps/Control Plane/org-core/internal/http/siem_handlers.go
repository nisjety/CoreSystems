package http

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"log"
	"net/http"
	"strings"
	"time"

	orgcore "github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/org"
	"github.com/gin-gonic/gin"
)

// siemDestinationsFlagEnv switches the SIEM destination routes on. Anything but
// "1" or "true" leaves them unmounted (404), the default.
const siemDestinationsFlagEnv = "ORG_SIEM_DESTINATIONS_ENABLED"

func siemDestinationsEnabled(getenv func(string) string) bool {
	switch getenv(siemDestinationsFlagEnv) {
	case "1", "true":
		return true
	}
	return false
}

// ValidateSIEMDestinationsConfig is the boot check for the SIEM switch. With the
// switch off it reads nothing else and returns nil. With it on, an unusable seal
// key is a startup error: NewServer would otherwise mount the routes and answer
// 503 for as long as the process runs. The error names the variables, never
// their values.
func ValidateSIEMDestinationsConfig(getenv func(string) string) error {
	if !siemDestinationsEnabled(getenv) {
		return nil
	}
	if _, err := orgcore.NewSIEMSealerFromEnv(getenv); err != nil {
		return fmt.Errorf("%s is on but the SIEM seal is not usable: %w", siemDestinationsFlagEnv, err)
	}
	return nil
}

// siemDestinationService is the seam the SIEM routes depend on. *org.Service
// satisfies it; the handler tests use a fake so the real router and the real
// service-auth middleware run without a database.
type siemDestinationService interface {
	ListSIEMDestinations(ctx context.Context, orgID string) ([]orgcore.SIEMDestination, error)
	CreateSIEMDestination(ctx context.Context, orgID, actorID, actorRole string, req orgcore.CreateSIEMDestinationRequest) (*orgcore.SIEMDestination, error)
	UpdateSIEMDestination(ctx context.Context, orgID, destinationID, actorID, actorRole string, req orgcore.UpdateSIEMDestinationRequest) (*orgcore.SIEMDestination, error)
	DeleteSIEMDestination(ctx context.Context, orgID, destinationID, actorID, actorRole string) error
	ListSIEMDestinationsForExport(ctx context.Context, updatedSince *time.Time) ([]orgcore.SIEMExporterDestination, error)
}

// SetSIEMDestinations wires the SIEM service. Like SetModelEntitlements it is a
// setter so NewServer keeps its signature; a server without it answers the
// routes 503 instead of serving a store it cannot read.
func (s *Server) SetSIEMDestinations(service siemDestinationService) {
	s.siem = service
}

// requireSIEMEnabled answers 404 on every SIEM route until the feature is on, so
// a deployment that never enables it exposes nothing and reveals nothing.
func (s *Server) requireSIEMEnabled() gin.HandlerFunc {
	return func(c *gin.Context) {
		if !s.siemEnabled {
			c.AbortWithStatusJSON(http.StatusNotFound, gin.H{"error": "not found"})
			return
		}
		c.Next()
	}
}

const (
	siemRequestTimeout = 5 * time.Second
	// siemMaxBodyBytes bounds a create or patch body: a 2 KiB URL, a 512 byte
	// secret and a filter of at most 128 short names fit well inside it.
	siemMaxBodyBytes = 32 << 10
)

// authorizeSIEMAdmin resolves the caller and requires the owner or admin role
// from org-core's own membership table; the role header the gateway forwards is
// a hint, never authority. It writes the 401/403/500/503 itself and returns
// ok=false when the request must stop.
func (s *Server) authorizeSIEMAdmin(c *gin.Context, orgID string) (callerID, role string, ok bool) {
	callerID = strings.TrimSpace(c.GetHeader("x-user-id"))
	if callerID == "" {
		c.JSON(http.StatusUnauthorized, gin.H{"error": "user not authenticated"})
		return "", "", false
	}
	if s.callerRoles == nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "siem destinations unavailable"})
		return "", "", false
	}
	role, err := s.callerRoles.CallerRole(c.Request.Context(), orgID, callerID)
	if err != nil {
		log.Printf("siem destinations: caller role lookup failed for org=%s: %v", orgID, err)
		c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to resolve caller role"})
		return "", "", false
	}
	switch strings.ToLower(strings.TrimSpace(role)) {
	case "owner", "admin":
		return callerID, strings.ToLower(strings.TrimSpace(role)), true
	case "":
		c.JSON(http.StatusForbidden, gin.H{"error": "forbidden: not a member of this organization"})
	default:
		c.JSON(http.StatusForbidden, gin.H{"error": "organization owner or admin role required"})
	}
	return "", "", false
}

// siemAdminRequest is the common prologue of the four admin routes: the org id,
// the admin check and the wired-service check. ok=false means a response was
// already written.
func (s *Server) siemAdminRequest(c *gin.Context) (orgID, actorID, actorRole string, ok bool) {
	c.Header("Cache-Control", "no-store")
	orgID = strings.TrimSpace(c.Param("id"))
	if orgID == "" {
		c.JSON(http.StatusBadRequest, gin.H{"error": "organization id is required"})
		return "", "", "", false
	}
	actorID, actorRole, ok = s.authorizeSIEMAdmin(c, orgID)
	if !ok {
		return "", "", "", false
	}
	if s.siem == nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "siem destinations unavailable"})
		return "", "", "", false
	}
	return orgID, actorID, actorRole, true
}

func decodeSIEMBody(c *gin.Context, into any) bool {
	decoder := json.NewDecoder(http.MaxBytesReader(c.Writer, c.Request.Body, siemMaxBodyBytes))
	decoder.DisallowUnknownFields()
	err := decoder.Decode(into)
	if err == nil {
		err = ensureJSONEOF(decoder)
	}
	if err == nil {
		return true
	}
	var tooLarge *http.MaxBytesError
	if errors.As(err, &tooLarge) {
		c.JSON(http.StatusRequestEntityTooLarge, gin.H{"error": "request body too large", "code": "body_too_large"})
		return false
	}
	c.JSON(http.StatusBadRequest, gin.H{"error": "invalid request body", "code": "invalid_body"})
	return false
}

// writeSIEMError maps a service error to a typed response. Nothing from the
// request body (URL, secret) is echoed back or logged.
func writeSIEMError(c *gin.Context, orgID, operation string, err error) {
	var invalid *orgcore.SIEMValidationError
	var conflict *orgcore.SIEMRevisionConflictError
	switch {
	case errors.As(err, &invalid):
		c.JSON(http.StatusBadRequest, gin.H{"error": "invalid siem destination", "code": "invalid_siem_destination", "details": invalid.Errors})
	case errors.As(err, &conflict):
		c.JSON(http.StatusConflict, gin.H{"error": "the destination changed since it was read", "code": "revision_conflict", "current_revision": conflict.Current})
	case errors.Is(err, orgcore.ErrSIEMDestinationNotFound):
		c.JSON(http.StatusNotFound, gin.H{"error": "siem destination not found", "code": "siem_destination_not_found"})
	case errors.Is(err, orgcore.ErrSIEMDestinationLimit):
		c.JSON(http.StatusConflict, gin.H{"error": "an organization can have at most 3 siem destinations", "code": "siem_destination_limit"})
	case errors.Is(err, orgcore.ErrPlanUpgradeRequired):
		c.JSON(http.StatusPaymentRequired, gin.H{"error": "plan_upgrade_required", "message": "SIEM export is available on a higher plan."})
	case errors.Is(err, orgcore.ErrNotFound):
		c.JSON(http.StatusNotFound, gin.H{"error": "organization not found"})
	default:
		log.Printf("siem destinations: %s failed for org=%s: %v", operation, orgID, err)
		c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to " + operation + " siem destination"})
	}
}

// listSIEMDestinations serves GET /orgs/:id/siem-destinations.
func (s *Server) listSIEMDestinations(c *gin.Context) {
	orgID, _, _, ok := s.siemAdminRequest(c)
	if !ok {
		return
	}
	ctx, cancel := context.WithTimeout(c.Request.Context(), siemRequestTimeout)
	defer cancel()
	destinations, err := s.siem.ListSIEMDestinations(ctx, orgID)
	if err != nil {
		writeSIEMError(c, orgID, "list", err)
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": destinations, "meta": gin.H{"count": len(destinations), "max": orgcore.MaxSIEMDestinationsPerOrg}})
}

// createSIEMDestination serves POST /orgs/:id/siem-destinations.
func (s *Server) createSIEMDestination(c *gin.Context) {
	orgID, actorID, actorRole, ok := s.siemAdminRequest(c)
	if !ok {
		return
	}
	var req orgcore.CreateSIEMDestinationRequest
	if !decodeSIEMBody(c, &req) {
		return
	}
	ctx, cancel := context.WithTimeout(c.Request.Context(), siemRequestTimeout)
	defer cancel()
	created, err := s.siem.CreateSIEMDestination(ctx, orgID, actorID, actorRole, req)
	if err != nil {
		writeSIEMError(c, orgID, "create", err)
		return
	}
	log.Printf("siem destinations: org=%s destination=%s created by actor=%s", orgID, created.ID, actorID)
	c.JSON(http.StatusCreated, gin.H{"data": created})
}

// updateSIEMDestination serves PATCH /orgs/:id/siem-destinations/:destinationId.
func (s *Server) updateSIEMDestination(c *gin.Context) {
	orgID, actorID, actorRole, ok := s.siemAdminRequest(c)
	if !ok {
		return
	}
	var req orgcore.UpdateSIEMDestinationRequest
	if !decodeSIEMBody(c, &req) {
		return
	}
	ctx, cancel := context.WithTimeout(c.Request.Context(), siemRequestTimeout)
	defer cancel()
	updated, err := s.siem.UpdateSIEMDestination(ctx, orgID, strings.TrimSpace(c.Param("destinationId")), actorID, actorRole, req)
	if err != nil {
		writeSIEMError(c, orgID, "update", err)
		return
	}
	log.Printf("siem destinations: org=%s destination=%s revision=%d updated by actor=%s", orgID, updated.ID, updated.Revision, actorID)
	c.JSON(http.StatusOK, gin.H{"data": updated})
}

// deleteSIEMDestination serves DELETE /orgs/:id/siem-destinations/:destinationId.
func (s *Server) deleteSIEMDestination(c *gin.Context) {
	orgID, actorID, actorRole, ok := s.siemAdminRequest(c)
	if !ok {
		return
	}
	destinationID := strings.TrimSpace(c.Param("destinationId"))
	ctx, cancel := context.WithTimeout(c.Request.Context(), siemRequestTimeout)
	defer cancel()
	if err := s.siem.DeleteSIEMDestination(ctx, orgID, destinationID, actorID, actorRole); err != nil {
		writeSIEMError(c, orgID, "delete", err)
		return
	}
	log.Printf("siem destinations: org=%s destination=%s deleted by actor=%s", orgID, destinationID, actorID)
	c.Status(http.StatusNoContent)
}

// listSIEMDestinationsInternal serves GET /internal/orgs/siem-destinations
// [?updated_since=RFC3339], the exporter's read of every enabled destination.
//
// Access requires org:siem:read:any (serviceScopesForRequest), which only the
// audit-core principal may hold (validateServiceCredential). This is the one
// response that carries the unsealed secret, so it is no-store and nothing about
// it is logged. A blank updated_since is refused rather than read as absent.
func (s *Server) listSIEMDestinationsInternal(c *gin.Context) {
	c.Header("Cache-Control", "no-store")
	var updatedSince *time.Time
	if raw, present := c.GetQuery("updated_since"); present {
		parsed, err := time.Parse(time.RFC3339Nano, strings.TrimSpace(raw))
		if err != nil {
			c.JSON(http.StatusBadRequest, gin.H{"error": "updated_since must be an RFC 3339 timestamp"})
			return
		}
		parsed = parsed.UTC()
		updatedSince = &parsed
	}
	if s.siem == nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "siem destinations unavailable"})
		return
	}
	ctx, cancel := context.WithTimeout(c.Request.Context(), siemRequestTimeout)
	defer cancel()
	destinations, err := s.siem.ListSIEMDestinationsForExport(ctx, updatedSince)
	if err != nil {
		log.Printf("siem destinations: export read failed: %v", err)
		c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to read siem destinations"})
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": destinations, "meta": gin.H{"count": len(destinations)}})
}
