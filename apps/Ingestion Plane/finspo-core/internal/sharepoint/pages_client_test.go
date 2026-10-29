package sharepoint

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
)

func TestPagesClientListSitePagesUsesV1CastEndpoint(t *testing.T) {
	t.Parallel()

	provider := &stubAccessTokenProvider{token: "token-1"}
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/v1.0/sites/site-1/pages/microsoft.graph.sitePage" {
			t.Fatalf("path = %s, want /v1.0/sites/site-1/pages/microsoft.graph.sitePage", r.URL.Path)
		}
		if got := r.Header.Get("Authorization"); got != "Bearer token-1" {
			t.Fatalf("authorization = %q, want Bearer token-1", got)
		}
		_ = json.NewEncoder(w).Encode(map[string]any{
			"value": []map[string]any{{
				"id":         "page-1",
				"name":       "Home.aspx",
				"title":      "Home",
				"pageLayout": "home",
				"webUrl":     "https://example.sharepoint.com/SitePages/Home.aspx",
			}},
		})
	}))
	defer server.Close()

	client := NewPagesClient(PagesClientConfig{BaseURL: server.URL, TokenProvider: provider})
	pages, err := client.ListSitePages(context.Background(), "org-1", "site-1")
	if err != nil {
		t.Fatalf("ListSitePages(): %v", err)
	}
	if len(pages) != 1 {
		t.Fatalf("len(pages) = %d, want 1", len(pages))
	}
	if pages[0].ID != "page-1" || pages[0].Title != "Home" {
		t.Fatalf("page = %+v, want id page-1 / title Home", pages[0])
	}
	if provider.receivedOrgID != "org-1" {
		t.Fatalf("orgID = %q, want org-1", provider.receivedOrgID)
	}
}

func TestPagesClientListSitePagesFallsBackToBeta(t *testing.T) {
	t.Parallel()

	provider := &stubAccessTokenProvider{token: "token-1"}
	var sawBeta bool
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		switch r.URL.Path {
		case "/v1.0/sites/site-1/pages/microsoft.graph.sitePage":
			// Tenant where the v1.0 sitePages surface has not rolled out.
			w.WriteHeader(http.StatusBadRequest)
			_, _ = w.Write([]byte(`{"error":{"code":"BadRequest"}}`))
		case "/beta/sites/site-1/pages":
			sawBeta = true
			_ = json.NewEncoder(w).Encode(map[string]any{
				"value": []map[string]any{{"id": "page-9", "name": "News.aspx", "title": "News"}},
			})
		default:
			t.Fatalf("unexpected path %s", r.URL.Path)
		}
	}))
	defer server.Close()

	client := NewPagesClient(PagesClientConfig{BaseURL: server.URL, TokenProvider: provider})
	pages, err := client.ListSitePages(context.Background(), "org-1", "site-1")
	if err != nil {
		t.Fatalf("ListSitePages(): %v", err)
	}
	if !sawBeta {
		t.Fatal("expected fallback to the beta endpoint")
	}
	if len(pages) != 1 || pages[0].ID != "page-9" {
		t.Fatalf("pages = %+v, want single page-9", pages)
	}
}

func TestPagesClientListSitePagesSurfacesRealErrors(t *testing.T) {
	t.Parallel()

	provider := &stubAccessTokenProvider{token: "token-1"}
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		w.WriteHeader(http.StatusForbidden)
		_, _ = w.Write([]byte(`{"error":{"code":"accessDenied"}}`))
	}))
	defer server.Close()

	client := NewPagesClient(PagesClientConfig{BaseURL: server.URL, TokenProvider: provider})
	if _, err := client.ListSitePages(context.Background(), "org-1", "site-1"); err == nil {
		t.Fatal("expected 403 to surface as an error, got nil")
	} else if !strings.Contains(err.Error(), "403") {
		t.Fatalf("error = %v, want the 403 status in the message", err)
	}
}

func TestPagesClientFetchPageTextConcatenatesTextWebParts(t *testing.T) {
	t.Parallel()

	provider := &stubAccessTokenProvider{token: "token-1"}
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/v1.0/sites/site-1/pages/page-1/microsoft.graph.sitePage" {
			t.Fatalf("path = %s, want the cast page path", r.URL.Path)
		}
		if got := r.URL.Query().Get("$expand"); got != "canvasLayout" {
			t.Fatalf("$expand = %q, want canvasLayout", got)
		}
		_, _ = w.Write([]byte(`{
			"id": "page-1",
			"canvasLayout": {
				"horizontalSections": [
					{"columns": [{"webparts": [
						{"@odata.type": "#microsoft.graph.textWebPart", "innerHtml": "<h2>Welcome</h2><p>First &amp; foremost.</p>"},
						{"@odata.type": "#microsoft.graph.standardWebPart"}
					]}]}
				],
				"verticalSection": {"webParts": [
					{"@odata.type": "#microsoft.graph.textWebPart", "innerHtml": "<p>Sidebar note</p>"}
				]}
			}
		}`))
	}))
	defer server.Close()

	client := NewPagesClient(PagesClientConfig{BaseURL: server.URL, TokenProvider: provider})
	text, err := client.FetchPageText(context.Background(), "org-1", "site-1", "page-1")
	if err != nil {
		t.Fatalf("FetchPageText(): %v", err)
	}

	if !strings.Contains(text, "Welcome") || !strings.Contains(text, "First & foremost.") {
		t.Fatalf("text = %q, want horizontal-section content with decoded entities", text)
	}
	if !strings.Contains(text, "Sidebar note") {
		t.Fatalf("text = %q, want vertical-section content (webParts casing)", text)
	}
	if strings.Contains(text, "<") {
		t.Fatalf("text = %q, tags must not leak through", text)
	}
}

func TestPagesClientFetchPageTextEmptyCanvasReturnsEmpty(t *testing.T) {
	t.Parallel()

	provider := &stubAccessTokenProvider{token: "token-1"}
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		_, _ = w.Write([]byte(`{"id": "page-1", "canvasLayout": {"horizontalSections": []}}`))
	}))
	defer server.Close()

	client := NewPagesClient(PagesClientConfig{BaseURL: server.URL, TokenProvider: provider})
	text, err := client.FetchPageText(context.Background(), "org-1", "site-1", "page-1")
	if err != nil {
		t.Fatalf("FetchPageText(): %v", err)
	}
	if text != "" {
		t.Fatalf("text = %q, want empty for a canvas without text web parts", text)
	}
}
