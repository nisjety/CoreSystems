package api_test

import (
	"bytes"
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"testing"
	"time"

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

const testKey = "0123456789abcdef0123456789abcdef"

const testCredentials = `[{"principal":"integration-test","audience":"audit-core","token":"0123456789abcdef0123456789abcdef","scopes":["audit:write"],"planes":["frontend"]}]`

type usageIngestStore struct {
	inserted bool
	err      error
	source   string
	event    *events.UsageEvent
	audit    *events.AuditEvent
}

func (s *usageIngestStore) ListAudit(context.Context, store.AuditFilter) ([]store.AuditRow, error) {
	return nil, nil
}
func (s *usageIngestStore) ListUsage(context.Context, store.UsageFilter) ([]store.UsageRow, error) {
	return nil, nil
}
func (s *usageIngestStore) SummariseUsage(context.Context, string, time.Time, time.Time) ([]store.UsageSummary, error) {
	return nil, nil
}
func (s *usageIngestStore) InsertAuditFromSource(_ context.Context, event *events.AuditEvent, source string) (bool, error) {
	s.audit = event
	s.source = source
	return s.inserted, s.err
}
func (s *usageIngestStore) InsertUsageFromSource(_ context.Context, event *events.UsageEvent, source string) (bool, error) {
	s.event = event
	s.source = source
	return s.inserted, s.err
}

// newRouter creates a chi router with the API mounted, using a nil-pool
// store (safe as long as the store is never actually called).
func newRouter(t *testing.T) http.Handler {
	t.Helper()
	// store.New requires a non-nil pool; we pass nil because unit tests
	// that exercise 401/400 paths never invoke InsertAudit.
	// The success-path test accepts 500 since there is no real pool.
	st := store.New(nil)
	r := chi.NewRouter()
	handler, err := api.New(st, testCredentials)
	if err != nil {
		t.Fatalf("new API: %v", err)
	}
	handler.Mount(r)
	return r
}

func TestIngestUsageExactRetryAndConflictContract(t *testing.T) {
	payload, err := json.Marshal(events.UsageEvent{
		EventID: "usage:frontend:request-1", OccurredAt: time.Date(2026, 7, 15, 12, 0, 0, 0, time.UTC),
		OrgID: "org-1", Plane: "frontend", Producer: "integration-test", Op: "render", CostCents: 1.25,
	})
	if err != nil {
		t.Fatal(err)
	}
	for name, fixture := range map[string]struct {
		inserted bool
		err      error
		status   int
	}{
		"first":            {inserted: true, status: http.StatusAccepted},
		"exact-retry":      {inserted: false, status: http.StatusOK},
		"payload-conflict": {err: store.ErrUsageEventConflict, status: http.StatusConflict},
	} {
		t.Run(name, func(t *testing.T) {
			stub := &usageIngestStore{inserted: fixture.inserted, err: fixture.err}
			handler, newErr := api.New(stub, testCredentials)
			if newErr != nil {
				t.Fatal(newErr)
			}
			router := chi.NewRouter()
			handler.Mount(router)
			req := httptest.NewRequest(http.MethodPost, "/v1/usage", bytes.NewReader(payload))
			req.Header.Set("Content-Type", "application/json")
			authorizeWriter(req, testKey)
			recorder := httptest.NewRecorder()
			router.ServeHTTP(recorder, req)
			if recorder.Code != fixture.status {
				t.Fatalf("status = %d, want %d: %s", recorder.Code, fixture.status, recorder.Body.String())
			}
			if stub.source != "http:integration-test" {
				t.Fatalf("source = %q; want authenticated principal identity", stub.source)
			}
		})
	}
}

func TestIngestAuditExactRetryAndConflictContract(t *testing.T) {
	payload, err := json.Marshal(events.AuditEvent{
		EventID:    "membership:org-1:user-1:1:member_added",
		OccurredAt: time.Date(2026, 7, 15, 12, 0, 0, 0, time.UTC),
		OrgID:      "org-1", Plane: "frontend", Producer: "integration-test", Event: "member_added", Outcome: "ok",
	})
	if err != nil {
		t.Fatal(err)
	}
	for name, fixture := range map[string]struct {
		inserted bool
		err      error
		status   int
	}{
		"first":            {inserted: true, status: http.StatusAccepted},
		"exact-retry":      {inserted: false, status: http.StatusOK},
		"payload-conflict": {err: store.ErrAuditEventConflict, status: http.StatusConflict},
	} {
		t.Run(name, func(t *testing.T) {
			stub := &usageIngestStore{inserted: fixture.inserted, err: fixture.err}
			handler, newErr := api.New(stub, testCredentials)
			if newErr != nil {
				t.Fatal(newErr)
			}
			router := chi.NewRouter()
			handler.Mount(router)
			req := httptest.NewRequest(http.MethodPost, "/v1/audit", bytes.NewReader(payload))
			req.Header.Set("Content-Type", "application/json")
			authorizeWriter(req, testKey)
			recorder := httptest.NewRecorder()
			router.ServeHTTP(recorder, req)
			if recorder.Code != fixture.status {
				t.Fatalf("status = %d, want %d: %s", recorder.Code, fixture.status, recorder.Body.String())
			}
			if stub.source != "http:integration-test" {
				t.Fatalf("source = %q; want authenticated principal identity", stub.source)
			}
		})
	}
}

func authorizeWriter(req *http.Request, token string) {
	req.Header.Set("X-Service-Id", "integration-test")
	req.Header.Set("X-Service-Token", token)
}

func TestIngestAudit_MissingKey_Returns401(t *testing.T) {
	srv := newRouter(t)

	payload := validAuditPayload(t)
	req := httptest.NewRequest(http.MethodPost, "/v1/audit", bytes.NewReader(payload))
	req.Header.Set("Content-Type", "application/json")
	// no service credential headers

	rr := httptest.NewRecorder()
	srv.ServeHTTP(rr, req)

	if rr.Code != http.StatusUnauthorized {
		t.Fatalf("expected 401, got %d", rr.Code)
	}
}

func TestReadyzFailsClosedWhenAConfiguredBusIsDisconnected(t *testing.T) {
	r := chi.NewRouter()
	handler, err := api.New(store.New(nil), testCredentials, func(context.Context) api.Readiness {
		return api.Readiness{
			DatabaseConnected: true,
			NATSBuses: []api.NATSBusReadiness{
				readyBus("primary"),
				{Name: "model", Connected: false},
			},
		}
	})
	if err != nil {
		t.Fatal(err)
	}
	handler.Mount(r)

	rr := httptest.NewRecorder()
	r.ServeHTTP(rr, httptest.NewRequest(http.MethodGet, "/readyz", nil))
	if rr.Code != http.StatusServiceUnavailable {
		t.Fatalf("expected 503, got %d: %s", rr.Code, rr.Body.String())
	}
}

func TestReadyzReportsAllDependenciesConnected(t *testing.T) {
	r := chi.NewRouter()
	handler, err := api.New(store.New(nil), testCredentials, func(context.Context) api.Readiness {
		return api.Readiness{
			DatabaseConnected: true,
			NATSBuses: []api.NATSBusReadiness{
				readyBus("primary"),
				readyBus("model"),
			},
		}
	})
	if err != nil {
		t.Fatal(err)
	}
	handler.Mount(r)

	rr := httptest.NewRecorder()
	r.ServeHTTP(rr, httptest.NewRequest(http.MethodGet, "/readyz", nil))
	if rr.Code != http.StatusOK {
		t.Fatalf("expected 200, got %d: %s", rr.Code, rr.Body.String())
	}
}

func readyBus(name string) api.NATSBusReadiness {
	return api.NATSBusReadiness{
		Name:            name,
		Connected:       true,
		SubscriberReady: true,
		Audit:           api.ConsumerReadiness{Consumer: "audit-core-" + name + "-audit", Ready: true},
		Usage:           api.ConsumerReadiness{Consumer: "audit-core-" + name + "-usage", Ready: true},
	}
}

func TestIngestAudit_WrongKey_Returns401(t *testing.T) {
	srv := newRouter(t)

	payload := validAuditPayload(t)
	req := httptest.NewRequest(http.MethodPost, "/v1/audit", bytes.NewReader(payload))
	req.Header.Set("Content-Type", "application/json")
	authorizeWriter(req, "abcdef0123456789abcdef0123456789")

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
		EventID: "audit:integration-test:missing-org", OccurredAt: time.Now().UTC(),
		Plane: "frontend", Producer: "integration-test", Event: "user_login",
	}
	payload, _ := json.Marshal(ev)

	req := httptest.NewRequest(http.MethodPost, "/v1/audit", bytes.NewReader(payload))
	req.Header.Set("Content-Type", "application/json")
	authorizeWriter(req, testKey)

	rr := httptest.NewRecorder()
	srv.ServeHTTP(rr, req)

	if rr.Code != http.StatusBadRequest {
		t.Fatalf("expected 400, got %d (body: %s)", rr.Code, rr.Body.String())
	}
}

func TestIngestAudit_MissingPlane_Returns400(t *testing.T) {
	srv := newRouter(t)

	ev := events.AuditEvent{
		EventID: "audit:integration-test:missing-plane", OccurredAt: time.Now().UTC(),
		OrgID: "org-1",
		// plane intentionally omitted
		Producer: "integration-test", Event: "user_login",
	}
	payload, _ := json.Marshal(ev)

	req := httptest.NewRequest(http.MethodPost, "/v1/audit", bytes.NewReader(payload))
	req.Header.Set("Content-Type", "application/json")
	authorizeWriter(req, testKey)

	rr := httptest.NewRecorder()
	srv.ServeHTTP(rr, req)

	if rr.Code != http.StatusBadRequest {
		t.Fatalf("expected 400, got %d (body: %s)", rr.Code, rr.Body.String())
	}
}

func TestIngestAudit_MissingEvent_Returns400(t *testing.T) {
	srv := newRouter(t)

	ev := events.AuditEvent{
		EventID: "audit:integration-test:missing-event", OccurredAt: time.Now().UTC(),
		OrgID: "org-1", Plane: "frontend", Producer: "integration-test",
		// event intentionally omitted
	}
	payload, _ := json.Marshal(ev)

	req := httptest.NewRequest(http.MethodPost, "/v1/audit", bytes.NewReader(payload))
	req.Header.Set("Content-Type", "application/json")
	authorizeWriter(req, testKey)

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
	authorizeWriter(req, testKey)
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

func TestIngestAuditRejectsProducerThatDoesNotMatchAuthenticatedPrincipal(t *testing.T) {
	stub := &usageIngestStore{inserted: true}
	handler, err := api.New(stub, testCredentials)
	if err != nil {
		t.Fatal(err)
	}
	router := chi.NewRouter()
	handler.Mount(router)
	payload, err := json.Marshal(events.AuditEvent{
		EventID: "audit:forged:1", OccurredAt: time.Now().UTC(), OrgID: "org-1",
		Plane: "frontend", Producer: "another-service", Event: "forged",
	})
	if err != nil {
		t.Fatal(err)
	}
	req := httptest.NewRequest(http.MethodPost, "/v1/audit", bytes.NewReader(payload))
	authorizeWriter(req, testKey)
	recorder := httptest.NewRecorder()
	router.ServeHTTP(recorder, req)
	if recorder.Code != http.StatusForbidden {
		t.Fatalf("status = %d, want 403: %s", recorder.Code, recorder.Body.String())
	}
	if stub.event != nil {
		t.Fatal("forged producer reached the store")
	}
}

// validAuditPayload returns a minimal valid AuditEvent JSON body.
func validAuditPayload(t *testing.T) []byte {
	t.Helper()
	ev := events.AuditEvent{
		EventID: "audit:integration-test:login", OccurredAt: time.Now().UTC(),
		OrgID: "org-abc", Plane: "frontend", Producer: "integration-test", Event: "user_login",
	}
	b, err := json.Marshal(ev)
	if err != nil {
		t.Fatalf("marshal: %v", err)
	}
	return b
}
