package http

import (
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"testing"

	"github.com/gin-gonic/gin"

	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/spaces"
)

// EP-8 S7: POST /api/v1/internal/spaces/memory-decision (pure tier). The
// CONTROL_SPACE_MEMORY_DECISIONS switch, the issuer scope and the checks made
// before any repository read. Issuance itself needs the database.

const spaceMemoryPath = "/api/v1/internal/spaces/memory-decision"

func spaceMemoryHarness(t *testing.T, enabled bool, repo *spaces.Repository, ctx map[string]any) *gin.Engine {
	t.Helper()
	gin.SetMode(gin.TestMode)
	server := NewServer(nil, nil, nil, nil, "0")
	server.SetSpaceRepository(repo)
	server.SetSpaceMemoryDecisions(enabled)
	engine := gin.New()
	group := engine.Group("/api/v1/internal/spaces")
	group.Use(func(c *gin.Context) {
		for key, value := range ctx {
			c.Set(key, value)
		}
		c.Next()
	})
	server.registerSpaceRoutes(group)
	return engine
}

func TestSpaceMemoryRouteIsRegisteredOnceUnderTheIssueScope(t *testing.T) {
	gin.SetMode(gin.TestMode)
	counts := routeCounts(NewServer(nil, nil, nil, nil, "0").router.Routes())
	if counts["POST "+spaceMemoryPath] != 1 {
		t.Fatalf("memory-decision registered %d times, want 1", counts["POST "+spaceMemoryPath])
	}
	scope := serviceScopeForRequest(httptest.NewRequest(http.MethodPost, spaceMemoryPath, nil))
	if scope != "spaces:issue" {
		t.Fatalf("scope = %q, want spaces:issue", scope)
	}
	if !serviceScopeRequiresVerifiedDelegation(scope) {
		t.Fatal("spaces:issue must require a verified gateway delegation")
	}
}

// The switch defaults off: a fresh server answers 404 with the typed code and
// reads nothing, even with no repository wired (the off path never reaches the
// 503 repository check).
func TestSpaceMemoryRouteIsDarkWithTheSwitchOff(t *testing.T) {
	for name, repo := range map[string]*spaces.Repository{"no repository": nil, "repository": {}} {
		t.Run(name, func(t *testing.T) {
			engine := spaceMemoryHarness(t, false, repo, gatewayContext())
			rec := postJSON(engine, spaceMemoryPath, map[string]any{"space_ref": "room-1", "operation": "list"})
			if rec.Code != http.StatusNotFound {
				t.Fatalf("status = %d, want 404 (%s)", rec.Code, rec.Body.String())
			}
			var body map[string]any
			_ = json.Unmarshal(rec.Body.Bytes(), &body)
			if body["code"] != "space_memory_decisions_disabled" {
				t.Fatalf("code = %v (%s)", body["code"], rec.Body.String())
			}
		})
	}
}

func TestSpaceMemoryRouteRequiresAVerifiedGatewayDelegation(t *testing.T) {
	for name, ctx := range map[string]map[string]any{
		"no auth":             {},
		"other service":       {"auth_method": "service_principal", "service_id": "session-core", "delegation_verified": true},
		"gateway, unverified": {"auth_method": "service_principal", "service_id": verevonGatewayPrincipal, "delegation_verified": false},
	} {
		t.Run(name, func(t *testing.T) {
			engine := spaceMemoryHarness(t, true, &spaces.Repository{}, ctx)
			rec := postJSON(engine, spaceMemoryPath, map[string]any{"space_ref": "room-1", "operation": "list"})
			if rec.Code != http.StatusForbidden {
				t.Fatalf("status = %d, want 403 (%s)", rec.Code, rec.Body.String())
			}
		})
	}
}

func TestSpaceMemoryRouteFailsClosedWithoutARepository(t *testing.T) {
	engine := spaceMemoryHarness(t, true, nil, gatewayContext())
	rec := postJSON(engine, spaceMemoryPath, map[string]any{"space_ref": "room-1", "operation": "list"})
	if rec.Code != http.StatusServiceUnavailable {
		t.Fatalf("status = %d, want 503 (%s)", rec.Code, rec.Body.String())
	}
}

// Shape is refused with 400 before any membership read (the empty repository
// would fail a read, so a 400 proves none happened).
func TestSpaceMemoryRouteRejectsBadShapesBeforeReadingAuthority(t *testing.T) {
	engine := spaceMemoryHarness(t, true, &spaces.Repository{}, gatewayContext())
	for name, body := range map[string]map[string]any{
		"no space_ref":          {"operation": "list"},
		"no operation":          {"space_ref": "room-1"},
		"unknown operation":     {"space_ref": "room-1", "operation": "purge"},
		"list with a memory id": {"space_ref": "room-1", "operation": "list", "memory_id": "mem-1"},
		"delete without an id":  {"space_ref": "room-1", "operation": "delete"},
		"padded memory id":      {"space_ref": "room-1", "operation": "delete", "memory_id": "mem-1 "},
	} {
		t.Run(name, func(t *testing.T) {
			rec := postJSON(engine, spaceMemoryPath, body)
			if rec.Code != http.StatusBadRequest {
				t.Fatalf("status = %d, want 400 (%s)", rec.Code, rec.Body.String())
			}
		})
	}
}
