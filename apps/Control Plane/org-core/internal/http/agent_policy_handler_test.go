package http

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"net/http"
	"strings"
	"sync"
	"testing"

	orgcore "github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/org"
)

const (
	agentPolicyTestSessionToken   = "sessioncore0123456789abcdef01234567"
	agentPolicyTestRetentionToken = "authcore0123456789abcdef0123456789ab"
	agentPolicyTestReadOnlyToken  = "readonly0123456789abcdef0123456789ab"
)

type fakeAgentPolicyReader struct {
	mu     sync.Mutex
	policy *orgcore.AgentLearningPolicy
	err    error
	gotID  string
	calls  int
}

func (f *fakeAgentPolicyReader) GetAgentLearningPolicy(_ context.Context, orgID string) (*orgcore.AgentLearningPolicy, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	f.calls++
	f.gotID = orgID
	return f.policy, f.err
}

func (f *fakeAgentPolicyReader) snapshot() (string, int) {
	f.mu.Lock()
	defer f.mu.Unlock()
	return f.gotID, f.calls
}

// agentPolicyTestServer builds org-core's real server (real router and real
// service-auth middleware) with a small registry and a fake policy reader.
func agentPolicyTestServer(t *testing.T, reader agentPolicyReader) *Server {
	t.Helper()
	t.Setenv(serviceCredentialEnv, fmt.Sprintf(`[
  {"principal":"session-core","audience":"org-core","token":%q,"scopes":["org:read:any","org:agent-policy:read:any"]},
  {"principal":"auth-core","audience":"org-core","token":%q,"scopes":["org:retention:read:any"]},
  {"principal":"read-only-fixture","audience":"org-core","token":%q,"scopes":["org:read:any"]}
]`, agentPolicyTestSessionToken, agentPolicyTestRetentionToken, agentPolicyTestReadOnlyToken))
	server := NewServer(0, nil, nil, "", "")
	server.agentPolicy = reader
	return server
}

func TestAgentPolicyRouteIsMounted(t *testing.T) {
	server := NewServer(0, nil, nil, "", "")
	for _, route := range server.router.Routes() {
		if route.Method == http.MethodGet && route.Path == "/internal/orgs/:orgId/agent-policy" {
			return
		}
	}
	t.Fatal("GET /internal/orgs/:orgId/agent-policy is not mounted")
}

func TestAgentPolicyHandlerReturnsOnlyThePolicy(t *testing.T) {
	policy := orgcore.AgentLearningPolicy{Memory: "disabled", LearningReview: "proposals", Revision: 3, UpdatedAt: "2026-10-24T09:00:00Z"}
	reader := &fakeAgentPolicyReader{policy: &policy}
	server := agentPolicyTestServer(t, reader)

	response := serveRetentionRequest(server, http.MethodGet, "/internal/orgs/org-1/agent-policy", "session-core", agentPolicyTestSessionToken)
	if response.Code != http.StatusOK {
		t.Fatalf("status=%d body=%s", response.Code, response.Body.String())
	}
	want := `{"memory":"disabled","learningReview":"proposals","revision":3,"updatedAt":"2026-10-24T09:00:00Z"}`
	if strings.TrimSpace(response.Body.String()) != want {
		t.Fatalf("body=%s want=%s", response.Body.String(), want)
	}
	if got := response.Header().Get("Cache-Control"); got != "no-store" {
		t.Fatalf("Cache-Control=%q want no-store", got)
	}
	if gotID, calls := reader.snapshot(); gotID != "org-1" || calls != 1 {
		t.Fatalf("reader saw org=%q calls=%d", gotID, calls)
	}
}

func TestAgentPolicyHandlerErrors(t *testing.T) {
	t.Run("unknown organization is a definitive 404", func(t *testing.T) {
		server := agentPolicyTestServer(t, &fakeAgentPolicyReader{err: fmt.Errorf("lookup: %w", orgcore.ErrNotFound)})
		response := serveRetentionRequest(server, http.MethodGet, "/internal/orgs/org-missing/agent-policy", "session-core", agentPolicyTestSessionToken)
		if response.Code != http.StatusNotFound {
			t.Fatalf("status=%d body=%s", response.Code, response.Body.String())
		}
	})
	t.Run("a failed lookup is a 500 that leaks nothing and never looks like a policy", func(t *testing.T) {
		server := agentPolicyTestServer(t, &fakeAgentPolicyReader{err: errors.New("pq: password authentication failed for user coresystem")})
		response := serveRetentionRequest(server, http.MethodGet, "/internal/orgs/org-1/agent-policy", "session-core", agentPolicyTestSessionToken)
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
		if _, hasPolicy := payload["memory"]; hasPolicy {
			t.Fatalf("an error response must never look like a policy: %s", response.Body.String())
		}
	})
	t.Run("no reader configured is a 503, never a default policy", func(t *testing.T) {
		server := agentPolicyTestServer(t, nil)
		response := serveRetentionRequest(server, http.MethodGet, "/internal/orgs/org-1/agent-policy", "session-core", agentPolicyTestSessionToken)
		if response.Code != http.StatusServiceUnavailable {
			t.Fatalf("status=%d body=%s", response.Code, response.Body.String())
		}
	})
}

func TestAgentPolicyRouteRequiresItsOwnScope(t *testing.T) {
	reader := &fakeAgentPolicyReader{policy: &orgcore.AgentLearningPolicy{Memory: "allow", LearningReview: "disabled"}}
	server := agentPolicyTestServer(t, reader)
	path := "/internal/orgs/org-1/agent-policy"

	if response := serveRetentionRequest(server, http.MethodGet, path, "", ""); response.Code != http.StatusUnauthorized {
		t.Fatalf("no credential status=%d", response.Code)
	}
	if response := serveRetentionRequest(server, http.MethodGet, path, "session-core", "wrong-token"); response.Code != http.StatusUnauthorized {
		t.Fatalf("wrong token status=%d", response.Code)
	}
	// org:read:any reads the organization record and the retention scope reads
	// the posture; neither is a grant on the policy.
	if response := serveRetentionRequest(server, http.MethodGet, path, "read-only-fixture", agentPolicyTestReadOnlyToken); response.Code != http.StatusForbidden {
		t.Fatalf("org:read:any holder status=%d want=%d", response.Code, http.StatusForbidden)
	}
	if response := serveRetentionRequest(server, http.MethodGet, path, "auth-core", agentPolicyTestRetentionToken); response.Code != http.StatusForbidden {
		t.Fatalf("retention-scope holder status=%d want=%d", response.Code, http.StatusForbidden)
	}
	if response := serveRetentionRequest(server, http.MethodPost, path, "session-core", agentPolicyTestSessionToken); response.Code != http.StatusForbidden {
		t.Fatalf("POST status=%d want=%d", response.Code, http.StatusForbidden)
	}
	if _, calls := reader.snapshot(); calls != 0 {
		t.Fatalf("an unauthorized request reached the reader %d times", calls)
	}
}

// Pins the production wiring: without it every poll would answer 503 and
// session-core, failing closed, would stop remembering for every org.
func TestNewServerInstallsTheOrganizationServiceAsTheAgentPolicyReader(t *testing.T) {
	service := orgcore.NewService(&orgcore.Repository{}, nil)
	server := NewServer(0, service, nil, "", "")
	installed, ok := server.agentPolicy.(*orgcore.Service)
	if !ok || installed != service {
		t.Fatalf("NewServer did not install the organization service as the agent policy reader: %T", server.agentPolicy)
	}
	if withoutService := NewServer(0, nil, nil, "", ""); withoutService.agentPolicy != nil {
		t.Fatalf("a nil organization service must leave the reader unset (not a typed nil): %T", withoutService.agentPolicy)
	}
}
