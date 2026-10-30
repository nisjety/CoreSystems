package api

import (
	"github.com/triodelab/integration-corev2/internal/sitesources"
	"github.com/triodelab/integration-corev2/internal/webvitals"
	"io"
	"net/http/httptest"
	"strings"
	"testing"
)

type bindingSources struct{ sourceFixture }

func (f *bindingSources) Configured(org string, in sitesources.Input) (bool, error) {
	return org == "org-1" && in.WebsiteRevision == 7, nil
}

type bindingVitals struct{ vitalsFixture }

func (f *bindingVitals) Configured(org string, in webvitals.Input) (bool, error) {
	return org == "org-1" && in.WebsiteRevision == 7, nil
}

func TestBindingMetadataUsesVerifiedOrgAndNeverCollects(t *testing.T) {
	for _, endpoint := range []struct{ path, body string }{{"site-sources", sourceBody}, {"web-vitals", vitalsBody}} {
		for _, authorized := range []bool{false, true} {
			cfg, repo, oauth := testOAuthStack(t)
			sources := &bindingSources{}
			vitals := &bindingVitals{}
			app := NewServer(ServerConfig{Config: cfg, Repo: repo, OAuth: oauth, Auth: fakeVerifier{principal: sourcePrincipal()}, SiteSources: sources, WebVitals: vitals})
			req := httptest.NewRequest("POST", "/api/v1/insights/"+endpoint.path+"/binding", strings.NewReader(endpoint.body))
			req.Header.Set("Content-Type", "application/json")
			req.Header.Set("X-Org-ID", "forged")
			if authorized {
				req.Header.Set("Authorization", "Bearer synthetic")
			}
			res, err := app.Test(req)
			if err != nil {
				t.Fatal(err)
			}
			raw, _ := io.ReadAll(res.Body)
			res.Body.Close()
			want := 401
			if authorized {
				want = 200
			}
			if res.StatusCode != want {
				t.Fatalf("%s: %d %s", endpoint.path, res.StatusCode, raw)
			}
			if authorized && (!strings.Contains(string(raw), `"configured":true`) || res.Header.Get("Cache-Control") != "no-store" || strings.Contains(string(raw), "tokenFile")) {
				t.Fatal("invalid metadata", string(raw))
			}
			if sources.calls != 0 || vitals.calls != 0 {
				t.Fatal("metadata collected provider data")
			}
		}
	}
}
