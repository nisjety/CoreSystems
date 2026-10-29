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
		if got := r.Header.Get("Authorization"); got != "Bearer service-token" {
			t.Fatalf("Authorization = %q", got)
		}
		if got := r.Header.Get("X-Org-ID") + r.Header.Get("X-User-ID") + r.Header.Get("X-Internal-Api-Key"); got != "" {
			t.Fatalf("caller-selected identity/shared-key headers must be absent: %q", got)
		}
		if err := json.NewDecoder(r.Body).Decode(&captured); err != nil {
			t.Fatalf("decode body: %v", err)
		}
		w.WriteHeader(http.StatusCreated)
	}))
	defer server.Close()

	modified := time.Date(2026, time.May, 26, 12, 0, 0, 0, time.UTC)
	tokens := &fakeOrgTokenProvider{configured: true, tokens: []string{"service-token"}}
	client := NewSourceObjectClient(server.URL, tokens)
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
	if len(tokens.orgs) != 1 || tokens.orgs[0] != "org-1" {
		t.Fatalf("token orgs = %#v, want org-1", tokens.orgs)
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

	client := NewSourceObjectClient("", nil)
	if err := client.UpsertSourceObject(t.Context(), store.Source{}, store.Item{}); err != nil {
		t.Fatalf("noop upsert error = %v", err)
	}
	if err := client.DeleteSourceObject(t.Context(), store.Source{}, store.Item{}); err != nil {
		t.Fatalf("noop delete error = %v", err)
	}
}

func TestSourceObjectClientRetriesOnceWithFreshTokenOn401(t *testing.T) {
	requests := 0
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		requests++
		if requests == 1 {
			w.WriteHeader(http.StatusUnauthorized)
			return
		}
		if got := r.Header.Get("Authorization"); got != "Bearer fresh-token" {
			t.Fatalf("retry Authorization = %q", got)
		}
		w.WriteHeader(http.StatusCreated)
	}))
	defer server.Close()

	tokens := &fakeOrgTokenProvider{configured: true, tokens: []string{"stale-token", "fresh-token"}}
	client := NewSourceObjectClient(server.URL, tokens)
	err := client.UpsertSourceObject(t.Context(), store.Source{OrganizationID: "org-1"}, store.Item{})
	if err != nil {
		t.Fatalf("UpsertSourceObject: %v", err)
	}
	if requests != 2 || len(tokens.invalidated) != 1 || tokens.invalidated[0] != "org-1:stale-token" {
		t.Fatalf("requests=%d invalidated=%#v", requests, tokens.invalidated)
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
