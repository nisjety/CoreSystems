package sharepoint

import (
	"context"
	"encoding/json"
	"errors"
	"net/http"
	"net/http/httptest"
	"testing"
	"time"
)

type stubAccessTokenProvider struct {
	token         string
	err           error
	receivedOrgID string
}

func (s *stubAccessTokenProvider) AccessToken(_ context.Context, organizationID string) (string, error) {
	s.receivedOrgID = organizationID
	if s.err != nil {
		return "", s.err
	}
	return s.token, nil
}

func TestGraphBrowserListSitesUsesTokenProvider(t *testing.T) {
	t.Parallel()

	provider := &stubAccessTokenProvider{token: "token-123"}
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.Method != http.MethodGet {
			t.Fatalf("method = %s, want GET", r.Method)
		}
		if r.URL.Path != "/v1.0/sites" {
			t.Fatalf("path = %s, want /v1.0/sites", r.URL.Path)
		}
		if r.URL.Query().Get("search") != "*" {
			t.Fatalf("search = %q, want *", r.URL.Query().Get("search"))
		}
		if got := r.Header.Get("Authorization"); got != "Bearer token-123" {
			t.Fatalf("authorization = %q, want Bearer token-123", got)
		}

		_ = json.NewEncoder(w).Encode(map[string]any{
			"value": []map[string]any{{
				"id":          "site-1",
				"name":        "finance",
				"displayName": "Finance",
				"webUrl":      "https://example.sharepoint.com/sites/finance",
				"description": "Finance workspace",
			}},
		})
	}))
	defer server.Close()

	browser := NewGraphBrowser(GraphBrowserConfig{
		BaseURL:       server.URL,
		TokenProvider: provider,
	})

	sites, err := browser.ListSites(context.Background(), "org-123")
	if err != nil {
		t.Fatalf("ListSites(): %v", err)
	}

	if provider.receivedOrgID != "org-123" {
		t.Fatalf("orgID = %q, want org-123", provider.receivedOrgID)
	}
	if len(sites) != 1 {
		t.Fatalf("len(sites) = %d, want 1", len(sites))
	}
	if sites[0].ID != "site-1" {
		t.Fatalf("site ID = %q, want site-1", sites[0].ID)
	}
	if sites[0].DisplayName != "Finance" {
		t.Fatalf("display name = %q, want Finance", sites[0].DisplayName)
	}
}

func TestGraphBrowserListItemsBuildsDriveChildrenPath(t *testing.T) {
	t.Parallel()

	provider := &stubAccessTokenProvider{token: "token-123"}
	modifiedAt := time.Date(2026, time.April, 9, 10, 0, 0, 0, time.UTC)
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.EscapedPath() != "/v1.0/sites/site-1/drive/root:/Shared%20Documents/Reports:/children" {
			t.Fatalf("path = %s, want /v1.0/sites/site-1/drive/root:/Shared%%20Documents/Reports:/children", r.URL.EscapedPath())
		}
		if got := r.Header.Get("Authorization"); got != "Bearer token-123" {
			t.Fatalf("authorization = %q, want Bearer token-123", got)
		}

		_ = json.NewEncoder(w).Encode(map[string]any{
			"value": []map[string]any{
				{
					"name":                 "Budget.xlsx",
					"size":                 1024,
					"lastModifiedDateTime": modifiedAt.Format(time.RFC3339),
					"webUrl":               "https://example.sharepoint.com/sites/finance/Budget.xlsx",
				},
				{
					"name":   "Archive",
					"size":   0,
					"folder": map[string]any{},
					"webUrl": "https://example.sharepoint.com/sites/finance/Archive",
				},
			},
		})
	}))
	defer server.Close()

	browser := NewGraphBrowser(GraphBrowserConfig{
		BaseURL:       server.URL,
		TokenProvider: provider,
	})

	items, err := browser.ListItems(context.Background(), "org-123", "site-1", "/Shared Documents/Reports")
	if err != nil {
		t.Fatalf("ListItems(): %v", err)
	}

	if provider.receivedOrgID != "org-123" {
		t.Fatalf("orgID = %q, want org-123", provider.receivedOrgID)
	}
	if len(items) != 2 {
		t.Fatalf("len(items) = %d, want 2", len(items))
	}
	if items[0].Path != "/Shared Documents/Reports/Budget.xlsx" {
		t.Fatalf("first item path = %q, want /Shared Documents/Reports/Budget.xlsx", items[0].Path)
	}
	if items[1].IsFolder != true {
		t.Fatalf("second item IsFolder = %t, want true", items[1].IsFolder)
	}
	if items[1].Path != "/Shared Documents/Reports/Archive" {
		t.Fatalf("second item path = %q, want /Shared Documents/Reports/Archive", items[1].Path)
	}
}

func TestGraphBrowserPropagatesTokenProviderErrors(t *testing.T) {
	t.Parallel()

	provider := &stubAccessTokenProvider{err: errors.New("token unavailable")}
	browser := NewGraphBrowser(GraphBrowserConfig{TokenProvider: provider})

	_, err := browser.ListSites(context.Background(), "org-123")
	if err == nil {
		t.Fatal("ListSites() error = nil, want non-nil")
	}
}

func TestBuildDriveChildrenPathPreservesGraphSiteIDFormat(t *testing.T) {
	t.Parallel()

	siteID := "contoso.sharepoint.com,2C712604-1370-44E7-A1F5-426573FDA80A,2D2244C3-251A-49EA-93A8-39E1C3A060FE"
	got := buildDriveChildrenPath(siteID, "/Shared Documents")
	want := "/v1.0/sites/contoso.sharepoint.com,2C712604-1370-44E7-A1F5-426573FDA80A,2D2244C3-251A-49EA-93A8-39E1C3A060FE/drive/root:/Shared%20Documents:/children"

	if got != want {
		t.Fatalf("buildDriveChildrenPath() = %q, want %q", got, want)
	}
}

func TestGraphBrowserListSitesPagination(t *testing.T) {
	t.Parallel()

	provider := &stubAccessTokenProvider{token: "tok"}
	requestNum := 0
	var server *httptest.Server
	server = httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		requestNum++
		switch requestNum {
		case 1:
			_ = json.NewEncoder(w).Encode(map[string]any{
				"value": []map[string]any{
					{"id": "s1", "name": "alpha", "displayName": "Alpha"},
					{"id": "s2", "name": "beta", "displayName": "Beta"},
				},
				"@odata.nextLink": server.URL + "/v1.0/sites?$skiptoken=page2",
			})
		default:
			_ = json.NewEncoder(w).Encode(map[string]any{
				"value": []map[string]any{
					{"id": "s3", "name": "gamma", "displayName": "Gamma"},
				},
			})
		}
	}))
	defer server.Close()

	browser := NewGraphBrowser(GraphBrowserConfig{
		BaseURL:       server.URL,
		TokenProvider: provider,
	})

	sites, err := browser.ListSites(context.Background(), "org-1")
	if err != nil {
		t.Fatalf("ListSites(): %v", err)
	}
	if len(sites) != 3 {
		t.Fatalf("len(sites) = %d, want 3", len(sites))
	}
	if sites[0].ID != "s1" {
		t.Fatalf("sites[0].ID = %q, want s1", sites[0].ID)
	}
	if sites[2].ID != "s3" {
		t.Fatalf("sites[2].ID = %q, want s3", sites[2].ID)
	}
}

func TestGraphBrowserListItemsPagination(t *testing.T) {
	t.Parallel()

	provider := &stubAccessTokenProvider{token: "tok"}
	requestNum := 0
	var server *httptest.Server
	server = httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		requestNum++
		switch requestNum {
		case 1:
			_ = json.NewEncoder(w).Encode(map[string]any{
				"value": []map[string]any{
					{"name": "file1.docx", "size": int64(100)},
					{"name": "file2.xlsx", "size": int64(200)},
				},
				"@odata.nextLink": server.URL + "/v1.0/sites/site-1/drive/root:/Reports:/children?$skiptoken=page2",
			})
		default:
			_ = json.NewEncoder(w).Encode(map[string]any{
				"value": []map[string]any{
					{"name": "file3.pdf", "size": int64(300)},
				},
			})
		}
	}))
	defer server.Close()

	browser := NewGraphBrowser(GraphBrowserConfig{
		BaseURL:       server.URL,
		TokenProvider: provider,
	})

	items, err := browser.ListItems(context.Background(), "org-1", "site-1", "/Reports")
	if err != nil {
		t.Fatalf("ListItems(): %v", err)
	}
	if len(items) != 3 {
		t.Fatalf("len(items) = %d, want 3", len(items))
	}
	if items[0].Name != "file1.docx" {
		t.Fatalf("items[0].Name = %q, want file1.docx", items[0].Name)
	}
	if items[2].Name != "file3.pdf" {
		t.Fatalf("items[2].Name = %q, want file3.pdf", items[2].Name)
	}
}
