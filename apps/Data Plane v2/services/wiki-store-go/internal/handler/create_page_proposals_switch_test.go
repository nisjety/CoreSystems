package handler

import (
	"bytes"
	"net/http"
	"net/http/httptest"
	"testing"
	"time"

	"github.com/go-chi/chi/v5"
	"github.com/golang-jwt/jwt/v5"

	"github.com/triodelab/dataplane/services/wiki-store-go/internal/authctx"
)

// W5-094-a: the two create-page proposal routes exist only while WIKI_CREATE_PAGE_PROPOSALS_ENABLED is on.
// Off, they answer the router's 404 for every caller and every other route is exactly as before. On, they
// sit behind the same signed-bearer boundary, scopes and ZDR durable-write guard as the rest of the wiki.

// switchHandlers returns a full handler set where every handler records that it ran.
func switchHandlers(called map[string]bool, createPageProposals bool) wikiRouteHandlers {
	mark := func(name string) http.Handler {
		return http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
			called[name] = true
			w.WriteHeader(http.StatusNoContent)
		})
	}
	return wikiRouteHandlers{
		getOperatingMap: mark("getOperatingMap"), submitOperatingMapProposal: mark("submitOperatingMapProposal"),
		reviewOperatingMapProposal: mark("reviewOperatingMapProposal"), createOperatingMapBlueprintSuggestion: mark("blueprint"),
		refreshOperatingMap: mark("refreshOperatingMap"), createPage: mark("createPage"), listPages: mark("listPages"),
		getPageByPath: mark("getPageByPath"), getPage: mark("getPage"), updateVersion: mark("updateVersion"),
		listVersions: mark("listVersions"), diffVersions: mark("diffVersions"), getBacklinks: mark("getBacklinks"),
		submitProposal: mark("submitProposal"), reviewProposal: mark("reviewProposal"), createSourceLog: mark("createSourceLog"),
		listSourceLogs: mark("listSourceLogs"), createMaintenanceLog: mark("createMaintenanceLog"),
		listMaintenanceLogs: mark("listMaintenanceLogs"), maintenanceSweep: mark("maintenanceSweep"),

		createPageProposals:      createPageProposals,
		submitCreatePageProposal: mark("submitCreatePageProposal"),
		listProposals:            mark("listProposals"),
	}
}

func signedBearer(t *testing.T, key any, scopes []string, zdr bool) string {
	t.Helper()
	now := time.Now()
	token, err := jwt.NewWithClaims(jwt.SigningMethodRS256, jwt.MapClaims{
		"iss": "https://auth.test/issuer", "aud": "data-plane",
		"sub": "member", "user_id": "member", "org_id": "org-authorized", "scopes": scopes, "zdr": zdr,
		"iat": now.Add(-time.Minute).Unix(), "nbf": now.Add(-time.Minute).Unix(), "exp": now.Add(time.Hour).Unix(),
	}).SignedString(key)
	if err != nil {
		t.Fatalf("sign JWT: %v", err)
	}
	return token
}

func serve(router http.Handler, method, path, bearer string) int {
	req := httptest.NewRequest(method, path, bytes.NewBufferString(`{"path":"/new","title":"New","content":"x"}`))
	if bearer != "" {
		req.Header.Set("Authorization", "Bearer "+bearer)
	}
	response := httptest.NewRecorder()
	router.ServeHTTP(response, req)
	return response.Code
}

func TestCreatePageProposalRoutesAreAbsentWhileTheSwitchIsOff(t *testing.T) {
	_, verifier, bearer := routeTestIdentity(t)
	called := map[string]bool{}
	router := chi.NewRouter()
	mountProtectedRoutes(router, authctx.Middleware(verifier), switchHandlers(called, false))

	for _, route := range []struct{ method, path string }{
		{http.MethodPost, "/v1/wiki/proposals/create-page"},
		{http.MethodGet, "/v1/wiki/proposals"},
	} {
		if status := serve(router, route.method, route.path, bearer); status != http.StatusNotFound {
			t.Errorf("%s %s with the switch off = %d, want 404", route.method, route.path, status)
		}
	}
	if called["submitCreatePageProposal"] || called["listProposals"] {
		t.Fatal("a create-page proposal handler ran with the switch off")
	}

	// Every route that existed before answers exactly as before (the signed bearer reaches its handler).
	for _, route := range []struct{ method, path, handler string }{
		{http.MethodPost, "/v1/wiki/pages/page-1/proposals", "submitProposal"},
		{http.MethodPost, "/v1/wiki/proposals/review", "reviewProposal"},
		{http.MethodPost, "/v1/wiki/pages", "createPage"},
		{http.MethodGet, "/v1/wiki/pages", "listPages"},
	} {
		if status := serve(router, route.method, route.path, bearer); status != http.StatusNoContent || !called[route.handler] {
			t.Errorf("%s %s with the switch off = %d (handler ran %v), want 204 through its handler", route.method, route.path, status, called[route.handler])
		}
	}
}

func TestCreatePageProposalRoutesSitBehindTheWikiBoundaryWhileTheSwitchIsOn(t *testing.T) {
	key, verifier, bearer := routeTestIdentity(t)
	called := map[string]bool{}
	router := chi.NewRouter()
	mountProtectedRoutes(router, authctx.Middleware(verifier), switchHandlers(called, true))

	if status := serve(router, http.MethodPost, "/v1/wiki/proposals/create-page", ""); status != http.StatusUnauthorized {
		t.Errorf("create-page proposal without a bearer = %d, want 401", status)
	}
	if status := serve(router, http.MethodGet, "/v1/wiki/proposals", ""); status != http.StatusUnauthorized {
		t.Errorf("proposal list without a bearer = %d, want 401", status)
	}
	if called["submitCreatePageProposal"] || called["listProposals"] {
		t.Fatal("an unauthenticated request reached a create-page proposal handler")
	}

	readOnly := signedBearer(t, key, []string{"wiki.read"}, false)
	if status := serve(router, http.MethodPost, "/v1/wiki/proposals/create-page", readOnly); status != http.StatusForbidden || called["submitCreatePageProposal"] {
		t.Errorf("create-page proposal with wiki.read only = %d, want 403 before the handler", status)
	}
	writeOnly := signedBearer(t, key, []string{"wiki.write"}, false)
	if status := serve(router, http.MethodGet, "/v1/wiki/proposals", writeOnly); status != http.StatusForbidden || called["listProposals"] {
		t.Errorf("proposal list with wiki.write only = %d, want 403 before the handler", status)
	}

	// A zero-retention caller files nothing durable.
	zdr := signedBearer(t, key, []string{"wiki.read", "wiki.write"}, true)
	if status := serve(router, http.MethodPost, "/v1/wiki/proposals/create-page", zdr); status != http.StatusForbidden || called["submitCreatePageProposal"] {
		t.Errorf("create-page proposal under ZDR = %d, want 403 before the handler", status)
	}

	if status := serve(router, http.MethodPost, "/v1/wiki/proposals/create-page", bearer); status != http.StatusNoContent || !called["submitCreatePageProposal"] {
		t.Errorf("create-page proposal with the switch on = %d, want 204 through its handler", status)
	}
	if status := serve(router, http.MethodGet, "/v1/wiki/proposals", bearer); status != http.StatusNoContent || !called["listProposals"] {
		t.Errorf("proposal list with the switch on = %d, want 204 through its handler", status)
	}
	// The review route is not shadowed by the new /proposals routes.
	if status := serve(router, http.MethodPost, "/v1/wiki/proposals/review", bearer); status != http.StatusNoContent || !called["reviewProposal"] {
		t.Errorf("proposal review with the switch on = %d, want 204 through its own handler", status)
	}
}

func TestMountRoutesCarriesTheSwitchFromTheHandler(t *testing.T) {
	_, verifier, bearer := routeTestIdentity(t)
	for _, on := range []bool{false, true} {
		router := chi.NewRouter()
		MountRoutes(router, authctx.Middleware(verifier), NewWikiHandler(nil, on))
		status := serve(router, http.MethodGet, "/v1/wiki/proposals?status=nonsense", bearer)
		if !on && status != http.StatusNotFound {
			t.Errorf("MountRoutes with the switch off: proposal list = %d, want 404", status)
		}
		if on && status == http.StatusNotFound {
			t.Errorf("MountRoutes with the switch on: proposal list = 404, want the route mounted")
		}
	}
}
