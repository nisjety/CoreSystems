package catalogwatch

import (
	"context"
	"net/http"
	"net/http/httptest"
	"net/url"
	"strings"
	"testing"
)

func TestClientSnapshotPostsFixedSPARQLQueryAndSortsResources(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.Method != http.MethodPost {
			t.Fatalf("method = %s, want POST", r.Method)
		}
		if got := r.Header.Get("Accept"); got != "application/sparql-results+json" {
			t.Fatalf("Accept = %q", got)
		}
		if err := r.ParseForm(); err != nil {
			t.Fatalf("parse form: %v", err)
		}
		query := r.Form.Get("query")
		if !strings.Contains(query, "SELECT ?dataset") || !strings.Contains(query, "dcat:Dataset") {
			t.Fatalf("query does not contain the catalog contract: %q", query)
		}

		w.Header().Set("Content-Type", "application/sparql-results+json")
		_, _ = w.Write([]byte(`{
  "head": {"vars": ["dataset", "title", "publisher", "modified", "access"]},
  "results": {"bindings": [
    {"dataset": {"type": "uri", "value": "https://data.norge.no/dataset/z"}, "title": {"type": "literal", "value": "Z dataset"}, "publisher": {"type": "literal", "value": "Publisher Z"}, "modified": {"type": "literal", "value": "2026-07-20"}, "access": {"type": "uri", "value": "https://example.test/z"}},
    {"dataset": {"type": "uri", "value": "https://data.norge.no/dataset/a"}, "title": {"type": "literal", "value": "A dataset"}, "publisher": {"type": "literal", "value": "Publisher A"}, "modified": {"type": "literal", "value": "2026-07-19"}}
  ]}
}`))
	}))
	defer server.Close()

	client := NewClient(server.Client(), server.URL)
	snapshot, err := client.Snapshot(context.Background())
	if err != nil {
		t.Fatalf("Snapshot() error = %v", err)
	}
	if len(snapshot.Resources) != 2 {
		t.Fatalf("resource count = %d, want 2", len(snapshot.Resources))
	}
	if snapshot.Resources[0].URI != "https://data.norge.no/dataset/a" {
		t.Fatalf("first resource = %q, want sorted URI", snapshot.Resources[0].URI)
	}
	if snapshot.Resources[1].AccessURL != "https://example.test/z" {
		t.Fatalf("access URL = %q", snapshot.Resources[1].AccessURL)
	}
	if snapshot.RetrievedAt.IsZero() {
		t.Fatal("RetrievedAt is zero")
	}
}

func TestClientSnapshotRejectsOversizedResponses(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		w.WriteHeader(http.StatusOK)
		_, _ = w.Write([]byte(strings.Repeat("x", 128)))
	}))
	defer server.Close()

	client := NewClient(server.Client(), server.URL)
	client.MaxResponseBytes = 32
	if _, err := client.Snapshot(context.Background()); err == nil {
		t.Fatal("Snapshot() error = nil, want oversized response error")
	}
}

func TestDiffSnapshotsDetectsAddedRemovedAndChangedResources(t *testing.T) {
	previous := Snapshot{Resources: []Resource{
		{URI: "https://data.norge.no/dataset/changed", Title: "old"},
		{URI: "https://data.norge.no/dataset/removed", Title: "removed"},
	}}
	current := Snapshot{Resources: []Resource{
		{URI: "https://data.norge.no/dataset/added", Title: "added"},
		{URI: "https://data.norge.no/dataset/changed", Title: "new"},
	}}

	diff := DiffSnapshots(previous, current)
	if got := diff.Added[0].URI; got != "https://data.norge.no/dataset/added" {
		t.Fatalf("added URI = %q", got)
	}
	if got := diff.Removed[0].URI; got != "https://data.norge.no/dataset/removed" {
		t.Fatalf("removed URI = %q", got)
	}
	if got := diff.Changed[0].Title; got != "new" {
		t.Fatalf("changed title = %q", got)
	}
}

func TestNewClientUsesDefaultEndpointWhenEndpointIsBlank(t *testing.T) {
	client := NewClient(nil, "")
	parsed, err := url.Parse(client.Endpoint)
	if err != nil {
		t.Fatalf("parse endpoint: %v", err)
	}
	if parsed.Host != "sparql.fellesdatakatalog.digdir.no" {
		t.Fatalf("endpoint host = %q", parsed.Host)
	}
}
