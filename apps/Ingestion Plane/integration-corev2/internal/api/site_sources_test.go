package api

import (
	"context"
	"errors"
	"io"
	"net/http/httptest"
	"strings"
	"testing"
	"time"

	"github.com/triodelab/integration-corev2/internal/auth"
	"github.com/triodelab/integration-corev2/internal/sitesources"
)

const sourceBody = `{"websiteId":"web_12345678901234567890123456789012","site":"https://triodelab.no/","websiteRevision":7,"provider":"cookiebot"}`

type sourceFixture struct {
	calls   int
	failure error
}

func (f *sourceFixture) Collect(_ context.Context, org string, in sitesources.Input) (sitesources.Report, error) {
	f.calls++
	if org != "org-1" {
		return sitesources.Report{}, errors.New("forged organization")
	}
	return sitesources.Report{WebsiteID: in.WebsiteID, Site: in.Site, WebsiteRevision: in.WebsiteRevision, Provider: in.Provider, ObservedAt: "2026-10-23T12:00:00Z", Window: sitesources.Window{StartDate: "2026-10-16", EndDate: "2026-10-22"}, Status: "not_configured"}, f.failure
}
func sourcePrincipal() auth.Principal {
	return auth.Principal{UserID: "service:marketing", PrincipalType: "service", OrganizationID: "org-1", Scopes: []string{"integration:read"}}
}
func TestSiteSourcesAPIHasStrictReadScopeAndEnvelope(t *testing.T) {
	for _, tc := range []struct {
		name, body, query, method string
		principal                 auth.Principal
		bearer, internal          bool
		want                      int
	}{
		{"anonymous", sourceBody, "", "POST", auth.Principal{}, false, false, 401},
		{"read", sourceBody, "", "POST", sourcePrincipal(), true, false, 200},
		{"human-no-scope", sourceBody, "", "POST", auth.Principal{UserID: "u1", OrganizationID: "org-1"}, true, false, 403},
		{"internal", sourceBody, "", "POST", auth.Principal{}, false, true, 403},
		{"unknown", strings.Replace(sourceBody, `"provider":"cookiebot"`, `"provider":"cookiebot","apiKey":"secret"`, 1), "", "POST", sourcePrincipal(), true, false, 400},
		{"case", strings.Replace(sourceBody, `"websiteId"`, `"WebsiteID"`, 1), "", "POST", sourcePrincipal(), true, false, 400},
		{"duplicate", strings.Replace(sourceBody, `"provider":"cookiebot"`, `"provider":"cookiebot","provider":"algolia"`, 1), "", "POST", sourcePrincipal(), true, false, 400},
		{"query", sourceBody, "?org=forged", "POST", sourcePrincipal(), true, false, 400},
		{"oversize", sourceBody + strings.Repeat(" ", 2048), "", "POST", sourcePrincipal(), true, false, 400},
		{"method", "", "", "GET", sourcePrincipal(), true, false, 405},
	} {
		t.Run(tc.name, func(t *testing.T) {
			cfg, repo, oauth := testOAuthStack(t)
			reader := &sourceFixture{}
			app := NewServer(ServerConfig{Config: cfg, Repo: repo, OAuth: oauth, Auth: fakeVerifier{principal: tc.principal}, SiteSources: reader})
			req := httptest.NewRequest(tc.method, "/api/v1/insights/site-sources"+tc.query, strings.NewReader(tc.body))
			req.Header.Set("Content-Type", "application/json")
			req.Header.Set("X-Org-ID", "forged")
			if tc.bearer {
				req.Header.Set("Authorization", "Bearer synthetic")
			}
			if tc.internal {
				req.Header.Set("X-Internal-API-Key", cfg.InternalAPIKey)
			}
			res, err := app.Test(req)
			if err != nil {
				t.Fatal(err)
			}
			defer res.Body.Close()
			raw, _ := io.ReadAll(res.Body)
			if res.StatusCode != tc.want {
				t.Fatalf("status%d want%d %s", res.StatusCode, tc.want, raw)
			}
			if tc.want == 200 {
				if reader.calls != 1 || strings.Contains(string(raw), `"meta"`) || !strings.Contains(string(raw), `"success":true`) || res.Header.Get("Cache-Control") != "no-store" {
					t.Fatal("unsafe envelope")
				}
			} else if reader.calls != 0 {
				t.Fatal("rejected request read upstream")
			}
		})
	}
}
func TestSiteSourcesAPIAdmissionAndGenericFailures(t *testing.T) {
	for _, tc := range []struct {
		failure error
		want    int
	}{{sitesources.ErrBusy, 429}, {errors.New("secret-key-in-URL"), 503}} {
		cfg, repo, oauth := testOAuthStack(t)
		app := NewServer(ServerConfig{Config: cfg, Repo: repo, OAuth: oauth, Auth: fakeVerifier{principal: sourcePrincipal()}, SiteSources: &sourceFixture{failure: tc.failure}})
		req := httptest.NewRequest("POST", "/api/v1/insights/site-sources", strings.NewReader(sourceBody))
		req.Header.Set("Authorization", "Bearer synthetic")
		req.Header.Set("Content-Type", "application/json")
		res, err := app.Test(req)
		if err != nil {
			t.Fatal(err)
		}
		defer res.Body.Close()
		raw, _ := io.ReadAll(res.Body)
		if res.StatusCode != tc.want || strings.Contains(string(raw), "secret-key") {
			t.Fatal("failure leaked")
		}
		if tc.want == 429 && res.Header.Get("Retry-After") != "60" {
			t.Fatal("retry")
		}
	}
	cfg, repo, oauth := testOAuthStack(t)
	cfg.RateLimitEnabled = true
	cfg.RateLimitMax = 1
	cfg.RateLimitWindow = time.Minute
	reader := &sourceFixture{}
	app := NewServer(ServerConfig{Config: cfg, Repo: repo, OAuth: oauth, Auth: fakeVerifier{principal: sourcePrincipal()}, SiteSources: reader})
	for _, want := range []int{200, 429} {
		req := httptest.NewRequest("POST", "/api/v1/insights/site-sources", strings.NewReader(sourceBody))
		req.Header.Set("Authorization", "Bearer synthetic")
		req.Header.Set("Content-Type", "application/json")
		res, err := app.Test(req)
		if err != nil {
			t.Fatal(err)
		}
		res.Body.Close()
		if res.StatusCode != want {
			t.Fatal("rate bypass")
		}
	}
}
