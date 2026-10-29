package api

import (
	"context"
	"encoding/json"
	"io"
	"net/http/httptest"
	"strings"
	"testing"
	"time"

	"github.com/triodelab/integration-corev2/internal/auth"
	"github.com/triodelab/integration-corev2/internal/publicsignals"
)

type publicSignalsFixture struct {
	calls   int
	failure error
}

func (f *publicSignalsFixture) Collect(_ context.Context, site string) (publicsignals.Report, error) {
	f.calls++
	if f.failure != nil {
		return publicsignals.Report{}, f.failure
	}
	return publicsignals.Report{Site: site, Source: "public_web_signals", ObservedAt: "2026-10-23T12:00:00Z", CrUX: []publicsignals.Device{{FormFactor: "PHONE", Status: "unavailable", Reason: "not_configured"}, {FormFactor: "DESKTOP", Status: "unavailable", Reason: "not_configured"}}, CommonCrawl: publicsignals.CommonCrawl{Status: "unavailable", Reason: "no_data", Captures: []publicsignals.Capture{}, Limit: 20}}, nil
}
func TestPublicSignalsAPIHasClosedReadAuthorityAndInputBoundary(t *testing.T) {
	for _, tc := range []struct {
		name, method, query, body, media string
		bearer                           bool
		principal                        auth.Principal
		want                             int
	}{
		{"anonymous", "POST", "", `{"site":"https://triodelab.no/"}`, "application/json", false, auth.Principal{}, 401},
		{"noscope", "POST", "", `{"site":"https://triodelab.no/"}`, "application/json", true, auth.Principal{UserID: "service:x", OrganizationID: "org-1", PrincipalType: "service"}, 403},
		{"noorg", "POST", "", `{"site":"https://triodelab.no/"}`, "application/json", true, auth.Principal{UserID: "user-1"}, 503},
		{"read", "POST", "", `{"site":"https://triodelab.no/"}`, "application/json", true, auth.Principal{UserID: "service:x", OrganizationID: "org-1", PrincipalType: "service", Scopes: []string{"integration:read"}}, 200},
		{"human", "POST", "", `{"site":"https://triodelab.no/"}`, "application/json", true, auth.Principal{UserID: "user-1", OrganizationID: "org-1"}, 200},
		{"unknown", "POST", "", `{"site":"https://triodelab.no/","organizationId":"other"}`, "application/json", true, auth.Principal{UserID: "user-1", OrganizationID: "org-1"}, 400},
		{"duplicate", "POST", "", `{"site":"https://triodelab.no/","site":"https://other.no/"}`, "application/json", true, auth.Principal{UserID: "user-1", OrganizationID: "org-1"}, 400},
		{"credential", "POST", "", `{"site":"https://triodelab.no/","apiKey":"synthetic"}`, "application/json", true, auth.Principal{UserID: "user-1", OrganizationID: "org-1"}, 400},
		{"query", "POST", "?site=https%3A%2F%2Ftriodelab.no%2F", `{"site":"https://triodelab.no/"}`, "application/json", true, auth.Principal{UserID: "user-1", OrganizationID: "org-1"}, 400},
		{"private", "POST", "", `{"site":"https://127.0.0.1/"}`, "application/json", true, auth.Principal{UserID: "user-1", OrganizationID: "org-1"}, 400},
		{"missing", "POST", "", `{}`, "application/json", true, auth.Principal{UserID: "user-1", OrganizationID: "org-1"}, 400},
		{"nonstring", "POST", "", `{"site":3}`, "application/json", true, auth.Principal{UserID: "user-1", OrganizationID: "org-1"}, 400},
		{"trailing", "POST", "", `{"site":"https://triodelab.no/"}{}`, "application/json", true, auth.Principal{UserID: "user-1", OrganizationID: "org-1"}, 400},
		{"media", "POST", "", `{"site":"https://triodelab.no/"}`, "text/plain", true, auth.Principal{UserID: "user-1", OrganizationID: "org-1"}, 400},
		{"get", "GET", "", "", "application/json", true, auth.Principal{UserID: "user-1", OrganizationID: "org-1"}, 405},
	} {
		t.Run(tc.name, func(t *testing.T) {
			cfg, repo, oauth := testOAuthStack(t)
			reader := &publicSignalsFixture{}
			app := NewServer(ServerConfig{Config: cfg, Repo: repo, OAuth: oauth, PublicSignals: reader, Auth: fakeVerifier{principal: tc.principal}})
			req := httptest.NewRequest(tc.method, "/api/v1/insights/public-signals"+tc.query, strings.NewReader(tc.body))
			req.Header.Set("Content-Type", tc.media)
			req.Header.Set("X-Org-ID", "org-forged")
			if tc.bearer {
				req.Header.Set("Authorization", "Bearer synthetic")
			}
			res, err := app.Test(req)
			if err != nil {
				t.Fatal(err)
			}
			defer res.Body.Close()
			if res.StatusCode != tc.want {
				raw, _ := io.ReadAll(res.Body)
				t.Fatalf("status%d want%d %s", res.StatusCode, tc.want, raw)
			}
			if tc.want == 200 {
				var envelope struct {
					Success bool                 `json:"success"`
					Data    publicsignals.Report `json:"data"`
				}
				if json.NewDecoder(res.Body).Decode(&envelope) != nil || !envelope.Success || envelope.Data.Site != "https://triodelab.no/" || res.Header.Get("Cache-Control") != "no-store" {
					t.Fatal("invalid public report")
				}
				if reader.calls != 1 {
					t.Fatal("read repeated")
				}
			} else if reader.calls != 0 {
				t.Fatal("invalid input collected")
			}
		})
	}
}
func TestPublicSignalsAPIEnforcesRateLimitAndRejectsLegacyTenantKey(t *testing.T) {
	cfg, repo, oauth := testOAuthStack(t)
	cfg.RateLimitEnabled = true
	cfg.RateLimitMax = 1
	cfg.RateLimitWindow = time.Minute
	cfg.AllowLegacyTenantKey = true
	reader := &publicSignalsFixture{}
	app := NewServer(ServerConfig{Config: cfg, Repo: repo, OAuth: oauth, PublicSignals: reader, Auth: fakeVerifier{principal: auth.Principal{UserID: "u1", OrganizationID: "org-1"}}})
	for _, want := range []int{200, 429} {
		req := httptest.NewRequest("POST", "/api/v1/insights/public-signals", strings.NewReader(`{"site":"https://triodelab.no/"}`))
		req.Header.Set("Content-Type", "application/json")
		req.Header.Set("Authorization", "Bearer synthetic")
		res, err := app.Test(req)
		if err != nil {
			t.Fatal(err)
		}
		res.Body.Close()
		if res.StatusCode != want {
			t.Fatal(res.StatusCode, want)
		}
	}
	if reader.calls != 1 {
		t.Fatal("rate limit bypassed")
	}
	cfg.RateLimitEnabled = false
	app = NewServer(ServerConfig{Config: cfg, Repo: repo, OAuth: oauth, PublicSignals: reader})
	req := httptest.NewRequest("POST", "/api/v1/insights/public-signals", strings.NewReader(`{"site":"https://triodelab.no/"}`))
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("X-Internal-API-Key", cfg.InternalAPIKey)
	req.Header.Set("X-Org-ID", "org-forged")
	res, err := app.Test(req)
	if err != nil {
		t.Fatal(err)
	}
	res.Body.Close()
	if res.StatusCode != 403 {
		t.Fatal("legacy tenant authority accepted")
	}
}

func TestPublicSignalsAdmissionFailureIsExplicit(t *testing.T) {
	cfg, repo, oauth := testOAuthStack(t)
	for _, tc := range []struct {
		err  error
		want int
	}{{publicsignals.ErrBusy, 429}, {publicsignals.ErrInvalid, 503}} {
		reader := &publicSignalsFixture{failure: tc.err}
		app := NewServer(ServerConfig{Config: cfg, Repo: repo, OAuth: oauth, PublicSignals: reader, Auth: fakeVerifier{principal: auth.Principal{UserID: "u1", OrganizationID: "org-1"}}})
		req := httptest.NewRequest("POST", "/api/v1/insights/public-signals", strings.NewReader(`{"site":"https://triodelab.no/"}`))
		req.Header.Set("Content-Type", "application/json")
		req.Header.Set("Authorization", "Bearer synthetic")
		res, err := app.Test(req)
		if err != nil {
			t.Fatal(err)
		}
		defer res.Body.Close()
		if res.StatusCode != tc.want {
			t.Fatal(res.StatusCode, tc.want)
		}
		if tc.want == 429 && res.Header.Get("Retry-After") != "60" {
			t.Fatal("missing admission cooldown")
		}
	}
}
