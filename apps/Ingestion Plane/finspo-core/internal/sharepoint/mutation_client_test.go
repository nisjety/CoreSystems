package sharepoint

import (
	"context"
	"encoding/json"
	"io"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
)

func TestMutationClientDeleteItem(t *testing.T) {
	t.Parallel()

	var gotMethod, gotPath, gotAuth string
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		gotMethod = r.Method
		gotPath = r.URL.Path
		gotAuth = r.Header.Get("Authorization")
		w.WriteHeader(http.StatusNoContent)
	}))
	defer server.Close()

	client := NewMutationClient(MutationClientConfig{BaseURL: server.URL, TokenProvider: staticTokenProvider{token: "tok"}})
	if err := client.DeleteItem(context.Background(), "org-1", "d1", "i1"); err != nil {
		t.Fatalf("DeleteItem: %v", err)
	}
	if gotMethod != http.MethodDelete {
		t.Errorf("method = %q, want DELETE", gotMethod)
	}
	if !strings.HasSuffix(gotPath, "/drives/d1/items/i1") {
		t.Errorf("path = %q", gotPath)
	}
	if gotAuth != "Bearer tok" {
		t.Errorf("auth = %q", gotAuth)
	}
}

func TestMutationClientDeletePropagatesGraphError(t *testing.T) {
	t.Parallel()

	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		http.Error(w, `{"error":{"code":"accessDenied"}}`, http.StatusForbidden)
	}))
	defer server.Close()

	client := NewMutationClient(MutationClientConfig{BaseURL: server.URL, TokenProvider: staticTokenProvider{token: "tok"}})
	err := client.DeleteItem(context.Background(), "org-1", "d1", "i1")
	if err == nil {
		t.Fatal("expected error on 403, got nil")
	}
	if !strings.Contains(err.Error(), "403") {
		t.Errorf("error = %v, want 403 surfaced", err)
	}
}

func TestMutationClientMoveItem(t *testing.T) {
	t.Parallel()

	var gotMethod string
	var gotBody map[string]any
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		gotMethod = r.Method
		raw, _ := io.ReadAll(r.Body)
		_ = json.Unmarshal(raw, &gotBody)
		w.WriteHeader(http.StatusOK)
		_, _ = w.Write([]byte(`{"id":"i1"}`))
	}))
	defer server.Close()

	client := NewMutationClient(MutationClientConfig{BaseURL: server.URL, TokenProvider: staticTokenProvider{token: "tok"}})
	if err := client.MoveItem(context.Background(), "org-1", "d1", "i1", "archive-folder"); err != nil {
		t.Fatalf("MoveItem: %v", err)
	}
	if gotMethod != http.MethodPatch {
		t.Errorf("method = %q, want PATCH", gotMethod)
	}
	parent, ok := gotBody["parentReference"].(map[string]any)
	if !ok || parent["id"] != "archive-folder" {
		t.Errorf("body parentReference = %#v", gotBody["parentReference"])
	}
}

func TestMutationClientMoveRejectsEmptyDestination(t *testing.T) {
	t.Parallel()

	client := NewMutationClient(MutationClientConfig{BaseURL: "http://unused", TokenProvider: staticTokenProvider{token: "tok"}})
	if err := client.MoveItem(context.Background(), "org-1", "d1", "i1", ""); err == nil {
		t.Fatal("expected error on empty destination, got nil")
	}
}

func TestMutationClientRequiresToken(t *testing.T) {
	t.Parallel()

	client := NewMutationClient(MutationClientConfig{BaseURL: "http://unused"})
	if err := client.DeleteItem(context.Background(), "org-1", "d1", "i1"); err == nil {
		t.Fatal("expected ErrNotConfigured, got nil")
	}
}
