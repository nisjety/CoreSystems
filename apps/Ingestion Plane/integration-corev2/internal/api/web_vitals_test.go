package api

import (
	"context"
	"encoding/json"
	"errors"
	"io"
	"net/http/httptest"
	"strings"
	"testing"
	"time"

	"github.com/triodelab/integration-corev2/internal/auth"
	"github.com/triodelab/integration-corev2/internal/webvitals"
)

const vitalsBody = `{"websiteId":"web_12345678901234567890123456789012","site":"https://triodelab.no/","websiteRevision":7,"environment":"production"}`

type vitalsFixture struct {
	calls   int
	failure error
}

func (f *vitalsFixture) Get(_ context.Context, org string, in webvitals.Input) (webvitals.Report, error) {
	f.calls++
	if org != "org-1" {
		return webvitals.Report{}, errors.New("forged organization")
	}
	return webvitals.Report{WebsiteID: in.WebsiteID, Site: in.Site, WebsiteRevision: in.WebsiteRevision, Environment: in.Environment, Status: "not_configured", Samples: []webvitals.Sample{}, ObservedAt: "2026-10-23T12:00:00Z"}, f.failure
}
func TestWebVitalsReadAPIRejectsForgedAuthorityAndInput(t *testing.T) {
	for _, tc := range []struct {
		name, body, query, method, media string
		principal                        auth.Principal
		bearer, internal                 bool
		want                             int
	}{
		{"anonymous", vitalsBody, "", "POST", "application/json", auth.Principal{}, false, false, 401},
		{"read", vitalsBody, "", "POST", "application/json", auth.Principal{UserID: "service:marketing", OrganizationID: "org-1", PrincipalType: "service", Scopes: []string{"integration:read"}}, true, false, 200},
		{"no-scope", vitalsBody, "", "POST", "application/json", auth.Principal{UserID: "service:marketing", OrganizationID: "org-1", PrincipalType: "service"}, true, false, 403},
		{"internal", vitalsBody, "", "POST", "application/json", auth.Principal{}, false, true, 403},
		{"duplicate", strings.Replace(vitalsBody, `"websiteRevision":7`, `"websiteRevision":7,"websiteRevision":8`, 1), "", "POST", "application/json", auth.Principal{UserID: "u1", OrganizationID: "org-1"}, true, false, 400},
		{"unknown", strings.Replace(vitalsBody, `"websiteRevision":7`, `"websiteRevision":7,"token":"secret"`, 1), "", "POST", "application/json", auth.Principal{UserID: "u1", OrganizationID: "org-1"}, true, false, 400},
		{"missing", strings.Replace(vitalsBody, `,"environment":"production"`, "", 1), "", "POST", "application/json", auth.Principal{UserID: "u1", OrganizationID: "org-1"}, true, false, 400},
		{"query", vitalsBody, "?organizationId=forged", "POST", "application/json", auth.Principal{UserID: "u1", OrganizationID: "org-1"}, true, false, 400},
		{"oversize", vitalsBody + strings.Repeat(" ", 2048), "", "POST", "application/json", auth.Principal{UserID: "u1", OrganizationID: "org-1"}, true, false, 400},
		{"media", vitalsBody, "", "POST", "text/plain", auth.Principal{UserID: "u1", OrganizationID: "org-1"}, true, false, 400},
		{"method", "", "", "GET", "application/json", auth.Principal{UserID: "u1", OrganizationID: "org-1"}, true, false, 405},
	} {
		t.Run(tc.name, func(t *testing.T) {
			cfg, repo, oauth := testOAuthStack(t)
			reader := &vitalsFixture{}
			app := NewServer(ServerConfig{Config: cfg, Repo: repo, OAuth: oauth, Auth: fakeVerifier{principal: tc.principal}, WebVitals: reader})
			req := httptest.NewRequest(tc.method, "/api/v1/insights/web-vitals"+tc.query, strings.NewReader(tc.body))
			req.Header.Set("Content-Type", tc.media)
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
				t.Fatalf("status %d want %d %s", res.StatusCode, tc.want, raw)
			}
			if tc.want == 200 {
				var receipt map[string]json.RawMessage
				if json.Unmarshal(raw, &receipt) != nil || len(receipt) != 2 || receipt["success"] == nil || receipt["data"] == nil {
					t.Fatal("receipt must match Marketing's exact two-key contract", string(raw))
				}
				if reader.calls != 1 || !strings.Contains(string(raw), `"status":"not_configured"`) || res.Header.Get("Cache-Control") != "no-store" {
					t.Fatal("invalid read")
				}
			} else if reader.calls != 0 {
				t.Fatal("rejected request read source")
			}
		})
	}
}
func TestWebVitalsAPIHasRateAndAdmissionLimits(t *testing.T) {
	for _, tc := range []struct {
		failure error
		want    int
	}{{webvitals.ErrBusy, 429}, {errors.New("private provider detail"), 503}} {
		cfg, repo, oauth := testOAuthStack(t)
		reader := &vitalsFixture{failure: tc.failure}
		app := NewServer(ServerConfig{Config: cfg, Repo: repo, OAuth: oauth, Auth: fakeVerifier{principal: auth.Principal{UserID: "u1", OrganizationID: "org-1"}}, WebVitals: reader})
		req := httptest.NewRequest("POST", "/api/v1/insights/web-vitals", strings.NewReader(vitalsBody))
		req.Header.Set("Authorization", "Bearer synthetic")
		req.Header.Set("Content-Type", "application/json")
		res, err := app.Test(req)
		if err != nil {
			t.Fatal(err)
		}
		defer res.Body.Close()
		raw, _ := io.ReadAll(res.Body)
		if res.StatusCode != tc.want || strings.Contains(string(raw), "private provider detail") {
			t.Fatal("failure hidden or leaked")
		}
		if tc.want == 429 && res.Header.Get("Retry-After") != "60" {
			t.Fatal("retry missing")
		}
	}
	cfg, repo, oauth := testOAuthStack(t)
	cfg.RateLimitEnabled = true
	cfg.RateLimitMax = 1
	cfg.RateLimitWindow = time.Minute
	reader := &vitalsFixture{}
	app := NewServer(ServerConfig{Config: cfg, Repo: repo, OAuth: oauth, Auth: fakeVerifier{principal: auth.Principal{UserID: "u1", OrganizationID: "org-1"}}, WebVitals: reader})
	for _, want := range []int{200, 429} {
		req := httptest.NewRequest("POST", "/api/v1/insights/web-vitals", strings.NewReader(vitalsBody))
		req.Header.Set("Authorization", "Bearer synthetic")
		req.Header.Set("Content-Type", "application/json")
		res, err := app.Test(req)
		if err != nil {
			t.Fatal(err)
		}
		res.Body.Close()
		if res.StatusCode != want {
			t.Fatalf("%d want %d", res.StatusCode, want)
		}
	}
}
