package dataplane

import (
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"testing"
	"time"

	"github.com/google/uuid"

	"github.com/triodelab/finspo/internal/store"
)

func TestSourceObjectClientUpsertPayload(t *testing.T) {
	t.Parallel()

	var captured map[string]any
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/v1/source-objects/" {
			t.Fatalf("path = %q", r.URL.Path)
		}
		if got := r.Header.Get("X-Org-ID"); got != "org-1" {
			t.Fatalf("X-Org-ID = %q", got)
		}
		if got := r.Header.Get("X-Internal-Api-Key"); got != "secret" {
			t.Fatalf("X-Internal-Api-Key = %q", got)
		}
		if err := json.NewDecoder(r.Body).Decode(&captured); err != nil {
			t.Fatalf("decode body: %v", err)
		}
		w.WriteHeader(http.StatusCreated)
	}))
	defer server.Close()

	modified := time.Date(2026, time.May, 26, 12, 0, 0, 0, time.UTC)
	client := NewSourceObjectClient(server.URL, "secret")
	err := client.UpsertSourceObjectWithPermissions(t.Context(), store.Source{
		ID:             uuid.New(),
		OrganizationID: "org-1",
		TenantID:       "tenant-1",
		SiteID:         "site-1",
		DriveID:        "drive-1",
	}, store.Item{
		ItemID:       "item-1",
		ParentItemID: "parent-1",
		Path:         "/Reports/file.pdf",
		Name:         "file.pdf",
		MimeType:     "application/pdf",
		SizeBytes:    123,
		ModifiedAt:   &modified,
		QuickXorHash: "qx",
		SHA1Hash:     "sha1",
	}, []store.Permission{
		{
			PrincipalID:   "USER-1",
			PrincipalType: "User",
			Roles:         []string{"read", "read"},
			LinkScope:     "organization",
			LinkType:      "view",
			InheritedFrom: "parent-1",
		},
	})
	if err != nil {
		t.Fatalf("UpsertSourceObjectWithPermissions: %v", err)
	}

	if captured["external_id"] != "drive-1:item-1" {
		t.Fatalf("external_id = %#v", captured["external_id"])
	}
	if captured["content_hash"] != "sha1:sha1" {
		t.Fatalf("content_hash = %#v", captured["content_hash"])
	}
	if !hasACLTag(captured["acl_tags"], "sp:principal:user:user-1") {
		t.Fatalf("acl_tags missing principal tag: %#v", captured["acl_tags"])
	}
	if !hasACLTag(captured["acl_tags"], "sp:role:read") {
		t.Fatalf("acl_tags missing role tag: %#v", captured["acl_tags"])
	}
	if !hasACLTag(captured["acl_tags"], "sp:link_scope:organization") {
		t.Fatalf("acl_tags missing link scope tag: %#v", captured["acl_tags"])
	}
	if hasACLTag(captured["acl_tags"], "USER-1") {
		t.Fatalf("acl_tags should use normalized values: %#v", captured["acl_tags"])
	}
}

func TestSourceObjectClientNoopWhenBaseURLEmpty(t *testing.T) {
	t.Parallel()

	client := NewSourceObjectClient("", "")
	if err := client.UpsertSourceObject(t.Context(), store.Source{}, store.Item{}); err != nil {
		t.Fatalf("noop upsert error = %v", err)
	}
	if err := client.DeleteSourceObject(t.Context(), store.Source{}, store.Item{}); err != nil {
		t.Fatalf("noop delete error = %v", err)
	}
}

func hasACLTag(raw any, want string) bool {
	tags, ok := raw.([]any)
	if !ok {
		return false
	}
	for _, tag := range tags {
		if tag == want {
			return true
		}
	}
	return false
}
