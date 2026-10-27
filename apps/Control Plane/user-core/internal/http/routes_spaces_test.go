package http

import (
	"net/http"
	"sort"
	"testing"

	"github.com/gin-gonic/gin"
)

// preSeamSpaceRoutes is the exact method+path list the spacesGroup block of
// server.go registered before the P-UC1 seam moved it into registerSpaceRoutes
// (taken from d56b3c41d^:server.go). The seam is a pure move: every one of
// these must still be on the engine exactly once.
var preSeamSpaceRoutes = []string{
	"POST /api/v1/internal/spaces/register",
	"POST /api/v1/internal/spaces/deletion-authorizations",
	"PUT /api/v1/internal/spaces/deletion-policy",
	"PUT /api/v1/internal/spaces/:space_ref/legal-hold",
	"DELETE /api/v1/internal/spaces/:space_ref/legal-hold",
	"GET /api/v1/internal/spaces",
	"GET /api/v1/internal/spaces/:space_ref/roster",
	"PUT /api/v1/internal/spaces/:space_ref/memberships",
	"POST /api/v1/internal/spaces/recipient-audiences",
	"PUT /api/v1/internal/spaces/effect-policy",
	"GET /api/v1/internal/spaces/:space_ref/membership",
	"POST /api/v1/internal/spaces/personal-thread-decision",
	"POST /api/v1/internal/spaces/thread-decision",
	"POST /api/v1/internal/spaces/thread-append-decision",
	"POST /api/v1/internal/spaces/thread-read-decision",
	"POST /api/v1/internal/spaces/personal-retrieval-decision",
	"POST /api/v1/internal/spaces/retrieval-decision",
	"POST /api/v1/internal/spaces/personal-import-decision",
	"POST /api/v1/internal/spaces/schedule-create-decision",
	"POST /api/v1/internal/spaces/watch-create-decision",
	"POST /api/v1/internal/spaces/watch-observe-decision",
	"POST /api/v1/internal/spaces/import-execution-decision",
	"POST /api/v1/internal/spaces/schedule-fire-decision",
	"POST /api/v1/internal/spaces/sandbox-capability-decision",
	"POST /api/v1/internal/spaces/scheduled-run-decision",
	"POST /api/v1/internal/spaces/scheduled-run-execution-decision",
	"POST /api/v1/internal/spaces/scheduled-step-decision",
	"POST /api/v1/internal/spaces/owner-grant-decision",
	"POST /api/v1/internal/spaces/model-action-view",
	"POST /api/v1/internal/spaces/run-action-decision",
	"POST /api/v1/internal/spaces/run-action-authority-check",
	"POST /api/v1/internal/spaces/owner-effect-reservations/reserve",
	"POST /api/v1/internal/spaces/owner-effect-reservations/:reservation_id/commit",
	"GET /api/v1/internal/spaces/owner-effect-reservations/:reservation_id",
}

func routeCounts(routes gin.RoutesInfo) map[string]int {
	counts := make(map[string]int, len(routes))
	for _, route := range routes {
		counts[route.Method+" "+route.Path]++
	}
	return counts
}

func TestSpaceRouteTableMatchesPreSeamGolden(t *testing.T) {
	gin.SetMode(gin.TestMode)
	server := NewServer(nil, nil, nil, nil, "0")
	counts := routeCounts(server.router.Routes())

	var missing, duplicated []string
	for _, route := range preSeamSpaceRoutes {
		switch counts[route] {
		case 1:
		case 0:
			missing = append(missing, route)
		default:
			duplicated = append(duplicated, route)
		}
	}
	sort.Strings(missing)
	sort.Strings(duplicated)
	if len(missing) > 0 {
		t.Fatalf("pre-seam space routes missing from the engine: %v", missing)
	}
	if len(duplicated) > 0 {
		t.Fatalf("pre-seam space routes registered more than once: %v", duplicated)
	}
}

func TestSpaceRoutesAreRegisteredOnlyByTheSeam(t *testing.T) {
	gin.SetMode(gin.TestMode)
	server := NewServer(nil, nil, nil, nil, "0")
	// The core registrar alone (no feature registrars) must produce at least
	// the golden set, so a feature can never be what carries a core route.
	saved := spaceFeatureRouteRegistrars
	spaceFeatureRouteRegistrars = nil
	t.Cleanup(func() { spaceFeatureRouteRegistrars = saved })

	engine := gin.New()
	group := engine.Group("/api/v1/internal/spaces")
	server.registerSpaceRoutes(group)
	counts := routeCounts(engine.Routes())
	for _, route := range preSeamSpaceRoutes {
		if counts[route] != 1 {
			t.Errorf("core registerSpaceRoutes did not register %q exactly once (got %d)", route, counts[route])
		}
	}
	if len(counts) != len(preSeamSpaceRoutes) {
		t.Errorf("core registerSpaceRoutes registered %d routes, golden has %d", len(counts), len(preSeamSpaceRoutes))
	}
}

func TestRegisterSpaceFeatureRouteAppearsOnEngine(t *testing.T) {
	gin.SetMode(gin.TestMode)
	savedSpace := spaceFeatureRouteRegistrars
	t.Cleanup(func() { spaceFeatureRouteRegistrars = savedSpace })

	registerSpaceFeatureRoute(func(s *Server, spaces *gin.RouterGroup) {
		spaces.GET("/:space_ref/p-uc1-probe", func(c *gin.Context) { c.Status(http.StatusNoContent) })
	})

	server := NewServer(nil, nil, nil, nil, "0")
	counts := routeCounts(server.router.Routes())
	if counts["GET /api/v1/internal/spaces/:space_ref/p-uc1-probe"] != 1 {
		t.Fatalf("feature registrar route not on the engine: %v", counts)
	}
	// The probe must not displace any pre-seam route.
	for _, route := range preSeamSpaceRoutes {
		if counts[route] != 1 {
			t.Errorf("pre-seam route %q lost or duplicated after adding a feature registrar (got %d)", route, counts[route])
		}
	}
}

func TestRegisterFeatureRouteAppearsOnV1AndInternalGroups(t *testing.T) {
	gin.SetMode(gin.TestMode)
	saved := featureRouteRegistrars
	t.Cleanup(func() { featureRouteRegistrars = saved })

	registerFeatureRoute(func(s *Server, v1 *gin.RouterGroup, internal *gin.RouterGroup) {
		v1.GET("/p-uc1-v1-probe", func(c *gin.Context) { c.Status(http.StatusNoContent) })
		internal.GET("/p-uc1-internal-probe", func(c *gin.Context) { c.Status(http.StatusNoContent) })
	})

	server := NewServer(nil, nil, nil, nil, "0")
	counts := routeCounts(server.router.Routes())
	for _, want := range []string{
		"GET /api/v1/p-uc1-v1-probe",
		"GET /api/v1/internal/p-uc1-internal-probe",
	} {
		if counts[want] != 1 {
			t.Errorf("registerFeatureRoute route %q not on the engine exactly once (got %d)", want, counts[want])
		}
	}
}
