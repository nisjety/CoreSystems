package algoliamcp

import (
	"context"
	"encoding/json"
	"github.com/triodelab/integration-corev2/internal/oauth"
	"github.com/triodelab/integration-corev2/internal/store"
	"io"
	"net/http"
	"strings"
	"testing"
	"time"
)

type fixtureTokens struct{}

func (fixtureTokens) AlgoliaAccessToken(context.Context, string, string) (oauth.AccessTokenResult, error) {
	return oauth.AccessTokenResult{AccessToken: "protected-token"}, nil
}

type fixtureTransport func(*http.Request) (*http.Response, error)

func (f fixtureTransport) RoundTrip(r *http.Request) (*http.Response, error) { return f(r) }
func fixtureService(t *testing.T) (*Service, *store.MemoryRepository, *int) {
	t.Helper()
	repo := store.NewMemoryRepository()
	_, err := repo.UpsertConnection(context.Background(), store.Connection{ID: "conn_one", ProviderKey: "algolia", ConnectorType: "algolia", OrganizationID: "org-1", UserID: "u1", Status: "active", EncryptedAccessToken: "encrypted-fixture", AccessTokenExpiresAt: time.Date(2026, 10, 24, 12, 0, 0, 0, time.UTC), CreatedAt: time.Date(2026, 10, 23, 12, 0, 0, 0, time.UTC), Scopes: []string{"public"}, Capabilities: []string{"search.analytics.read"}, ProviderContext: map[string]string{"algolia_resource": "https://mcp.algolia.com/mcp"}})
	if err != nil {
		t.Fatal(err)
	}
	calls := new(int)
	client := &http.Client{Transport: fixtureTransport(func(r *http.Request) (*http.Response, error) {
		*calls++
		if r.URL.String() != "https://mcp.algolia.com/mcp" || r.Header.Get("Authorization") != "Bearer protected-token" {
			t.Fatal("unpinned MCP bearer")
		}
		var q struct {
			ID     int             `json:"id"`
			Method string          `json:"method"`
			Params json.RawMessage `json:"params"`
		}
		_ = json.NewDecoder(r.Body).Decode(&q)
		var result any = map[string]any{}
		switch q.Method {
		case "initialize":
			result = map[string]any{"protocolVersion": "2025-03-26", "capabilities": map[string]any{"tools": map[string]any{}}, "serverInfo": map[string]any{"name": "fixture", "version": "1"}}
		case "notifications/initialized":
			return &http.Response{StatusCode: 202, Header: http.Header{}, Body: io.NopCloser(strings.NewReader(""))}, nil
		case "tools/list":
			result = json.RawMessage(`{"tools":[{"name":"algolia_search_list_indices","inputSchema":{"type":"object","properties":{"applicationId":{"type":"string","enum":["APP1234567"]}},"required":["applicationId"]}},{"name":"algolia_analytics_no_results_rate","inputSchema":{"type":"object","properties":{"applicationId":{"type":"string"},"indexName":{"type":"string"},"startDate":{"type":"string"},"endDate":{"type":"string"}},"required":["applicationId","indexName","startDate","endDate"]}}]}`)
		case "tools/call":
			var p struct {
				Name string `json:"name"`
			}
			_ = json.Unmarshal(q.Params, &p)
			if p.Name == "algolia_search_list_indices" {
				result = map[string]any{"content": []any{map[string]any{"type": "text", "text": `{"items":[{"name":"public_index"}]}`}}}
			} else if p.Name == "algolia_analytics_no_results_rate" {
				result = map[string]any{"content": []any{map[string]any{"type": "text", "text": `{"count":10,"noResultCount":2,"rate":0.2,"dates":[{"date":"2026-10-22","count":10,"noResultCount":2,"rate":0.2}]}`}}}
			} else {
				t.Fatal("unexpected tool")
			}
		default:
			t.Fatal("unbounded RPC method")
		}
		body, _ := json.Marshal(map[string]any{"jsonrpc": "2.0", "id": q.ID, "result": result})
		return &http.Response{StatusCode: 200, Header: http.Header{"Content-Type": []string{"application/json"}}, Body: io.NopCloser(strings.NewReader(string(body)))}, nil
	})}
	s := New(repo, fixtureTokens{}, client, func() time.Time { return time.Date(2026, 10, 23, 12, 0, 0, 0, time.UTC) })
	return s, repo, calls
}
func TestMCPDiscoveryReportAndAccess(t *testing.T) {
	s, repo, calls := fixtureService(t)
	ctx := context.Background()
	before, err := s.Access(ctx, "org-1", "conn_one")
	if err != nil || before.Status != "active" || len(before.Generation) != 32 || *calls != 0 {
		t.Fatalf("access %v", err)
	}
	apps, err := s.Applications(ctx, "org-1", "conn_one")
	if err != nil || len(apps) != 1 || apps[0].ID != "APP1234567" {
		t.Fatalf("apps %v %v", apps, err)
	}
	indices, err := s.Indices(ctx, "org-1", "conn_one", "APP1234567")
	if err != nil || len(indices) != 1 || indices[0].Name != "public_index" {
		t.Fatalf("indices %v %v", indices, err)
	}
	report, err := s.Report(ctx, "org-1", ReportInput{WebsiteID: "web_12345678901234567890123456789012", Site: "https://triodelab.no/", WebsiteRevision: 1, ConnectionID: "conn_one", ApplicationID: "APP1234567", IndexName: "public_index"})
	if err != nil || report.Status != "connected" || *report.Algolia.Analytics.Searches != 10 {
		t.Fatalf("report %+v %v", report, err)
	}
	connection, _ := repo.GetConnection(ctx, "conn_one")
	connection.LastRefreshedAt = time.Now()
	_, _ = repo.UpsertConnection(ctx, connection)
	after, _ := s.Access(ctx, "org-1", "conn_one")
	if before.Generation != after.Generation {
		t.Fatal("token refresh changed authority generation")
	}
	connection.Status = "deleted"
	_, _ = repo.UpsertConnection(ctx, connection)
	after, _ = s.Access(ctx, "org-1", "conn_one")
	if after.Status != "reconnect_required" || after.Generation == before.Generation {
		t.Fatal("revoked grant still active")
	}
	if _, err = s.Access(ctx, "wrong-org", "conn_one"); err == nil {
		t.Fatal("cross-org grant exposed")
	}
}
func TestMCPRejectsUnmappedResourcesBeforeAnalytics(t *testing.T) {
	s, _, calls := fixtureService(t)
	if _, err := s.Indices(context.Background(), "org-1", "conn_one", "FORGED1234"); err == nil {
		t.Fatal("unknown app allowed")
	}
	if *calls != 3 {
		t.Fatalf("unexpected calls %d", *calls)
	}
}
