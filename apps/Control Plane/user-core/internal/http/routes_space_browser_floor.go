package http

import (
	"log"
	"net/http"
	"strings"

	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/spaces"
	"github.com/gin-gonic/gin"
)

// F-f03-01: the browser-floor read that org-core's browser-site resolve makes for a Space
// (DEC-19 D9). Scope is in scopes_space_browser_floor.go; the guard is
// requireSpaceBrowserFloorReader in spaces.go.
//
//	GET /api/v1/internal/spaces/:space_ref/browser-floor?org_id=<org>   org-core only (spaces:browser-floor:read)
func init() {
	registerSpaceFeatureRoute(func(s *Server, spacesGroup *gin.RouterGroup) {
		spacesGroup.GET("/:space_ref/browser-floor", s.requireSpaceBrowserFloorReader, s.getSpaceBrowserFloor)
	})
}

// maxBrowserFloorIDLength bounds one space_ref or org_id on the browser-floor read.
const maxBrowserFloorIDLength = 128

// getSpaceBrowserFloor answers whether one Space is entitled to the agent browser. The
// answer is the stored bit only, and a Space that is not registered for the organisation
// is answered not entitled, never with a 404 that would show another organisation's Space.
// A database failure is a 503, which org-core reads as unknown and refuses on. The answer
// is no-store, and the log carries the organisation and the Space reference only.
func (s *Server) getSpaceBrowserFloor(c *gin.Context) {
	c.Header("Cache-Control", "no-store")
	if !s.spaceRepoAvailable(c) {
		return
	}
	spaceRef := strings.TrimSpace(c.Param("space_ref"))
	orgID := strings.TrimSpace(c.Query("org_id"))
	if !validBrowserFloorID(spaceRef) {
		abortTypedError(c, http.StatusBadRequest, spaces.CodeInvalidRequest, "space_ref is malformed",
			map[string]any{"field": "space_ref"})
		return
	}
	if !validBrowserFloorID(orgID) {
		abortTypedError(c, http.StatusBadRequest, spaces.CodeInvalidRequest, "org_id is required",
			map[string]any{"field": "org_id"})
		return
	}
	entitled, err := s.spaceRepo.BrowserFloor(c.Request.Context(), orgID, spaceRef)
	if err != nil {
		log.Printf("space browser floor: read failed for org=%s space=%s: %v", orgID, spaceRef, err)
		abortTypedError(c, http.StatusServiceUnavailable, "space_policy_unavailable", "the browser floor could not be read", nil)
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": gin.H{"space_ref": spaceRef, "browser_entitled": entitled}})
}

// validBrowserFloorID accepts a non-empty identifier of bounded length made of letters,
// digits and the separators used in Space and organisation ids. Anything else is malformed.
func validBrowserFloorID(value string) bool {
	if value == "" || len(value) > maxBrowserFloorIDLength {
		return false
	}
	for _, character := range value {
		if (character >= 'a' && character <= 'z') || (character >= 'A' && character <= 'Z') ||
			(character >= '0' && character <= '9') || strings.ContainsRune("._:-", character) {
			continue
		}
		return false
	}
	return true
}
