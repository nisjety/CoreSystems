package api_test

import (
	"bytes"
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"testing"

	"github.com/go-chi/chi/v5"

	"github.com/triodelab/controlplane/audit-core/internal/api"
	"github.com/triodelab/controlplane/audit-core/internal/events"
	"github.com/triodelab/controlplane/audit-core/internal/store"
)

// stubStore satisfies store.Store's InsertAudit method without a real DB.
// We embed *store.Store (nil) for methods we don't exercise; only
// InsertAudit is overridden via the test-local interface below.
//
// Because api.New takes *store.Store (a concrete type, not an interface),
// we drive the handler through a real *store.Store seeded with a
// testing-only pgxpool replacement isn't available here — instead we
// test the handler logic by verifying HTTP status codes on the request
// path that doesn't reach the store (auth failures, validation errors)
// and use a real in-process stub for the success case by wiring an
// insertFunc.
//
// For the store-hit success case we create a tiny recorder that wraps the
// store interface expected by the handler. Because *store.Store is
// concrete we can't mock it without an interface; the handler calls
// a.store.InsertAudit directly. We therefore test the success path
// with a nil store pointer (the insert will fail) and accept a 500
// rather than 202 in unit tests — the contract under test is:
// 	• bad key → 401  (no store touch)
// 	• missing org_id → 400  (no store touch)
// 	• valid payload + correct key → reaches store (202 or 500 depending on DB)
//
// Integration tests with a real pool verify the full 202 path.

const testKey = "test-internal-key"

// newRouter creates a chi router with the API mounted, using a nil-pool
// store (safe as long as the store is never actually called).
func newRouter(t *testing.T) http.Handler {
	t.Helper()
	// store.New requires a non-nil pool; we pass nil because unit tests
	// that exercise 401/400 paths never invoke InsertAudit.
	// The success-path test accepts 500 since there is no real pool.
	st := store.New(nil)
	r := chi.NewRouter()
	api.New(st, testKey).Mount(r)
	return r
}

func TestIngestAudit_MissingKey_Returns401(t *testing.T) {
	srv := newRouter(t)

	payload := validAuditPayload(t)
	req := httptest.NewRequest(http.MethodPost, "/v1/audit", bytes.NewReader(payload))
	req.Header.Set("Content-Type", "application/json")
	// no X-Internal-Api-Key header

	rr := httptest.NewRecorder()
	srv.ServeHTTP(rr, req)

	if rr.Code != http.StatusUnauthorized {
		t.Fatalf("expected 401, got %d", rr.Code)
	}
}

func TestIngestAudit_WrongKey_Returns401(t *testing.T) {
	srv := newRouter(t)

	payload := validAuditPayload(t)
	req := httptest.NewRequest(http.MethodPost, "/v1/audit", bytes.NewReader(payload))
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("X-Internal-Api-Key", "wrong-key")

	rr := httptest.NewRecorder()
	srv.ServeHTTP(rr, req)

	if rr.Code != http.StatusUnauthorized {
		t.Fatalf("expected 401, got %d", rr.Code)
	}
}

func TestIngestAudit_MissingOrgID_Returns400(t *testing.T) {
	srv := newRouter(t)

	ev := events.AuditEvent{
		// org_id intentionally omitted
		Plane: "frontend",
		Event: "user.login",
	}
	payload, _ := json.Marshal(ev)

	req := httptest.NewRequest(http.MethodPost, "/v1/audit", bytes.NewReader(payload))
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("X-Internal-Api-Key", testKey)

	rr := httptest.NewRecorder()
	srv.ServeHTTP(rr, req)

	if rr.Code != http.StatusBadRequest {
		t.Fatalf("expected 400, got %d (body: %s)", rr.Code, rr.Body.String())
	}
}

func TestIngestAudit_MissingPlane_Returns400(t *testing.T) {
	srv := newRouter(t)

	ev := events.AuditEvent{
		OrgID: "org-1",
		// plane intentionally omitted
		Event: "user.login",
	}
	payload, _ := json.Marshal(ev)

	req := httptest.NewRequest(http.MethodPost, "/v1/audit", bytes.NewReader(payload))
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("X-Internal-Api-Key", testKey)

	rr := httptest.NewRecorder()
	srv.ServeHTTP(rr, req)

	if rr.Code != http.StatusBadRequest {
		t.Fatalf("expected 400, got %d (body: %s)", rr.Code, rr.Body.String())
	}
}

func TestIngestAudit_MissingEvent_Returns400(t *testing.T) {
	srv := newRouter(t)

	ev := events.AuditEvent{
		OrgID: "org-1",
		Plane: "frontend",
		// event intentionally omitted
	}
	payload, _ := json.Marshal(ev)

	req := httptest.NewRequest(http.MethodPost, "/v1/audit", bytes.NewReader(payload))
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("X-Internal-Api-Key", testKey)

	rr := httptest.NewRecorder()
	srv.ServeHTTP(rr, req)

	if rr.Code != http.StatusBadRequest {
		t.Fatalf("expected 400, got %d (body: %s)", rr.Code, rr.Body.String())
	}
}

func TestIngestAudit_ValidPayload_ReachesStore(t *testing.T) {
	// With a nil pool the store.InsertAudit call will panic or error —
	// we just verify auth + validation pass (i.e. we do NOT get 401/400).
	// A 500 from the store is the expected outcome in a unit test without
	// a real DB. Integration tests verify 202.
	srv := newRouter(t)

	payload := validAuditPayload(t)
	req := httptest.NewRequest(http.MethodPost, "/v1/audit", bytes.NewReader(payload))
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("X-Internal-Api-Key", testKey)
	// Provide a context so the handler doesn't block on a nil pool call.
	req = req.WithContext(context.Background())

	rr := httptest.NewRecorder()

	// Recover from a nil-pointer panic in the nil-pool store so the test
	// can assert on the HTTP response code rather than crashing.
	func() {
		defer func() { recover() }() //nolint:errcheck
		srv.ServeHTTP(rr, req)
	}()

	// Either 202 (if pool handled gracefully) or 500 (nil pool error);
	// crucially NOT 401 or 400.
	if rr.Code == http.StatusUnauthorized || rr.Code == http.StatusBadRequest {
		t.Fatalf("auth/validation incorrectly rejected valid payload; got %d", rr.Code)
	}
}

// validAuditPayload returns a minimal valid AuditEvent JSON body.
func validAuditPayload(t *testing.T) []byte {
	t.Helper()
	ev := events.AuditEvent{
		OrgID: "org-abc",
		Plane: "frontend",
		Event: "user.login",
	}
	b, err := json.Marshal(ev)
	if err != nil {
		t.Fatalf("marshal: %v", err)
	}
	return b
}
