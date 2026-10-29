package capabilityhealth

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strings"
	"sync"
	"testing"
	"time"
)

func TestNewRejectsPartialConfiguration(t *testing.T) {
	if _, err := New(Config{CapabilityCoreURL: "http://capability-core:8085"}); err == nil {
		t.Fatal("New() accepted a partial reporter configuration")
	}
}

func TestAttestMintsNarrowTokenReadsVersionAndPostsContentFreeOwnerHealth(t *testing.T) {
	var mu sync.Mutex
	var paths []string
	var attestationBody map[string]any
	var authHeaders http.Header

	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		mu.Lock()
		paths = append(paths, r.Method+" "+r.URL.Path)
		mu.Unlock()
		switch r.URL.Path {
		case "/api/capability-core/internal-token":
			if got := r.Header.Get("x-service-id"); got != "conversation-core" {
				t.Fatalf("service id = %q", got)
			}
			if got := r.Header.Get("x-service-api-key"); got != "dev-only-credential" {
				t.Fatalf("service credential = %q", got)
			}
			var request tokenRequest
			if err := json.NewDecoder(r.Body).Decode(&request); err != nil {
				t.Fatalf("decode token request: %v", err)
			}
			if request.OrgID != "global" || request.Reason == "" || len(request.Scopes) != 2 || request.Scopes[0] != readScope || request.Scopes[1] != ownerHealthScope {
				t.Fatalf("token request = %#v", request)
			}
			_, _ = w.Write([]byte(`{"token":"short-lived-capability-token","audience":"capability-core","expiresInSeconds":300}`))
		case "/api/v1/capabilities/cap.tool.ticket.create":
			if got := r.Header.Get("Authorization"); got != "Bearer short-lived-capability-token" {
				t.Fatalf("row authorization = %q", got)
			}
			_, _ = w.Write([]byte(`{"id":"cap.tool.ticket.create","version":"1.0.0"}`))
		case "/api/v1/capabilities/owner-actions/health":
			if got := r.Header.Get("Authorization"); got != "Bearer short-lived-capability-token" {
				t.Fatalf("attestation authorization = %q", got)
			}
			authHeaders = r.Header.Clone()
			if err := json.NewDecoder(r.Body).Decode(&attestationBody); err != nil {
				t.Fatalf("decode attestation: %v", err)
			}
			w.WriteHeader(http.StatusOK)
		default:
			http.NotFound(w, r)
		}
	}))
	defer server.Close()

	reporter, err := New(Config{
		CapabilityCoreURL: server.URL,
		AuthCoreURL:       server.URL,
		ServiceID:         "conversation-core",
		Credential:        "dev-only-credential",
		HTTPClient:        server.Client(),
	})
	if err != nil {
		t.Fatalf("New() error = %v", err)
	}
	if err := reporter.Attest(context.Background()); err != nil {
		t.Fatalf("Attest() error = %v", err)
	}
	if got, want := strings.Join(paths, ","), "POST /api/capability-core/internal-token,GET /api/v1/capabilities/cap.tool.ticket.create,POST /api/v1/capabilities/owner-actions/health"; got != want {
		t.Fatalf("request path order = %q, want %q", got, want)
	}
	want := map[string]any{
		"id":             "cap.tool.ticket.create",
		"version":        "1.0.0",
		"state":          "available",
		"reason_code":    "runtime_healthy",
		"reason":         "conversation-core owner-action adapter and Control-bound execution path ready",
		"execution_mode": "agentic",
		"cost_class":     "bounded",
	}
	if len(attestationBody) != len(want) {
		t.Fatalf("attestation body = %#v, want exact content-free fields", attestationBody)
	}
	for key, expected := range want {
		if attestationBody[key] != expected {
			t.Errorf("attestation[%q] = %#v, want %#v", key, attestationBody[key], expected)
		}
	}
	if authHeaders.Get("x-service-api-key") != "" {
		t.Fatal("service credential leaked to Capability Core")
	}
}

func TestAttestFailsBeforeCapabilityWriteWhenAuthCoreRefusesToken(t *testing.T) {
	var capabilityWrites int
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path == "/api/capability-core/internal-token" {
			http.Error(w, "denied", http.StatusForbidden)
			return
		}
		if r.Method == http.MethodPost && strings.Contains(r.URL.Path, "/owner-actions/") {
			capabilityWrites++
		}
		http.NotFound(w, r)
	}))
	defer server.Close()
	reporter, err := New(Config{
		CapabilityCoreURL: server.URL,
		AuthCoreURL:       server.URL,
		ServiceID:         "conversation-core",
		Credential:        "dev-only-credential",
		Interval:          time.Second,
		HTTPClient:        server.Client(),
	})
	if err != nil {
		t.Fatalf("New() error = %v", err)
	}
	if err := reporter.Attest(context.Background()); err == nil {
		t.Fatal("Attest() succeeded after Auth Core denied the token")
	}
	if capabilityWrites != 0 {
		t.Fatalf("Capability Core received %d writes after token refusal", capabilityWrites)
	}
}
