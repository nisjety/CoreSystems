package api

import (
	"context"
	"io"
	"net/http/httptest"
	"strings"
	"testing"

	"github.com/triodelab/integration-corev2/internal/auth"
	"github.com/triodelab/integration-corev2/internal/pagespeed"
)

type auditRunner struct{}

func (auditRunner) Run(context.Context, string, string) (*pagespeed.Report, string) {
	return nil, "quota_limited"
}

func TestPageSpeedAPIAuthenticationValidationAndTenantIsolation(t *testing.T) {
	jobs := pagespeed.NewService(auditRunner{}, nil)
	defer jobs.Close()
	for _, tc := range []struct {
		name, method, path, body string
		principal                auth.Principal
		bearer                   bool
		want                     int
	}{
		{"anonymous", "GET", "?site=https%3A%2F%2Faquatiq.com%2F&strategy=mobile", "", auth.Principal{}, false, 401},
		{"no-read-scope", "POST", "", `{"site":"https://aquatiq.com/","strategy":"mobile"}`, auth.Principal{UserID: "service:x", OrganizationID: "org-1", PrincipalType: "service"}, true, 403},
		{"read", "GET", "?site=https%3A%2F%2Faquatiq.com%2F&strategy=mobile", "", auth.Principal{UserID: "service:x", OrganizationID: "org-1", PrincipalType: "service", Scopes: []string{"integration:read"}}, true, 200},
		{"start", "POST", "", `{"site":"https://aquatiq.com/","strategy":"mobile"}`, auth.Principal{UserID: "user-1", OrganizationID: "org-1"}, true, 202},
		{"unknown-field", "POST", "", `{"site":"https://aquatiq.com/","strategy":"mobile","organizationId":"org-2"}`, auth.Principal{UserID: "user-1", OrganizationID: "org-1"}, true, 400},
		{"duplicate-field", "POST", "", `{"site":"https://aquatiq.com/","site":"https://evil.com/","strategy":"mobile"}`, auth.Principal{UserID: "user-1", OrganizationID: "org-1"}, true, 400},
		{"trailing-json", "POST", "", `{"site":"https://aquatiq.com/","strategy":"mobile"}{}`, auth.Principal{UserID: "user-1", OrganizationID: "org-1"}, true, 400},
		{"private", "POST", "", `{"site":"https://127.0.0.1/","strategy":"mobile"}`, auth.Principal{UserID: "user-1", OrganizationID: "org-1"}, true, 400},
		{"duplicate-query", "GET", "?site=https%3A%2F%2Faquatiq.com%2F&strategy=mobile&strategy=desktop", "", auth.Principal{UserID: "user-1", OrganizationID: "org-1"}, true, 400},
		{"unknown-query", "GET", "?site=https%3A%2F%2Faquatiq.com%2F&strategy=mobile&organizationId=org-2", "", auth.Principal{UserID: "user-1", OrganizationID: "org-1"}, true, 400},
		{"post-query", "POST", "?site=ignored", `{"site":"https://aquatiq.com/","strategy":"mobile"}`, auth.Principal{UserID: "user-1", OrganizationID: "org-1"}, true, 400},
		{"wrong-type", "POST", "", `{"site":3,"strategy":"mobile"}`, auth.Principal{UserID: "user-1", OrganizationID: "org-1"}, true, 400},
		{"missing", "POST", "", `{"site":"https://aquatiq.com/"}`, auth.Principal{UserID: "user-1", OrganizationID: "org-1"}, true, 400},
	} {
		t.Run(tc.name, func(t *testing.T) {
			cfg, repo, oauthService := testOAuthStack(t)
			app := NewServer(ServerConfig{Config: cfg, Repo: repo, OAuth: oauthService, Auth: fakeVerifier{principal: tc.principal}, PageSpeed: jobs})
			req := httptest.NewRequest(tc.method, "/api/v1/insights/pagespeed"+tc.path, strings.NewReader(tc.body))
			req.Header.Set("Content-Type", "application/json")
			if tc.bearer {
				req.Header.Set("Authorization", "Bearer synthetic")
			}
			req.Header.Set("X-Org-ID", "org-forged")
			res, err := app.Test(req)
			if err != nil {
				t.Fatal(err)
			}
			defer res.Body.Close()
			if res.StatusCode != tc.want {
				body, _ := io.ReadAll(res.Body)
				t.Fatalf("status %d want %d: %s", res.StatusCode, tc.want, body)
			}
			if tc.want < 300 && res.Header.Get("Cache-Control") != "no-store" {
				t.Fatal("missing no-store")
			}
		})
	}
	if got := jobs.Get("org-forged", "https://aquatiq.com/", "mobile"); got.Status != "idle" {
		t.Fatal("forged scoping header used")
	}
	if got := jobs.Get("org-2", "https://aquatiq.com/", "mobile"); got.Status != "idle" {
		t.Fatal("tenant isolation failed")
	}
}

func TestPageSpeedRejectsLegacyOrgHeaderAndUnavailableService(t *testing.T) {
	cfg, repo, oauthService := testOAuthStack(t)
	cfg.AllowLegacyTenantKey = true
	app := NewServer(ServerConfig{Config: cfg, Repo: repo, OAuth: oauthService, Auth: fakeVerifier{principal: auth.Principal{UserID: "user-1", OrganizationID: "org-1"}}})
	request := httptest.NewRequest("GET", "/api/v1/insights/pagespeed?site=https%3A%2F%2Faquatiq.com%2F&strategy=mobile", nil)
	request.Header.Set("X-Internal-API-Key", cfg.InternalAPIKey)
	request.Header.Set("X-Org-ID", "forged-org")
	response, err := app.Test(request)
	if err != nil {
		t.Fatal(err)
	}
	defer response.Body.Close()
	if response.StatusCode != 403 {
		t.Fatalf("legacy org header accepted: %d", response.StatusCode)
	}
	request = httptest.NewRequest("GET", "/api/v1/insights/pagespeed?site=https%3A%2F%2Faquatiq.com%2F&strategy=mobile", nil)
	request.Header.Set("Authorization", "Bearer synthetic")
	response, err = app.Test(request)
	if err != nil {
		t.Fatal(err)
	}
	defer response.Body.Close()
	if response.StatusCode != 503 {
		t.Fatalf("missing service not signalled: %d", response.StatusCode)
	}
}
