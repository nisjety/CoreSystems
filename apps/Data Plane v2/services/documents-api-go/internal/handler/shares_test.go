package handler

import (
	"bytes"
	"context"
	"net/http"
	"net/http/httptest"
	"testing"

	"github.com/go-chi/chi/v5"

	"github.com/triodelab/dataplane/services/documents-api-go/internal/userauthz"
)

// WS14-S4: share routes. These cover everything decided BEFORE the owner proof
// (flag, principal kind, bearer, retention posture, wiring). The owner proof
// itself reads the document, so owner / non-owner / role / self-share / cap /
// Control-error cases run against a database in shares_live_test.go
// (build tag `integration`).

type stubShareGrants struct {
	calls []string
}

func (s *stubShareGrants) Grant(_ context.Context, _, _, _, _, _, _ string) (*userauthz.Grant, error) {
	s.calls = append(s.calls, "grant")
	return &userauthz.Grant{}, nil
}
func (s *stubShareGrants) Revoke(_ context.Context, _, _, _, _, _ string) error {
	s.calls = append(s.calls, "revoke")
	return nil
}
func (s *stubShareGrants) ListGrants(_ context.Context, _, _, _, _ string) ([]userauthz.Grant, error) {
	s.calls = append(s.calls, "list")
	return nil, nil
}

func shareRouter(auth *testAuth, h *DocumentHandler) http.Handler {
	r := chi.NewRouter()
	r.Use(func(next http.Handler) http.Handler { return auth.wrap(next) })
	r.Get("/v1/documents/{documentID}/shares", h.ListShares)
	r.Post("/v1/documents/{documentID}/shares", h.CreateShare)
	r.Delete("/v1/documents/{documentID}/shares/{subjectID}", h.DeleteShare)
	return r
}

func shareCall(r http.Handler, method, path, bearer, body string) *httptest.ResponseRecorder {
	req := httptest.NewRequest(method, path, bytes.NewBufferString(body))
	if bearer != "" {
		req.Header.Set("Authorization", "Bearer "+bearer)
	}
	req.Header.Set("X-Org-ID", testOrg)
	rec := httptest.NewRecorder()
	r.ServeHTTP(rec, req)
	return rec
}

var shareRoutes = []struct{ name, method, path, body string }{
	{"list", http.MethodGet, "/v1/documents/doc-1/shares", ""},
	{"add", http.MethodPost, "/v1/documents/doc-1/shares", `{"subject_id":"user-2","role":"view"}`},
	{"remove", http.MethodDelete, "/v1/documents/doc-1/shares/user-2", ""},
}

func TestShareRoutesAreOffByDefaultWith404SharesDisabled(t *testing.T) {
	auth := newTestAuth(t)
	stub := &stubShareGrants{}
	// A zero SharesConfig is "off"; wiring a client does not turn it on.
	router := shareRouter(auth, NewDocumentHandler(nil, nil).WithShares(SharesConfig{Grants: stub}))
	token := auth.userToken(testOrg, "owner-1", false)
	for _, tc := range shareRoutes {
		rec := shareCall(router, tc.method, tc.path, token, tc.body)
		if rec.Code != http.StatusNotFound || decodeError(t, rec)["code"] != "shares_disabled" {
			t.Errorf("%s: %d %s", tc.name, rec.Code, rec.Body.String())
		}
	}
	if len(stub.calls) != 0 {
		t.Fatalf("Control was called with the flag off: %v", stub.calls)
	}
}

func TestShareRoutesRefuseServicePrincipalsAndMissingBearer(t *testing.T) {
	auth := newTestAuth(t)
	stub := &stubShareGrants{}
	h := NewDocumentHandler(nil, nil).WithShares(SharesConfig{Enabled: true, Grants: stub})
	router := shareRouter(auth, h)

	// A service principal (even with write-all) is never a share actor.
	service := auth.serviceToken(testOrg, "gateway", "org:data:write_all", "documents:write")
	for _, tc := range shareRoutes {
		rec := shareCall(router, tc.method, tc.path, service, tc.body)
		if rec.Code != http.StatusForbidden {
			t.Errorf("service %s: %d %s", tc.name, rec.Code, rec.Body.String())
		}
		// No token at all never reaches the handler (verification is enforced).
		if rec := shareCall(router, tc.method, tc.path, "", tc.body); rec.Code != http.StatusUnauthorized {
			t.Errorf("anonymous %s: %d", tc.name, rec.Code)
		}
	}

	if len(stub.calls) != 0 {
		t.Fatalf("Control was called: %v", stub.calls)
	}
}

func TestShareWritesRefuseVerifiedZDRPosture(t *testing.T) {
	auth := newTestAuth(t)
	stub := &stubShareGrants{}
	router := shareRouter(auth, NewDocumentHandler(nil, nil).WithShares(SharesConfig{Enabled: true, Grants: stub}))
	zdr := auth.userToken(testOrg, "owner-1", true)
	for _, tc := range shareRoutes[1:] { // add, remove: durable writes
		rec := shareCall(router, tc.method, tc.path, zdr, tc.body)
		if rec.Code != http.StatusForbidden {
			t.Errorf("%s with zdr=true: %d %s", tc.name, rec.Code, rec.Body.String())
		}
		if len(stub.calls) != 0 {
			t.Fatalf("%s reached Control under zdr=true: %v", tc.name, stub.calls)
		}
	}
}

func TestShareRoutesWithoutAControlClientAreUnavailableNotOpen(t *testing.T) {
	auth := newTestAuth(t)
	router := shareRouter(auth, NewDocumentHandler(nil, nil).WithShares(SharesConfig{Enabled: true}))
	token := auth.userToken(testOrg, "owner-1", false)
	for _, tc := range shareRoutes {
		rec := shareCall(router, tc.method, tc.path, token, tc.body)
		if rec.Code != http.StatusServiceUnavailable || decodeError(t, rec)["code"] != "sharing_unavailable" {
			t.Errorf("%s: %d %s", tc.name, rec.Code, rec.Body.String())
		}
	}
}

func TestSharedWithMeIsReadOnlyIndependentOfTheWriteFlagAndNeedsAnEndUser(t *testing.T) {
	auth := newTestAuth(t)
	h := NewDocumentHandler(nil, nil) // shares off, no grant client
	handler := auth.wrap(http.HandlerFunc(h.SharedWithMe))
	call := func(bearer string) *httptest.ResponseRecorder {
		return shareCall(handler, http.MethodGet, "/v1/documents/shared-with-me", bearer, "")
	}
	if rec := call(auth.serviceToken(testOrg, "gateway", "documents:read")); rec.Code != http.StatusForbidden {
		t.Fatalf("service principal: %d", rec.Code)
	}
	// Flag off does NOT 404 this route; with no Control client it is a typed 503.
	rec := call(auth.userToken(testOrg, "viewer-1", false))
	if rec.Code != http.StatusServiceUnavailable || decodeError(t, rec)["code"] != "sharing_unavailable" {
		t.Fatalf("flag off, no authz client: %d %s", rec.Code, rec.Body.String())
	}
}
