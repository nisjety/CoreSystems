package learninghttp

import (
	"encoding/json"
	"net/http/httptest"
	"strings"
	"testing"
)

func TestPublicStatusDoesNotClaimReadiness(t *testing.T) {
	r := httptest.NewRecorder()
	NewHandler().ServeHTTP(r, httptest.NewRequest("GET", "/api/v1/learning/status", nil))
	var body struct {
		Data Status `json:"data"`
	}
	if err := json.Unmarshal(r.Body.Bytes(), &body); err != nil {
		t.Fatal(err)
	}
	if r.Code != 200 || body.Data.Ready || body.Data.ProcessingProfile != "strict-self-hosted" || len(body.Data.Capabilities) != 6 {
		t.Fatalf("unexpected status: %s", r.Body.String())
	}
	if r.Header().Get("Cache-Control") != "no-store" {
		t.Fatal("status must not be cached")
	}
}

func TestUnavailableOperationsCannotBeEnabledByIdentityHeaders(t *testing.T) {
	for _, path := range []string{"/readyz", "/api/v1/learning/courses", "/api/v1/learning/assistance"} {
		for _, method := range []string{"GET", "POST"} {
			t.Run(method+path, func(t *testing.T) {
				req := httptest.NewRequest(method, path, strings.NewReader(`{"organizationId":"school-a","role":"teacher"}`))
				req.Header.Set("Authorization", "Bearer forged")
				req.Header.Set("X-Organization-ID", "school-a")
				req.Header.Set("X-User-Role", "admin")
				r := httptest.NewRecorder()
				NewHandler().ServeHTTP(r, req)
				if path == "/readyz" && method == "POST" {
					if r.Code != 405 {
						t.Fatalf("got %d", r.Code)
					}
					return
				}
				if r.Code != 503 || !strings.Contains(r.Body.String(), "LEARNING_NOT_READY") {
					t.Fatalf("unexpected response: %d %s", r.Code, r.Body.String())
				}
			})
		}
	}
}

func TestHealthAndReadinessDiffer(t *testing.T) {
	for path, want := range map[string]int{"/healthz": 200, "/readyz": 503} {
		r := httptest.NewRecorder()
		NewHandler().ServeHTTP(r, httptest.NewRequest("GET", path, nil))
		if r.Code != want {
			t.Fatalf("%s got %d", path, r.Code)
		}
	}
}

func TestOnlyLoopbackCanBind(t *testing.T) {
	for addr, want := range map[string]bool{"127.0.0.1:3196": true, "[::1]:3196": true, "0.0.0.0:3196": false, ":3196": false, "school.example:3196": false, "127.0.0.1:0": false, "127.0.0.1:99999": false} {
		if got := ValidateListenAddress(addr) == nil; got != want {
			t.Fatalf("%s allowed=%v", addr, got)
		}
	}
}
