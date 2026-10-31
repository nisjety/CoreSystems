package api

import (
	"encoding/json"
	"io"
	"net/http/httptest"
	"strings"
	"testing"

	"github.com/triodelab/integration-corev2/internal/auth"
	"github.com/triodelab/integration-corev2/internal/clarity"
)

func TestClarityReadAPIRejectsForgedTenantAndUnsafeQueries(t *testing.T) {
	service, err := clarity.NewService("", nil, nil)
	if err != nil {
		t.Fatal(err)
	}
	for _, tc := range []struct {
		name, method, query string
		bearer              bool
		scopes              []string
		want                int
	}{
		{"anonymous", "GET", "site=https%3A%2F%2Faquatiq.com%2F&numOfDays=3", false, nil, 401},
		{"noscope", "GET", "site=https%3A%2F%2Faquatiq.com%2F&numOfDays=3", true, nil, 403},
		{"read", "GET", "site=https%3A%2F%2Faquatiq.com%2F&numOfDays=3", true, []string{"integration:read"}, 200},
		{"forgedorgquery", "GET", "site=https%3A%2F%2Faquatiq.com%2F&numOfDays=3&organizationId=org-2", true, []string{"integration:read"}, 400},
		{"duplicate", "GET", "site=https%3A%2F%2Faquatiq.com%2F&numOfDays=3&numOfDays=1", true, []string{"integration:read"}, 400},
		{"unknownwindow", "GET", "site=https%3A%2F%2Faquatiq.com%2F&numOfDays=30", true, []string{"integration:read"}, 400},
		{"private", "GET", "site=https%3A%2F%2F127.0.0.1%2F&numOfDays=3", true, []string{"integration:read"}, 400},
		{"projectoverride", "GET", "site=https%3A%2F%2Faquatiq.com%2F&numOfDays=3&projectId=other", true, []string{"integration:read"}, 400},
	} {
		t.Run(tc.name, func(t *testing.T) {
			cfg, repo, oauth := testOAuthStack(t)
			app := NewServer(ServerConfig{Config: cfg, Repo: repo, OAuth: oauth, Clarity: service, Auth: fakeVerifier{principal: auth.Principal{UserID: "service:marketing-core", OrganizationID: "org-1", PrincipalType: "service", Scopes: tc.scopes}}})
			req := httptest.NewRequest(tc.method, "/api/v1/insights/clarity?"+tc.query, nil)
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
					t.Fatal("missingno-store")
				}
				var envelope struct {
					Data clarity.Snapshot `json:"data"`
				}
				if err := json.NewDecoder(response.Body).Decode(&envelope); err != nil {
					t.Fatal(err)
				}
				if envelope.Data.Reason != "not_configured" || envelope.Data.Metrics != nil || envelope.Data.Scope != "clarity_project" {
					t.Fatal("unconfiguredsourcefabricateddata")
				}
			}
		})
	}
}

func TestClarityUnavailableServiceAndLegacyHeaderAreExplicit(t *testing.T) {
	cfg, repo, oauth := testOAuthStack(t)
	cfg.AllowLegacyTenantKey = true
	app := NewServer(ServerConfig{Config: cfg, Repo: repo, OAuth: oauth, Auth: fakeVerifier{principal: auth.Principal{UserID: "user-1", OrganizationID: "org-1"}}})
	for _, legacy := range []bool{false, true} {
		req := httptest.NewRequest("GET", "/api/v1/insights/clarity?site=https%3A%2F%2Faquatiq.com%2F&numOfDays=3", strings.NewReader(""))
		want := 503
		if legacy {
			req.Header.Set("X-Internal-API-Key", cfg.InternalAPIKey)
			req.Header.Set("X-Org-ID", "org-forged")
			want = 403
		} else {
			req.Header.Set("Authorization", "Bearer synthetic")
		}
		res, err := app.Test(req)
		if err != nil {
			t.Fatal(err)
		}
		res.Body.Close()
		if res.StatusCode != want {
			t.Fatalf("status=%d want=%d", res.StatusCode, want)
		}
	}
}
