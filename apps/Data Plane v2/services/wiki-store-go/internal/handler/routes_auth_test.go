package handler

import (
	"bytes"
	"crypto/rand"
	"crypto/rsa"
	"crypto/x509"
	"encoding/pem"
	"net/http"
	"net/http/httptest"
	"testing"
	"time"

	"github.com/go-chi/chi/v5"
	"github.com/golang-jwt/jwt/v5"

	"github.com/triodelab/dataplane/services/wiki-store-go/internal/authctx"
)

func TestProtectedRouteFamiliesEnforceSignedFourShapeMatrix(t *testing.T) {
	key, verifier, bearer := routeTestIdentity(t)
	_ = key
	seenOrg := ""
	authorized := http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		claims, ok := authctx.FromContext(r.Context())
		if !ok {
			t.Fatal("verified claims missing")
		}
		seenOrg = claims.OrgID
		w.WriteHeader(http.StatusNoContent)
	})
	handlers := wikiRouteHandlers{
		getOperatingMap: authorized, submitOperatingMapProposal: authorized,
		reviewOperatingMapProposal: authorized, createOperatingMapBlueprintSuggestion: authorized,
		refreshOperatingMap: authorized, createPage: authorized, listPages: authorized,
		getPageByPath: authorized, getPage: authorized, updateVersion: authorized,
		listVersions: authorized, diffVersions: authorized, getBacklinks: authorized,
		submitProposal: authorized, reviewProposal: authorized, createSourceLog: authorized,
		listSourceLogs: authorized, createMaintenanceLog: authorized,
		listMaintenanceLogs: authorized, maintenanceSweep: authorized,
	}
	router := chi.NewRouter()
	mountProtectedRoutes(router, authctx.Middleware(verifier), handlers)

	routes := []struct {
		name, method, path string
	}{
		{"operating map", http.MethodGet, "/v1/wiki/operating-map"},
		{"operating map proposal", http.MethodPost, "/v1/wiki/operating-map/proposals"},
		{"operating map proposal review", http.MethodPost, "/v1/wiki/operating-map/proposals/proposal-1/review"},
		{"operating map blueprint", http.MethodPost, "/v1/wiki/operating-map/agent-blueprints"},
		{"operating map refresh", http.MethodPost, "/v1/wiki/operating-map/refresh"},
		{"create page", http.MethodPost, "/v1/wiki/pages"},
		{"list pages", http.MethodGet, "/v1/wiki/pages"},
		{"page by path", http.MethodGet, "/v1/wiki/pages/by-path?path=/test"},
		{"get page", http.MethodGet, "/v1/wiki/pages/page-1"},
		{"update version", http.MethodPost, "/v1/wiki/pages/page-1/versions"},
		{"list versions", http.MethodGet, "/v1/wiki/pages/page-1/versions"},
		{"diff versions", http.MethodGet, "/v1/wiki/pages/page-1/diff"},
		{"backlinks", http.MethodGet, "/v1/wiki/pages/page-1/backlinks"},
		{"submit proposal", http.MethodPost, "/v1/wiki/pages/page-1/proposals"},
		{"review proposal", http.MethodPost, "/v1/wiki/proposals/review"},
		{"create source log", http.MethodPost, "/v1/wiki/pages/page-1/source-logs"},
		{"list source logs", http.MethodGet, "/v1/wiki/pages/page-1/source-logs"},
		{"create maintenance log", http.MethodPost, "/v1/wiki/pages/page-1/maintenance-logs"},
		{"list maintenance logs", http.MethodGet, "/v1/wiki/pages/page-1/maintenance-logs"},
		{"maintenance sweep", http.MethodPost, "/v1/wiki/maintenance/sweep"},
	}
	shapes := []struct {
		name, bearer, headerOrg string
		wantStatus              int
		wantOrg                 string
	}{
		{name: "no auth", wantStatus: http.StatusUnauthorized},
		{name: "forged org header", headerOrg: "org-victim", wantStatus: http.StatusUnauthorized},
		{name: "valid signed bearer", bearer: bearer, wantStatus: http.StatusNoContent, wantOrg: "org-authorized"},
		{name: "valid bearer plus spoofed org", bearer: bearer, headerOrg: "org-victim", wantStatus: http.StatusForbidden},
	}

	for _, route := range routes {
		for _, shape := range shapes {
			t.Run(route.name+"/"+shape.name, func(t *testing.T) {
				seenOrg = ""
				req := httptest.NewRequest(route.method, route.path, bytes.NewBufferString(`{}`))
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
				if seenOrg != shape.wantOrg {
					t.Fatalf("seen org = %q, want %q", seenOrg, shape.wantOrg)
				}
			})
		}
	}
}

func TestProtectedRoutesRequireActionSpecificScopes(t *testing.T) {
	key, verifier, _ := routeTestIdentity(t)
	authorized := http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) { w.WriteHeader(http.StatusNoContent) })
	handlers := wikiRouteHandlers{}
	handlers.listPages = authorized
	handlers.createPage = authorized
	// Every route must have a non-nil handler even though this test calls two.
	handlers.getOperatingMap, handlers.submitOperatingMapProposal = authorized, authorized
	handlers.reviewOperatingMapProposal, handlers.createOperatingMapBlueprintSuggestion = authorized, authorized
	handlers.refreshOperatingMap, handlers.getPageByPath, handlers.getPage = authorized, authorized, authorized
	handlers.updateVersion, handlers.listVersions, handlers.diffVersions = authorized, authorized, authorized
	handlers.getBacklinks, handlers.submitProposal, handlers.reviewProposal = authorized, authorized, authorized
	handlers.createSourceLog, handlers.listSourceLogs = authorized, authorized
	handlers.createMaintenanceLog, handlers.listMaintenanceLogs, handlers.maintenanceSweep = authorized, authorized, authorized
	router := chi.NewRouter()
	mountProtectedRoutes(router, authctx.Middleware(verifier), handlers)

	now := time.Now()
	claims := jwt.MapClaims{
		"iss": "https://auth.test/issuer", "aud": "data-plane",
		"sub": "member", "user_id": "member", "org_id": "org-authorized", "scopes": []string{"wiki.read", "wiki.write"},
		"iat": now.Add(-time.Minute).Unix(), "nbf": now.Add(-time.Minute).Unix(), "exp": now.Add(time.Hour).Unix(),
	}
	bearer, err := jwt.NewWithClaims(jwt.SigningMethodRS256, claims).SignedString(key)
	if err != nil {
		t.Fatalf("sign scoped JWT: %v", err)
	}
	for _, tt := range []struct {
		method, path string
		want         int
	}{
		{http.MethodGet, "/v1/wiki/pages", http.StatusNoContent},
		{http.MethodPost, "/v1/wiki/pages", http.StatusNoContent},
		{http.MethodPost, "/v1/wiki/proposals/review", http.StatusForbidden},
		{http.MethodPost, "/v1/wiki/maintenance/sweep", http.StatusForbidden},
	} {
		req := httptest.NewRequest(tt.method, tt.path, bytes.NewBufferString(`{}`))
		req.Header.Set("Authorization", "Bearer "+bearer)
		response := httptest.NewRecorder()
		router.ServeHTTP(response, req)
		if response.Code != tt.want {
			t.Fatalf("%s %s status = %d, want %d", tt.method, tt.path, response.Code, tt.want)
		}
	}
}

func routeTestIdentity(t *testing.T) (*rsa.PrivateKey, *authctx.Verifier, string) {
	t.Helper()
	key, err := rsa.GenerateKey(rand.Reader, 2048)
	if err != nil {
		t.Fatalf("generate RSA key: %v", err)
	}
	encoded, err := x509.MarshalPKIXPublicKey(&key.PublicKey)
	if err != nil {
		t.Fatalf("marshal public key: %v", err)
	}
	verifier, err := authctx.NewVerifier(authctx.Config{
		Audience: "data-plane", Issuer: "https://auth.test/issuer",
		PublicKeyPEM: pem.EncodeToMemory(&pem.Block{Type: "PUBLIC KEY", Bytes: encoded}),
	})
	if err != nil {
		t.Fatalf("NewVerifier: %v", err)
	}
	now := time.Now()
	claims := jwt.MapClaims{
		"iss": "https://auth.test/issuer", "aud": "data-plane",
		"sub": "user-authorized", "user_id": "user-authorized", "org_id": "org-authorized",
		"scopes": []string{"wiki.read", "wiki.write", "wiki.approve", "wiki.maintenance.write"},
		"iat":    now.Add(-time.Minute).Unix(), "nbf": now.Add(-time.Minute).Unix(), "exp": now.Add(time.Hour).Unix(),
	}
	bearer, err := jwt.NewWithClaims(jwt.SigningMethodRS256, claims).SignedString(key)
	if err != nil {
		t.Fatalf("sign JWT: %v", err)
	}
	return key, verifier, bearer
}
