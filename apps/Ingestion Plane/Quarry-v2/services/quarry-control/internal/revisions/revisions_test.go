package revisions

import (
	"context"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
)

func TestFetchLovdataSearchCapturesBoundedRevisionSnapshot(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/v1/search" || r.Header.Get("X-API-Key") != "test-key" {
			t.Fatalf("request = %s %s, api-key=%q", r.Method, r.URL.String(), r.Header.Get("X-API-Key"))
		}
		query := r.URL.Query()
		if query.Get("emne1") != "arbeidsmiljø" || query.Get("rows") != "10" || query.Get("offset") != "0" {
			t.Fatalf("query = %v", query)
		}
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{"results":[{"id":"LOV-2005-06-17-62"}]}`))
	}))
	defer server.Close()

	client := NewClient(server.Client(), server.URL+"/v1/search", server.URL+"/eksport", "test-key")
	snapshot, err := client.FetchLovdataSearch(context.Background(), LovdataSearchRequest{Terms: []string{"arbeidsmiljø"}})
	if err != nil {
		t.Fatalf("FetchLovdataSearch() error = %v", err)
	}
	if snapshot.Provider != "lovdata" || snapshot.Dataset != "current-laws-and-central-regulations" {
		t.Fatalf("source = %s/%s", snapshot.Provider, snapshot.Dataset)
	}
	if snapshot.ContentHash == "" || snapshot.QueryHash == "" || snapshot.RetrievedAt.IsZero() {
		t.Fatalf("snapshot metadata incomplete: %+v", snapshot)
	}
}

func TestFetchStortingExportPreservesXMLVersionAndFormat(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/eksport/sak" || r.URL.Query().Get("sakid") != "12345" || r.URL.Query().Get("format") != "XML" {
			t.Fatalf("request path/query = %s", r.URL.String())
		}
		w.Header().Set("Content-Type", "application/xml")
		_, _ = w.Write([]byte(`<sak><versjon>2.4</versjon><sak_id>12345</sak_id></sak>`))
	}))
	defer server.Close()

	client := NewClient(server.Client(), server.URL+"/v1/search", server.URL+"/eksport", "")
	snapshot, err := client.FetchStortingExport(context.Background(), StortingExportRequest{
		Resource: "sak",
		Params:   map[string]string{"sakid": "12345"},
		Format:   "XML",
	})
	if err != nil {
		t.Fatalf("FetchStortingExport() error = %v", err)
	}
	if snapshot.Version != "2.4" || snapshot.ContentType != "application/xml" {
		t.Fatalf("version/content type = %q/%q", snapshot.Version, snapshot.ContentType)
	}
	if snapshot.ResourceID != "12345" {
		t.Fatalf("resource id = %q", snapshot.ResourceID)
	}
}

func TestRevisionCollectorsRejectUnboundedOrUnapprovedRequests(t *testing.T) {
	client := NewClient(http.DefaultClient, "http://127.0.0.1:1", "http://127.0.0.1:1", "")
	if _, err := client.FetchLovdataSearch(context.Background(), LovdataSearchRequest{Terms: []string{strings.Repeat("x", 101)}}); err == nil {
		t.Fatal("FetchLovdataSearch() error = nil for oversized term")
	}
	if _, err := client.FetchStortingExport(context.Background(), StortingExportRequest{Resource: "arbitrary"}); err == nil {
		t.Fatal("FetchStortingExport() error = nil for unapproved resource")
	}
	if _, err := client.FetchStortingExport(context.Background(), StortingExportRequest{Resource: "sak", Format: "YAML"}); err == nil {
		t.Fatal("FetchStortingExport() error = nil for invalid format")
	}
}

func TestRevisionCollectorsRejectOversizedPayloads(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		_, _ = w.Write([]byte(strings.Repeat("x", 64)))
	}))
	defer server.Close()

	client := NewClient(server.Client(), server.URL+"/v1/search", server.URL+"/eksport", "test-key")
	client.MaxResponseBytes = 32
	_, err := client.FetchLovdataSearch(context.Background(), LovdataSearchRequest{Terms: []string{"law"}})
	if err == nil {
		t.Fatal("FetchLovdataSearch() error = nil for oversized payload")
	}
}
