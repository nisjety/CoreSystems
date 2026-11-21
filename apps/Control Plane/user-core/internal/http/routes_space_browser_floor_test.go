package http

import (
	"net/http"
	"strings"
	"testing"

	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/spaces"
	"github.com/gin-gonic/gin"
)

// TestRequireSpaceBrowserFloorReaderRequiresOrgCoreAndTheScope pins the guard of the
// browser-floor read: only org-core's service identity, holding spaces:browser-floor:read,
// passes. A user delegation, another principal, a bearer caller and a missing or different
// scope are all refused.
func TestRequireSpaceBrowserFloorReaderRequiresOrgCoreAndTheScope(t *testing.T) {
	s := &Server{}
	for _, candidate := range []struct {
		authMethod string
		serviceID  string
		scopes     []string
		allowed    bool
	}{
		{authMethod: "service_principal", serviceID: orgCorePrincipal, scopes: []string{"spaces:browser-floor:read"}, allowed: true},
		{authMethod: "service_principal", serviceID: orgCorePrincipal, scopes: []string{"spaces:resolve"}},
		{authMethod: "service_principal", serviceID: orgCorePrincipal},
		{authMethod: "service_principal", serviceID: verevonGatewayPrincipal, scopes: []string{"spaces:browser-floor:read"}},
		{authMethod: "service_principal", serviceID: "org-core-impostor", scopes: []string{"spaces:browser-floor:read"}},
		{authMethod: "bearer", serviceID: orgCorePrincipal, scopes: []string{"spaces:browser-floor:read"}},
	} {
		c, w := newGinCtx("/api/v1/internal/spaces/sp-1/browser-floor?org_id=org-1")
		c.Set("auth_method", candidate.authMethod)
		c.Set("service_id", candidate.serviceID)
		if candidate.scopes != nil {
			c.Set("service_scopes", candidate.scopes)
		}
		s.requireSpaceBrowserFloorReader(c)
		if candidate.allowed && w.Code != http.StatusOK {
			t.Fatalf("the org-core browser floor reader was denied: %d", w.Code)
		}
		if !candidate.allowed && w.Code != http.StatusForbidden {
			t.Fatalf("an ineligible caller was admitted: method=%q service=%q scopes=%v", candidate.authMethod, candidate.serviceID, candidate.scopes)
		}
	}
}

// TestBrowserFloorScopeIsAServiceScopeOfTheRead pins the scope the read is admitted under:
// GET of the browser-floor path is spaces:browser-floor:read, which no user delegation is
// needed for. Other methods are not matched by this rule.
func TestBrowserFloorScopeIsAServiceScopeOfTheRead(t *testing.T) {
	path := "/api/v1/internal/spaces/sp-1/browser-floor"
	if scope, ok := serviceScopeForSpacePath(http.MethodGet, path); !ok || scope != "spaces:browser-floor:read" {
		t.Fatalf("GET browser-floor scope = %q, matched %v; want spaces:browser-floor:read", scope, ok)
	}
	if spaceScopeRequiresVerifiedDelegation("spaces:browser-floor:read") {
		t.Fatal("the browser floor is read by a service principal, not a user delegation")
	}
	if scope, ok := serviceScopeForSpacePath(http.MethodPost, path); ok {
		t.Fatalf("only GET is the browser-floor read, POST matched %q", scope)
	}
}

// TestBrowserFloorRouteRefusesMalformedIdentifiers pins the 400 answers that come before any
// database read: a malformed or overlong Space reference, and a missing or malformed
// organisation id.
func TestBrowserFloorRouteRefusesMalformedIdentifiers(t *testing.T) {
	s := &Server{spaceRepo: &spaces.Repository{}}
	for _, tc := range []struct {
		name, ref, org string
	}{
		{"blank organisation", "sp-1", ""},
		{"malformed organisation", "sp-1", "org 1"},
		{"malformed space", "sp 1", "org-1"},
		{"overlong space", strings.Repeat("s", maxBrowserFloorIDLength+1), "org-1"},
	} {
		// The request target stays a valid URL; the malformed reference reaches the handler
		// as the route parameter it would be in production after URL decoding.
		c, w := newGinCtx("/api/v1/internal/spaces/sp-x/browser-floor")
		c.Params = gin.Params{{Key: "space_ref", Value: tc.ref}}
		query := c.Request.URL.Query()
		query.Set("org_id", tc.org)
		c.Request.URL.RawQuery = query.Encode()
		s.getSpaceBrowserFloor(c)
		if w.Code != http.StatusBadRequest {
			t.Fatalf("%s: a malformed identifier must be refused with 400, got %d", tc.name, w.Code)
		}
	}
}

// TestBrowserFloorRouteAnswers503WithoutTheRepository pins that a server with no Space
// repository answers 503, which org-core reads as unknown and refuses on.
func TestBrowserFloorRouteAnswers503WithoutTheRepository(t *testing.T) {
	s := &Server{}
	c, w := newGinCtx("/api/v1/internal/spaces/sp-1/browser-floor?org_id=org-1")
	c.Params = gin.Params{{Key: "space_ref", Value: "sp-1"}}
	s.getSpaceBrowserFloor(c)
	if w.Code != http.StatusServiceUnavailable {
		t.Fatalf("a server without the Space repository must answer 503, got %d", w.Code)
	}
}
