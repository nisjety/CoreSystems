package api

import (
	"context"
	"github.com/gofiber/fiber/v2"
	"github.com/triodelab/integration-corev2/internal/algoliamcp"
	"github.com/triodelab/integration-corev2/internal/auth"
	"github.com/triodelab/integration-corev2/internal/sitesources"
	"github.com/triodelab/integration-corev2/internal/store"
	"io"
	"net/http/httptest"
	"strings"
	"testing"
)

type algoliaFixture struct{ calls int }

func (f *algoliaFixture) Access(_ context.Context, org, id string) (algoliamcp.Access, error) {
	f.calls++
	return algoliamcp.Access{ConnectionID: id, Status: "active", Generation: "01234567890123456789012345678901"}, nil
}

func TestAlgoliaBrokerNeverLeasesResourceBoundGrant(t *testing.T) {
	cfg, repo, oauth := testOAuthStack(t)
	cfg.TokenLeaseConsumers = []string{"marketing-core"}
	_, _ = repo.UpsertConnection(context.Background(), store.Connection{ID: "conn_one", ProviderKey: "algolia", ConnectorType: "algolia", OrganizationID: "org-1", Status: "active"})
	app := NewServer(ServerConfig{Config: cfg, Repo: repo, OAuth: oauth})
	for _, selector := range []string{`"connectionId":"conn_one"`, `"connectorType":"algolia"`} {
		req := httptest.NewRequest("POST", "/internal/connectors/token", strings.NewReader(`{"organizationId":"org-1","consumer":"marketing-core",`+selector+`}`))
		req.Header.Set("Content-Type", "application/json")
		req.Header.Set("X-Internal-API-Key", cfg.InternalAPIKey)
		res, err := app.Test(req)
		if err != nil {
			t.Fatal(err)
		}
		raw, _ := io.ReadAll(res.Body)
		_ = res.Body.Close()
		if res.StatusCode != 403 || !strings.Contains(string(raw), "resource_bound_token") || strings.Contains(string(raw), "accessToken") {
			t.Fatalf("broker leaked or did not reject %d %s", res.StatusCode, raw)
		}
	}
}
func TestAlgoliaFailuresAreGeneric(t *testing.T) {
	for _, tc := range []struct {
		err    error
		status int
	}{{algoliamcp.ErrAccess, 404}, {algoliamcp.ErrBusy, 429}, {algoliamcp.ErrUnavailable, 503}} {
		app := fiber.New()
		app.Get("/", func(c *fiber.Ctx) error { return algoliaError(c, tc.err) })
		res, err := app.Test(httptest.NewRequest("GET", "/", nil))
		if err != nil {
			t.Fatal(err)
		}
		_ = res.Body.Close()
		if res.StatusCode != tc.status {
			t.Fatal("wrong safe status")
		}
	}
	cfg, repo, oauth := testOAuthStack(t)
	app := NewServer(ServerConfig{Config: cfg, Repo: repo, OAuth: oauth})
	res, err := app.Test(httptest.NewRequest("GET", "/api/v1/providers/algolia/callback?state=invalid", nil))
	if err != nil {
		t.Fatal(err)
	}
	_ = res.Body.Close()
	if res.StatusCode != 200 {
		t.Fatal("registered callback alias missing")
	}
}
func (f *algoliaFixture) Applications(context.Context, string, string) ([]algoliamcp.Application, error) {
	f.calls++
	return []algoliamcp.Application{{ID: "APP1234567"}}, nil
}
func (f *algoliaFixture) Indices(context.Context, string, string, string) ([]algoliamcp.Index, error) {
	f.calls++
	return []algoliamcp.Index{{Name: "public_index"}}, nil
}
func (f *algoliaFixture) Resources(_ context.Context, _ string, in algoliamcp.ResourceInput) (algoliamcp.Resources, error) {
	f.calls++
	return algoliamcp.Resources{ConnectionID: in.ConnectionID, ApplicationID: in.ApplicationID, IndexName: in.IndexName, Status: "allowed"}, nil
}
func (f *algoliaFixture) Report(_ context.Context, _ string, in algoliamcp.ReportInput) (sitesources.Report, error) {
	f.calls++
	return sitesources.Report{Provider: "algolia", Status: "unavailable"}, nil
}
func TestAlgoliaAPIReadBoundaries(t *testing.T) {
	for _, tc := range []struct {
		name, path, method, body string
		p                        auth.Principal
		internal                 bool
		want                     int
	}{
		{"access", "/api/v1/insights/algolia/access", "POST", `{"connectionId":"conn_one"}`, sourcePrincipal(), false, 200},
		{"resources", "/api/v1/insights/algolia/resources", "POST", `{"connectionId":"conn_one","applicationId":"APP1234567","indexName":"public_index"}`, sourcePrincipal(), false, 200},
		{"unknown", "/api/v1/insights/algolia/access", "POST", `{"connectionId":"conn_one","url":"https://evil.test"}`, sourcePrincipal(), false, 400},
		{"null", "/api/v1/insights/algolia/access", "POST", `{"connectionId":null}`, sourcePrincipal(), false, 400},
		{"report", "/api/v1/insights/algolia/report", "POST", `{"websiteId":"web_12345678901234567890123456789012","site":"https://triodelab.no/","websiteRevision":1,"connectionId":"conn_one","applicationId":"APP1234567","indexName":"public_index"}`, sourcePrincipal(), false, 200},
		{"duplicate", "/api/v1/insights/algolia/access", "POST", `{"connectionId":"conn_one","connectionId":"conn_two"}`, sourcePrincipal(), false, 400},
		{"query", "/api/v1/insights/algolia/access?org=forged", "POST", `{"connectionId":"conn_one"}`, sourcePrincipal(), false, 400},
		{"internal", "/api/v1/insights/algolia/access", "POST", `{"connectionId":"conn_one"}`, auth.Principal{}, true, 403},
		{"human-no-scope", "/api/v1/insights/algolia/access", "POST", `{"connectionId":"conn_one"}`, auth.Principal{UserID: "u1", OrganizationID: "org-1"}, false, 403},
		{"user-discovery", "/api/v1/providers/algolia/connections/conn_one/applications", "GET", "", auth.Principal{UserID: "u1", OrganizationID: "org-1", PrincipalType: "user"}, false, 200},
		{"service-discovery", "/api/v1/providers/algolia/connections/conn_one/applications", "GET", "", sourcePrincipal(), false, 403},
		{"wrong-user", "/api/v1/providers/algolia/connections/conn_one/applications", "GET", "", auth.Principal{UserID: "u2", OrganizationID: "org-1", PrincipalType: "user"}, false, 403},
		{"wrong-org", "/api/v1/providers/algolia/connections/conn_one/applications", "GET", "", auth.Principal{UserID: "u1", OrganizationID: "org-2", PrincipalType: "user"}, false, 404},
		{"index", "/api/v1/providers/algolia/connections/conn_one/indices?applicationId=APP1234567", "GET", "", auth.Principal{UserID: "u1", OrganizationID: "org-1", PrincipalType: "user"}, false, 200},
		{"duplicate-query", "/api/v1/providers/algolia/connections/conn_one/indices?applicationId=A&applicationId=B", "GET", "", auth.Principal{UserID: "u1", OrganizationID: "org-1", PrincipalType: "user"}, false, 400},
	} {
		t.Run(tc.name, func(t *testing.T) {
			cfg, repo, oauth := testOAuthStack(t)
			_, _ = repo.UpsertConnection(context.Background(), store.Connection{ID: "conn_one", ProviderKey: "algolia", OrganizationID: "org-1", UserID: "u1", Status: "active"})
			reader := &algoliaFixture{}
			app := NewServer(ServerConfig{Config: cfg, Repo: repo, OAuth: oauth, Auth: fakeVerifier{principal: tc.p}, AlgoliaMCP: reader})
			req := httptest.NewRequest(tc.method, tc.path, strings.NewReader(tc.body))
			req.Header.Set("Content-Type", "application/json")
			if tc.internal {
				req.Header.Set("X-Internal-API-Key", cfg.InternalAPIKey)
			} else {
				req.Header.Set("Authorization", "Bearer fixture")
			}
			res, err := app.Test(req)
			if err != nil {
				t.Fatal(err)
			}
			defer res.Body.Close()
			body, _ := io.ReadAll(res.Body)
			if res.StatusCode != tc.want {
				t.Fatalf("status%d want%d %s", res.StatusCode, tc.want, body)
			}
			if tc.want == 200 && (!strings.Contains(string(body), `"success":true`) || strings.Contains(string(body), `"meta"`)) {
				t.Fatal("unsafe envelope")
			}
			if tc.want != 200 && reader.calls != 0 {
				t.Fatal("rejected input reached reader")
			}
		})
	}
}
