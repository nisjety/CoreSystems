package sharepoint

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
)

type staticTokenProvider struct{ token string }

func (s staticTokenProvider) AccessToken(context.Context, string) (string, error) {
	return s.token, nil
}

func TestDeltaClientFetchSuccess(t *testing.T) {
	t.Parallel()

	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if got := r.Header.Get("Authorization"); got != "Bearer test-token" {
			t.Errorf("Authorization = %q, want Bearer test-token", got)
		}
		if !strings.HasSuffix(r.URL.Path, "/drives/drive-1/root/delta") {
			t.Errorf("URL path = %q, want suffix /drives/drive-1/root/delta", r.URL.Path)
		}
		_ = json.NewEncoder(w).Encode(map[string]any{
			"value": []map[string]any{
				{
					"id":     "item-1",
					"name":   "report.docx",
					"size":   1024,
					"webUrl": "https://example.sharepoint.com/sites/finance/report.docx",
					"file": map[string]any{
						"mimeType": "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
						"hashes": map[string]any{
							"quickXorHash": "QX==",
							"sha1Hash":     "AAAAAA",
						},
					},
					"parentReference": map[string]any{
						"driveId": "drive-1",
						"id":      "folder-1",
						"path":    "/drives/drive-1/root:/Reports",
					},
				},
				{
					"id":      "item-2",
					"deleted": map[string]any{"state": "deleted"},
				},
				{
					"id":     "folder-1",
					"name":   "Reports",
					"folder": map[string]any{"childCount": 3},
				},
			},
			"@odata.deltaLink": "https://graph.microsoft.com/v1.0/drives/drive-1/root/delta?token=abc",
		})
	}))
	defer server.Close()

	client := NewDeltaClient(DeltaClientConfig{
		BaseURL:       server.URL,
		TokenProvider: staticTokenProvider{token: "test-token"},
	})

	page, err := client.Fetch(context.Background(), "org-1", client.InitialDeltaURL("drive-1"))
	if err != nil {
		t.Fatalf("Fetch: %v", err)
	}

	if len(page.Items) != 3 {
		t.Fatalf("len items = %d, want 3", len(page.Items))
	}
	if page.NextLink != "" {
		t.Fatalf("NextLink = %q, want empty", page.NextLink)
	}
	if !strings.Contains(page.DeltaLink, "token=abc") {
		t.Fatalf("DeltaLink = %q, want token suffix", page.DeltaLink)
	}

	first := page.Items[0]
	if first.QuickXorHash() != "QX==" {
		t.Errorf("QuickXorHash = %q, want QX==", first.QuickXorHash())
	}
	if first.SHA1Hash() != "AAAAAA" {
		t.Errorf("SHA1Hash = %q", first.SHA1Hash())
	}
	if first.ParentItemID() != "folder-1" {
		t.Errorf("ParentItemID = %q, want folder-1", first.ParentItemID())
	}
	if got, want := first.FullPath(), "/Reports/report.docx"; got != want {
		t.Errorf("FullPath = %q, want %q", got, want)
	}

	if !page.Items[1].IsDeleted() {
		t.Errorf("item[1] should be deleted")
	}
	if !page.Items[2].IsFolder() {
		t.Errorf("item[2] should be folder")
	}
}

func TestDeltaClientFetchReturnsErrNotConfiguredWhenTokenMissing(t *testing.T) {
	t.Parallel()

	client := NewDeltaClient(DeltaClientConfig{})
	_, err := client.Fetch(context.Background(), "org-1", "https://graph.microsoft.com/v1.0/drives/d/root/delta")
	if err == nil {
		t.Fatal("expected error, got nil")
	}
}

func TestDeltaClientFetchPropagatesHTTPError(t *testing.T) {
	t.Parallel()

	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		http.Error(w, "boom", http.StatusBadGateway)
	}))
	defer server.Close()

	client := NewDeltaClient(DeltaClientConfig{
		BaseURL:       server.URL,
		TokenProvider: staticTokenProvider{token: "t"},
	})
	_, err := client.Fetch(context.Background(), "org-1", server.URL+"/v1.0/drives/d/root/delta")
	if err == nil {
		t.Fatal("expected error, got nil")
	}
}
