package api

import (
	"context"
	"github.com/triodelab/integration-corev2/internal/auth"
	"github.com/triodelab/integration-corev2/internal/trends"
	"net/http/httptest"
	"strings"
	"testing"
)

type trendsFixture struct{ calls int }

func (f *trendsFixture) Collect(_ context.Context, q trends.Query) (trends.Report, error) {
	f.calls++
	return trends.Report{Query: q, Status: "no_data"}, nil
}
func TestTrendsRequiresOrganizationReadScopeAndClosedInput(t *testing.T) {
	good := `{"terms":["SEO"],"geo":"NO","timeframe":"today 12-m"}`
	for _, tc := range []struct {
		body          string
		bearer, scope bool
		want          int
	}{
		{good, false, false, 401}, {good, true, false, 403}, {good, true, true, 200},
		{`{"terms":["SEO"],"geo":"NO","timeframe":"today 12-m","url":"http://localhost"}`, true, true, 400},
		{`{"terms":["SEO"],"geo":"NO","geo":"US","timeframe":"today 12-m"}`, true, true, 400},
	} {
		cfg, repo, oauth := testOAuthStack(t)
		fixture := &trendsFixture{}
		p := auth.Principal{UserID: "service:marketing", OrganizationID: "org-1", PrincipalType: "service"}
		if tc.scope {
			p.Scopes = []string{"integration:read"}
		}
		app := NewServer(ServerConfig{Config: cfg, Repo: repo, OAuth: oauth, Trends: fixture, Auth: fakeVerifier{principal: p}})
		req := httptest.NewRequest("POST", "/api/v1/insights/trends", strings.NewReader(tc.body))
		req.Header.Set("Content-Type", "application/json")
		req.Header.Set("X-Org-ID", "forged")
		if tc.bearer {
			req.Header.Set("Authorization", "Bearer fixture")
		}
		res, err := app.Test(req)
		if err != nil {
			t.Fatal(err)
		}
		res.Body.Close()
		if res.StatusCode != tc.want || tc.want != 200 && fixture.calls != 0 {
			t.Fatalf("status=%d want=%d calls=%d", res.StatusCode, tc.want, fixture.calls)
		}
	}
}
