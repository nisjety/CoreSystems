package watch

import (
	"context"
	"errors"
	"net/http"
	"net/http/httptest"
	"testing"
)

// Control's service-to-service resolvers answer a typed refusal (entitlement
// off, role floor, kind unsupported) with the 403 the denial vocabulary assigns
// it, and keep 503 for "could not answer". The sweeper retries only what wraps
// ErrAuthorityUnavailable and terminates the watch on anything else, so a typed
// refusal must come out as a final error and a 503 or a dead socket as a
// retryable one.

const typedRefusalBody = `{"error":"Space watch authority unavailable","code":"space_entitlement_off","resolvable_by":"org_admin","details":{"entitlement":"watch_observe"}}`
const typedUnavailableBody = `{"error":"Space watch authority unavailable","code":"space_authority_unavailable","resolvable_by":"retry","details":{}}`

func controlStub(t *testing.T, status int, body string) *httptest.Server {
	t.Helper()
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.Header.Get("X-Service-Id") != "capability-core" || r.Header.Get("X-Service-Token") != "tok" {
			http.Error(w, "unauthenticated", http.StatusUnauthorized)
			return
		}
		w.Header().Set("Content-Type", "application/json")
		w.WriteHeader(status)
		_, _ = w.Write([]byte(body))
	}))
	t.Cleanup(server.Close)
	return server
}

func testObserveAuthorizer(t *testing.T, endpoint string) *ControlObserveAuthorizer {
	t.Helper()
	authorizer, err := NewControlObserveAuthorizer(endpoint, "capability-core", "tok", nil)
	if err != nil {
		t.Fatalf("build the authorizer: %v", err)
	}
	return authorizer
}

func TestObserveAuthorizerTreatsATypedRefusalAsFinalAndAnOutageAsRetryable(t *testing.T) {
	t.Parallel()
	watch := activeWatch(Predicate{Kind: "contains", Value: "ERROR"}, "every", 3)
	cases := []struct {
		name          string
		status        int
		body          string
		wantOK        bool
		wantRetryable bool
	}{
		{"approved", http.StatusOK, `{"data":{}}`, true, false},
		{"typed refusal", http.StatusForbidden, typedRefusalBody, false, false},
		{"plain membership refusal", http.StatusForbidden, `{"error":"current personal Space authority required"}`, false, false},
		{"unknown route or space", http.StatusNotFound, `{"error":"not found"}`, false, false},
		{"authority unavailable", http.StatusServiceUnavailable, typedUnavailableBody, false, true},
		{"plain outage", http.StatusServiceUnavailable, `{"error":"Space authority repository unavailable"}`, false, true},
		{"gateway timeout", http.StatusGatewayTimeout, `timeout`, false, true},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			t.Parallel()
			server := controlStub(t, tc.status, tc.body)
			err := testObserveAuthorizer(t, server.URL).AuthorizeObserve(context.Background(), watch)
			if tc.wantOK {
				if err != nil {
					t.Fatalf("approval returned an error: %v", err)
				}
				return
			}
			if err == nil {
				t.Fatalf("status %d was accepted", tc.status)
			}
			if got := errors.Is(err, ErrAuthorityUnavailable); got != tc.wantRetryable {
				t.Fatalf("status %d: retryable = %t, want %t (%v)", tc.status, got, tc.wantRetryable, err)
			}
		})
	}
}

func TestObserveAuthorizerTreatsATransportFailureAsRetryable(t *testing.T) {
	t.Parallel()
	server := httptest.NewServer(http.HandlerFunc(func(http.ResponseWriter, *http.Request) {}))
	endpoint := server.URL
	server.Close() // nothing listens any more
	err := testObserveAuthorizer(t, endpoint).AuthorizeObserve(context.Background(),
		activeWatch(Predicate{Kind: "contains", Value: "ERROR"}, "every", 3))
	if !errors.Is(err, ErrAuthorityUnavailable) {
		t.Fatalf("a dead socket must be retryable, got %v", err)
	}
}

func TestCreateAuthorizerTreatsATypedRefusalAsFinalAndAnOutageAsRetryable(t *testing.T) {
	t.Parallel()
	watch := activeWatch(Predicate{Kind: "contains", Value: "ERROR"}, "every", 0)
	cases := []struct {
		name          string
		status        int
		body          string
		wantRetryable bool
	}{
		{"typed refusal", http.StatusForbidden, typedRefusalBody, false},
		{"bad request", http.StatusBadRequest, `{"error":"space_ref, watch_id are required"}`, false},
		{"authority unavailable", http.StatusServiceUnavailable, typedUnavailableBody, true},
		{"plain outage", http.StatusServiceUnavailable, `{"error":"Space authority repository unavailable"}`, true},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			t.Parallel()
			server := controlStub(t, tc.status, tc.body)
			authorizer, err := NewControlCreateAuthorizer(server.URL, "capability-core", "tok", nil)
			if err != nil {
				t.Fatalf("build the authorizer: %v", err)
			}
			_, err = authorizer.AuthorizeCreate(context.Background(), watch)
			if err == nil {
				t.Fatalf("status %d was accepted", tc.status)
			}
			if got := errors.Is(err, ErrAuthorityUnavailable); got != tc.wantRetryable {
				t.Fatalf("status %d: retryable = %t, want %t (%v)", tc.status, got, tc.wantRetryable, err)
			}
		})
	}
}
