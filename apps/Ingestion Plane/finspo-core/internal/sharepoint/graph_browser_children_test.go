package sharepoint

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"testing"
)

func TestGraphBrowserListChildrenReturnsFoldersOnly(t *testing.T) {
	t.Parallel()

	provider := &stubAccessTokenProvider{token: "token-abc"}
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/v1.0/drives/b!drive-1/items/root/children" {
			t.Fatalf("path = %s, want /v1.0/drives/b!drive-1/items/root/children", r.URL.Path)
		}
		if got := r.Header.Get("Authorization"); got != "Bearer token-abc" {
			t.Fatalf("authorization = %q, want Bearer token-abc", got)
		}

		_ = json.NewEncoder(w).Encode(map[string]any{
			"value": []map[string]any{
				{
					"id":     "folder-1",
					"name":   "Contracts",
					"webUrl": "https://example.sharepoint.com/Contracts",
					"folder": map[string]any{"childCount": 12},
					"parentReference": map[string]any{
						"path": "/drives/b!drive-1/root:",
					},
				},
				{
					"id":   "file-1",
					"name": "readme.txt",
					"file": map[string]any{"mimeType": "text/plain"},
					"parentReference": map[string]any{
						"path": "/drives/b!drive-1/root:",
					},
				},
			},
		})
	}))
	defer server.Close()

	browser := NewGraphBrowser(GraphBrowserConfig{
		BaseURL:       server.URL,
		TokenProvider: provider,
	})

	folders, err := browser.ListChildren(context.Background(), "org-1", "b!drive-1", "")
	if err != nil {
		t.Fatalf("ListChildren(): %v", err)
	}

	if len(folders) != 1 {
		t.Fatalf("len(folders) = %d, want 1 (files must be dropped)", len(folders))
	}
	got := folders[0]
	if got.ID != "folder-1" {
		t.Fatalf("folder ID = %q, want folder-1", got.ID)
	}
	if got.Path != "/Contracts" {
		t.Fatalf("folder path = %q, want /Contracts", got.Path)
	}
	if got.ChildCount != 12 {
		t.Fatalf("child count = %d, want 12", got.ChildCount)
	}
}

func TestGraphBrowserListChildrenDrillsIntoItemAndFollowsPaging(t *testing.T) {
	t.Parallel()

	provider := &stubAccessTokenProvider{token: "token-abc"}
	var server *httptest.Server
	server = httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/v1.0/drives/b!drive-1/items/folder-1/children" {
			t.Fatalf("path = %s, want /v1.0/drives/b!drive-1/items/folder-1/children", r.URL.Path)
		}

		if r.URL.Query().Get("page") == "2" {
			_ = json.NewEncoder(w).Encode(map[string]any{
				"value": []map[string]any{{
					"id":     "folder-3",
					"name":   "2027",
					"folder": map[string]any{"childCount": 0},
					"parentReference": map[string]any{
						"path": "/drives/b!drive-1/root:/Contracts",
					},
				}},
			})
			return
		}

		_ = json.NewEncoder(w).Encode(map[string]any{
			"value": []map[string]any{{
				"id":     "folder-2",
				"name":   "2026",
				"folder": map[string]any{"childCount": 4},
				"parentReference": map[string]any{
					"path": "/drives/b!drive-1/root:/Contracts",
				},
			}},
			"@odata.nextLink": server.URL + "/v1.0/drives/b!drive-1/items/folder-1/children?page=2",
		})
	}))
	defer server.Close()

	browser := NewGraphBrowser(GraphBrowserConfig{
		BaseURL:       server.URL,
		TokenProvider: provider,
	})

	folders, err := browser.ListChildren(context.Background(), "org-1", "b!drive-1", "folder-1")
	if err != nil {
		t.Fatalf("ListChildren(): %v", err)
	}

	if len(folders) != 2 {
		t.Fatalf("len(folders) = %d, want 2 (both pages)", len(folders))
	}
	if folders[0].Path != "/Contracts/2026" {
		t.Fatalf("folder[0] path = %q, want /Contracts/2026", folders[0].Path)
	}
	if folders[1].Path != "/Contracts/2027" {
		t.Fatalf("folder[1] path = %q, want /Contracts/2027", folders[1].Path)
	}
}

func TestGraphBrowserListChildrenWithoutTokenProviderFailsClosed(t *testing.T) {
	t.Parallel()

	browser := NewGraphBrowser(GraphBrowserConfig{})
	if _, err := browser.ListChildren(context.Background(), "org-1", "b!drive-1", ""); err == nil {
		t.Fatal("expected ErrNotConfigured, got nil")
	}
}
