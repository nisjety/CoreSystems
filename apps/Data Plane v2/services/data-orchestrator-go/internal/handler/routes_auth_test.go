package handler

import (
	"context"
	"errors"
	"net/http"
	"net/http/httptest"
	"testing"

	"github.com/go-chi/chi/v5"

	"github.com/triodelab/dataplane/services/data-orchestrator-go/internal/authctx"
)

func TestOrgIDFromDoesNotFallBackWithoutVerifiedClaims(t *testing.T) {
	if got := orgIDFrom(context.Background()); got != "" {
		t.Fatalf("orgIDFrom without claims = %q, want empty", got)
	}
}

func TestExportedRouteMountKeepsAuthBoundary(t *testing.T) {
	router := chi.NewRouter()
	MountProtectedRoutes(router, authctx.Middleware(routeTestVerifier{}), &OrchestratorHandler{})
	request := httptest.NewRequest(http.MethodGet, "/v1/orchestrator/stale-embeddings", nil)
	response := httptest.NewRecorder()

	router.ServeHTTP(response, request)

	if response.Code != http.StatusUnauthorized {
		t.Fatalf("status = %d, want 401", response.Code)
	}
}

type routeTestVerifier struct{}

func (routeTestVerifier) Verify(token string) (*authctx.Claims, error) {
	if token != "valid-token" && token != "member-token" {
		return nil, errors.New("invalid token")
	}
	claims := &authctx.Claims{
		OrgID:    "org_authorized",
		UserID:   "user_authorized",
		Verified: true,
	}
	if token == "valid-token" {
		claims.Scopes = []string{"data:orchestrate"}
	}
	claims.Subject = claims.UserID
	return claims, nil
}

func TestOrchestratorMutationsRequireDedicatedScope(t *testing.T) {
	router := chi.NewRouter()
	called := http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		w.WriteHeader(http.StatusNoContent)
	})
	mountProtectedRoutes(router, authctx.Middleware(routeTestVerifier{}), orchestratorRouteHandlers{
		createJob: called, getJob: called, reindex: called, staleEmbeddings: called,
	})

	request := httptest.NewRequest(http.MethodPost, "/v1/orchestrator/reindex", nil)
	request.Header.Set("Authorization", "Bearer member-token")
	response := httptest.NewRecorder()
	router.ServeHTTP(response, request)

	if response.Code != http.StatusForbidden {
		t.Fatalf("status = %d, want 403", response.Code)
	}
}

func TestProtectedRouteFamiliesEnforceFourShapeMatrix(t *testing.T) {
	router := chi.NewRouter()
	authorized := http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("X-Seen-Org-ID", orgIDFrom(r.Context()))
		w.WriteHeader(http.StatusNoContent)
	})
	mountProtectedRoutes(router, authctx.Middleware(routeTestVerifier{}), orchestratorRouteHandlers{
		createJob:       authorized,
		getJob:          authorized,
		reindex:         authorized,
		staleEmbeddings: authorized,
	})

	routes := []struct {
		name   string
		method string
		path   string
	}{
		{name: "create job", method: http.MethodPost, path: "/v1/orchestrator/jobs"},
		{name: "get job", method: http.MethodGet, path: "/v1/orchestrator/jobs/91b88438-4190-45f3-8674-58da542d8faf"},
		{name: "reindex", method: http.MethodPost, path: "/v1/orchestrator/reindex"},
		{name: "stale embeddings", method: http.MethodGet, path: "/v1/orchestrator/stale-embeddings"},
	}
	shapes := []struct {
		name       string
		bearer     string
		headerOrg  string
		wantStatus int
		wantOrg    string
	}{
		{name: "no auth", wantStatus: http.StatusUnauthorized},
		{name: "forged org header", headerOrg: "org_victim", wantStatus: http.StatusUnauthorized},
		{name: "valid verified identity", bearer: "valid-token", wantStatus: http.StatusNoContent, wantOrg: "org_authorized"},
		{name: "valid identity plus spoofed org", bearer: "valid-token", headerOrg: "org_victim", wantStatus: http.StatusForbidden},
	}

	for _, route := range routes {
		for _, shape := range shapes {
			t.Run(route.name+"/"+shape.name, func(t *testing.T) {
				req := httptest.NewRequest(route.method, route.path, nil)
				if shape.bearer != "" {
					req.Header.Set("Authorization", "Bearer "+shape.bearer)
				}
				if shape.headerOrg != "" {
					req.Header.Set("X-Org-ID", shape.headerOrg)
				}
				response := httptest.NewRecorder()

				router.ServeHTTP(response, req)

				if response.Code != shape.wantStatus {
					t.Fatalf("status = %d, want %d; body=%s", response.Code, shape.wantStatus, response.Body.String())
				}
				if got := response.Header().Get("X-Seen-Org-ID"); got != shape.wantOrg {
					t.Fatalf("seen org = %q, want %q", got, shape.wantOrg)
				}
			})
		}
	}
}
