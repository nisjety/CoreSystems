package api

import (
	"encoding/json"
	"io"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/triodelab/integration-corev2/internal/auth"
	"github.com/triodelab/integration-corev2/internal/clarity"
)

type clarityBindingTransport func(*http.Request) (*http.Response, error)

func (f clarityBindingTransport) RoundTrip(r *http.Request) (*http.Response, error) { return f(r) }

func TestClarityBindingMetadataAuthenticatesScopesAndRejectsForgedSelectors(t *testing.T) {
	service, err := clarity.NewService("", &http.Client{Transport: clarityBindingTransport(func(*http.Request) (*http.Response, error) {
		t.Fatal("metadata called export provider")
		return nil, nil
	})}, nil)
	if err != nil {
		t.Fatal(err)
	}
	for _, tc := range []struct {
		name, method, query, body string
		bearer                    bool
		scopes                    []string
		want                      int
	}{
		{"anonymous", "GET", "site=https%3A%2F%2Faquatiq.com%2F", "", false, nil, 401},
		{"noscope", "GET", "site=https%3A%2F%2Faquatiq.com%2F", "", true, nil, 403},
		{"read", "GET", "site=https%3A%2F%2Faquatiq.com%2F", "", true, []string{"integration:read"}, 200},
		{"forgedorg", "GET", "site=https%3A%2F%2Faquatiq.com%2F&organizationId=org-2", "", true, []string{"integration:read"}, 400},
		{"duplicate", "GET", "site=https%3A%2F%2Faquatiq.com%2F&site=https%3A%2F%2Faquatiq.com%2F", "", true, []string{"integration:read"}, 400},
		{"private", "GET", "site=https%3A%2F%2F127.0.0.1%2F", "", true, []string{"integration:read"}, 400},
		{"path", "GET", "site=https%3A%2F%2Faquatiq.com%2Fprivate", "", true, []string{"integration:read"}, 400},
		{"projectoverride", "GET", "site=https%3A%2F%2Faquatiq.com%2F&projectId=other", "", true, []string{"integration:read"}, 400},
		{"body", "GET", "site=https%3A%2F%2Faquatiq.com%2F", "{}", true, []string{"integration:read"}, 400},
		{"post", "POST", "site=https%3A%2F%2Faquatiq.com%2F", "", true, []string{"integration:read"}, 405},
	} {
		t.Run(tc.name, func(t *testing.T) {
			cfg, repo, oauth := testOAuthStack(t)
			app := NewServer(ServerConfig{Config: cfg, Repo: repo, OAuth: oauth, Clarity: service, Auth: fakeVerifier{principal: auth.Principal{UserID: "service:marketing-core", OrganizationID: "org-1", PrincipalType: "service", Scopes: tc.scopes}}})
			req := httptest.NewRequest(tc.method, "/api/v1/insights/clarity/binding?"+tc.query, strings.NewReader(tc.body))
			if tc.bearer {
				req.Header.Set("Authorization", "Bearer synthetic")
			}
			req.Header.Set("X-Org-ID", "org-forged")
			response, err := app.Test(req)
			if err != nil {
				t.Fatal(err)
			}
			defer response.Body.Close()
			if response.StatusCode != tc.want {
				body, _ := io.ReadAll(response.Body)
				t.Fatalf("status=%d want=%d body=%s", response.StatusCode, tc.want, body)
			}
			if tc.want == 200 {
				if response.Header.Get("Cache-Control") != "no-store" {
					t.Fatal("missing no-store")
				}
				var envelope struct {
					Success bool                  `json:"success"`
					Data    clarity.BindingAccess `json:"data"`
				}
				if err := json.NewDecoder(response.Body).Decode(&envelope); err != nil {
					t.Fatal(err)
				}
				if !envelope.Success || envelope.Data.Configured || envelope.Data.Site != "https://aquatiq.com/" || envelope.Data.ProjectID != "" || envelope.Data.Version != "" {
					t.Fatal("fabricated metadata")
				}
			}
		})
	}
}

func TestClarityBindingMetadataFreshManifestAndGenericFailure(t *testing.T) {
	dir := t.TempDir()
	path := filepath.Join(dir, "bindings.json")
	token := filepath.Join(dir, "clarity.token")
	if err := os.WriteFile(token, []byte("synthetic.jwt.signature"), 0600); err != nil {
		t.Fatal(err)
	}
	manifest := `{"version":1,"bindings":[{"organizationId":"org-1","site":"https://aquatiq.com/","projectId":"project-1","tokenFile":"clarity.token","confirmedAt":"2026-10-21T00:00:00Z"}]}`
	if err := os.WriteFile(path, []byte(manifest), 0600); err != nil {
		t.Fatal(err)
	}
	service, err := clarity.NewService(path, &http.Client{Transport: clarityBindingTransport(func(*http.Request) (*http.Response, error) {
		t.Fatal("configured metadata must not export")
		return nil, nil
	})}, nil)
	if err != nil {
		t.Fatal(err)
	}
	cfg, repo, oauth := testOAuthStack(t)
	app := NewServer(ServerConfig{Config: cfg, Repo: repo, OAuth: oauth, Clarity: service, Auth: fakeVerifier{principal: auth.Principal{UserID: "service:marketing-core", OrganizationID: "org-1", PrincipalType: "service", Scopes: []string{"integration:read"}}}})
	request := func(want int) string {
		t.Helper()
		req := httptest.NewRequest("GET", "/api/v1/insights/clarity/binding?site=https%3A%2F%2Faquatiq.com%2F", nil)
		req.Header.Set("Authorization", "Bearer synthetic")
		res, err := app.Test(req)
		if err != nil {
			t.Fatal(err)
		}
		defer res.Body.Close()
		body, _ := io.ReadAll(res.Body)
		if res.StatusCode != want {
			t.Fatalf("status=%d body=%s", res.StatusCode, body)
		}
		if strings.Contains(string(body), dir) || strings.Contains(string(body), "synthetic.jwt") || strings.Contains(string(body), "tokenFile") {
			t.Fatal("secret configuration leaked")
		}
		return string(body)
	}
	if body := request(200); !strings.Contains(body, `"configured":true`) || !strings.Contains(body, `"projectId":"project-1"`) {
		t.Fatal("current binding absent")
	}
	if err := os.Remove(token); err != nil {
		t.Fatal(err)
	}
	if body := request(503); !strings.Contains(body, "clarity_unavailable") {
		t.Fatal("missing generic error")
	}
	if err := os.WriteFile(path, []byte(`{"version":1,"bindings":[]}`), 0600); err != nil {
		t.Fatal(err)
	}
	if body := request(200); !strings.Contains(body, `"configured":false`) {
		t.Fatal("revoked binding still configured")
	}
}

func TestClarityBindingMetadataIsRateLimitedAndLegacyKeyCannotChooseOrg(t *testing.T) {
	cfg, repo, oauth := testOAuthStack(t)
	cfg.RateLimitEnabled, cfg.RateLimitMax, cfg.RateLimitWindow = true, 1, time.Minute
	cfg.AllowLegacyTenantKey = true
	service, _ := clarity.NewService("", nil, nil)
	app := NewServer(ServerConfig{Config: cfg, Repo: repo, OAuth: oauth, Clarity: service, Auth: fakeVerifier{principal: auth.Principal{UserID: "service:marketing-core", OrganizationID: "org-1", PrincipalType: "service", Scopes: []string{"integration:read"}}}})
	for index, want := range []int{200, 429} {
		req := httptest.NewRequest("GET", "/api/v1/insights/clarity/binding?site=https%3A%2F%2Faquatiq.com%2F", nil)
		req.Header.Set("Authorization", "Bearer synthetic")
		res, err := app.Test(req)
		if err != nil {
			t.Fatal(err)
		}
		res.Body.Close()
		if res.StatusCode != want {
			t.Fatalf("request %d status=%d want=%d", index, res.StatusCode, want)
		}
	}
	cfg.RateLimitEnabled = false
	app = NewServer(ServerConfig{Config: cfg, Repo: repo, OAuth: oauth, Clarity: service})
	req := httptest.NewRequest("GET", "/api/v1/insights/clarity/binding?site=https%3A%2F%2Faquatiq.com%2F", nil)
	req.Header.Set("X-Internal-API-Key", cfg.InternalAPIKey)
	req.Header.Set("X-Org-ID", "org-forged")
	res, err := app.Test(req)
	if err != nil {
		t.Fatal(err)
	}
	res.Body.Close()
	if res.StatusCode != 403 {
		t.Fatal("legacy key supplied tenant authority")
	}
}
