package sharepoint

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
)

func TestPermissionsClientListItemPermissions(t *testing.T) {
	t.Parallel()

	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if got := r.Header.Get("Authorization"); got != "Bearer tok" {
			t.Errorf("Authorization = %q", got)
		}
		if !strings.HasSuffix(r.URL.Path, "/drives/d1/items/i1/permissions") {
			t.Errorf("path = %q", r.URL.Path)
		}
		_ = json.NewEncoder(w).Encode(map[string]any{
			"value": []map[string]any{
				{
					"id":    "perm-1",
					"roles": []string{"read"},
					"grantedToV2": map[string]any{
						"user": map[string]any{"id": "user-1", "displayName": "Alice"},
					},
				},
				{
					"id":    "perm-2",
					"roles": []string{"write"},
					"link":  map[string]any{"scope": "organization", "type": "view"},
					"inheritedFrom": map[string]any{
						"driveId": "d1",
						"id":      "parent-1",
						"path":    "/drives/d1/root:/Reports",
					},
				},
			},
		})
	}))
	defer server.Close()

	client := NewPermissionsClient(PermissionsClientConfig{
		BaseURL:       server.URL,
		TokenProvider: staticTokenProvider{token: "tok"},
	})

	perms, err := client.ListItemPermissions(context.Background(), "org-1", "d1", "i1")
	if err != nil {
		t.Fatalf("ListItemPermissions: %v", err)
	}
	if len(perms) != 2 {
		t.Fatalf("len = %d, want 2", len(perms))
	}
	if id, kind, name := perms[0].GrantedToV2.Principal(); id != "user-1" || kind != "user" || name != "Alice" {
		t.Errorf("principal[0] = (%q, %q, %q)", id, kind, name)
	}
	if !perms[1].IsInherited() {
		t.Errorf("perm[1] should be inherited")
	}
	if perms[1].Link == nil || perms[1].Link.Scope != "organization" {
		t.Errorf("perm[1].Link = %#v", perms[1].Link)
	}
}

func TestPermissionsClientReturnsErrNotConfiguredWhenTokenMissing(t *testing.T) {
	t.Parallel()

	client := NewPermissionsClient(PermissionsClientConfig{})
	_, err := client.ListItemPermissions(context.Background(), "org-1", "d1", "i1")
	if err == nil {
		t.Fatal("expected error, got nil")
	}
}
