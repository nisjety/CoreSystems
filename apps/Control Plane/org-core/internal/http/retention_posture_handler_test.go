package http

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"net/http"
	"net/http/httptest"
	"strings"
	"sync"
	"testing"

	orgcore "github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/org"
)

const (
	retentionTestAuthToken     = "authcore0123456789abcdef0123456789ab"
	retentionTestReadOnlyToken = "readonly0123456789abcdef0123456789ab"
)

type fakeRetentionPostureReader struct {
	mu      sync.Mutex
	posture *orgcore.RetentionPosture
	err     error
	gotID   string
	calls   int
}

func (f *fakeRetentionPostureReader) GetRetentionPosture(_ context.Context, orgID string) (*orgcore.RetentionPosture, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	f.calls++
	f.gotID = orgID
	return f.posture, f.err
}

func (f *fakeRetentionPostureReader) snapshot() (string, int) {
	f.mu.Lock()
	defer f.mu.Unlock()
	return f.gotID, f.calls
}

// retentionTestServer builds org-core's real server (real router and real
// service-auth middleware) with a small registry and a fake posture reader.
func retentionTestServer(t *testing.T, reader retentionPostureReader) *Server {
	t.Helper()
	t.Setenv(serviceCredentialEnv, fmt.Sprintf(`[
  {"principal":"auth-core","audience":"org-core","token":%q,"scopes":["org:retention:read:any"]},
  {"principal":"read-only-fixture","audience":"org-core","token":%q,"scopes":["org:read:any"]}
]`, retentionTestAuthToken, retentionTestReadOnlyToken))
	server := NewServer(0, nil, nil, "", "")
	server.retentionPosture = reader
	return server
}

func serveRetentionRequest(server *Server, method, path, principal, token string) *httptest.ResponseRecorder {
	request := httptest.NewRequest(method, path, nil)
	if principal != "" {
		request.Header.Set("X-Service-Id", principal)
	}
	if token != "" {
		request.Header.Set("X-Service-Token", token)
	}
	response := httptest.NewRecorder()
	server.router.ServeHTTP(response, request)
	return response
}

func TestRetentionPostureRouteIsMounted(t *testing.T) {
	server := NewServer(0, nil, nil, "", "")
	for _, route := range server.router.Routes() {
		if route.Method == http.MethodGet && route.Path == "/internal/orgs/:orgId/retention-posture" {
			return
		}
	}
	t.Fatal("GET /internal/orgs/:orgId/retention-posture is not mounted")
}

func TestRetentionPostureHandlerReturnsOnlyTheTwoBooleans(t *testing.T) {
	cases := []struct {
		name    string
		posture orgcore.RetentionPosture
		want    string
	}{
		{"zdr org", orgcore.RetentionPosture{ZDRIntent: true, Entitled: true}, `{"zdrIntent":true,"entitled":true}`},
		{"plain org", orgcore.RetentionPosture{ZDRIntent: false, Entitled: true}, `{"zdrIntent":false,"entitled":true}`},
		{"plan lapsed", orgcore.RetentionPosture{ZDRIntent: true, Entitled: false}, `{"zdrIntent":true,"entitled":false}`},
	}
	for _, test := range cases {
		t.Run(test.name, func(t *testing.T) {
			posture := test.posture
			reader := &fakeRetentionPostureReader{posture: &posture}
			server := retentionTestServer(t, reader)

			response := serveRetentionRequest(server, http.MethodGet, "/internal/orgs/org-zdr-1/retention-posture", "auth-core", retentionTestAuthToken)
			if response.Code != http.StatusOK {
				t.Fatalf("status=%d body=%s", response.Code, response.Body.String())
			}
			if strings.TrimSpace(response.Body.String()) != test.want {
				t.Fatalf("body=%s want=%s", response.Body.String(), test.want)
			}
			if got := response.Header().Get("Cache-Control"); got != "no-store" {
				t.Fatalf("Cache-Control=%q want no-store", got)
			}
			if gotID, calls := reader.snapshot(); gotID != "org-zdr-1" || calls != 1 {
				t.Fatalf("reader saw org=%q calls=%d", gotID, calls)
			}
		})
	}
}

func TestRetentionPostureHandlerErrors(t *testing.T) {
	t.Run("unknown organization is a definitive 404", func(t *testing.T) {
		server := retentionTestServer(t, &fakeRetentionPostureReader{err: fmt.Errorf("lookup: %w", orgcore.ErrNotFound)})
		response := serveRetentionRequest(server, http.MethodGet, "/internal/orgs/org-missing/retention-posture", "auth-core", retentionTestAuthToken)
		if response.Code != http.StatusNotFound {
			t.Fatalf("status=%d body=%s", response.Code, response.Body.String())
		}
	})
	t.Run("a failed lookup is a 500 that leaks nothing", func(t *testing.T) {
		server := retentionTestServer(t, &fakeRetentionPostureReader{err: errors.New("pq: password authentication failed for user coresystem")})
		response := serveRetentionRequest(server, http.MethodGet, "/internal/orgs/org-1/retention-posture", "auth-core", retentionTestAuthToken)
		if response.Code != http.StatusInternalServerError {
			t.Fatalf("status=%d body=%s", response.Code, response.Body.String())
		}
		if strings.Contains(response.Body.String(), "password") || strings.Contains(response.Body.String(), "coresystem") {
			t.Fatalf("error detail leaked: %s", response.Body.String())
		}
		var payload map[string]any
		if err := json.Unmarshal(response.Body.Bytes(), &payload); err != nil {
			t.Fatalf("error body is not JSON: %v", err)
		}
		if _, hasPosture := payload["zdrIntent"]; hasPosture {
			t.Fatalf("an error response must never look like a posture: %s", response.Body.String())
		}
	})
	t.Run("no reader configured is a 503, never a default posture", func(t *testing.T) {
		server := retentionTestServer(t, nil)
		response := serveRetentionRequest(server, http.MethodGet, "/internal/orgs/org-1/retention-posture", "auth-core", retentionTestAuthToken)
		if response.Code != http.StatusServiceUnavailable {
			t.Fatalf("status=%d body=%s", response.Code, response.Body.String())
		}
	})
}

func TestRetentionPostureRouteRequiresItsOwnScope(t *testing.T) {
	reader := &fakeRetentionPostureReader{posture: &orgcore.RetentionPosture{ZDRIntent: true, Entitled: true}}
	server := retentionTestServer(t, reader)
	path := "/internal/orgs/org-1/retention-posture"

	if response := serveRetentionRequest(server, http.MethodGet, path, "", ""); response.Code != http.StatusUnauthorized {
		t.Fatalf("no credential status=%d", response.Code)
	}
	if response := serveRetentionRequest(server, http.MethodGet, path, "auth-core", "wrong-token"); response.Code != http.StatusUnauthorized {
		t.Fatalf("wrong token status=%d", response.Code)
	}
	// org:read:any reads the whole organization record; it is not a posture
	// grant, and the posture scope is not a read-the-organization grant.
	if response := serveRetentionRequest(server, http.MethodGet, path, "read-only-fixture", retentionTestReadOnlyToken); response.Code != http.StatusForbidden {
		t.Fatalf("org:read:any holder status=%d want=%d", response.Code, http.StatusForbidden)
	}
	if response := serveRetentionRequest(server, http.MethodGet, "/internal/orgs/org-1", "auth-core", retentionTestAuthToken); response.Code != http.StatusForbidden {
		t.Fatalf("posture-scope holder reading the org record status=%d want=%d", response.Code, http.StatusForbidden)
	}
	if response := serveRetentionRequest(server, http.MethodPost, path, "auth-core", retentionTestAuthToken); response.Code != http.StatusForbidden {
		t.Fatalf("POST status=%d want=%d", response.Code, http.StatusForbidden)
	}
	if _, calls := reader.snapshot(); calls != 0 {
		t.Fatalf("an unauthorized request reached the reader %d times", calls)
	}
}

// Every test above installs the posture reader by hand, so none of them would
// notice if NewServer stopped doing it: production would then answer 503 to
// every posture read (Auth Core: no token for any org it has not cached). This
// pins the production wiring, including the nil-service case, where assigning a
// typed-nil *Service would defeat the handler's nil check and panic instead of
// answering 503.
func TestNewServerInstallsTheOrganizationServiceAsTheRetentionPostureReader(t *testing.T) {
	service := orgcore.NewService(&orgcore.Repository{}, nil)
	server := NewServer(0, service, nil, "", "")
	installed, ok := server.retentionPosture.(*orgcore.Service)
	if !ok || installed != service {
		t.Fatalf("NewServer did not install the organization service as the retention posture reader: %T", server.retentionPosture)
	}

	if withoutService := NewServer(0, nil, nil, "", ""); withoutService.retentionPosture != nil {
		t.Fatalf("a nil organization service must leave the reader unset (not a typed nil): %T", withoutService.retentionPosture)
	}
}
