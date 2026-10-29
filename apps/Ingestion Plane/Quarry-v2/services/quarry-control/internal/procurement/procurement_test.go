package procurement

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
)

func TestSearchTEDPostsBoundedPublicNoticeQuery(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.Method != http.MethodPost || r.URL.Path != "/v3/notices/search" {
			t.Fatalf("request = %s %s", r.Method, r.URL.Path)
		}
		var body TEDSearchRequest
		if err := json.NewDecoder(r.Body).Decode(&body); err != nil {
			t.Fatalf("decode request: %v", err)
		}
		if body.Query != "CY=NO" || body.Limit != 25 || body.PaginationMode != "PAGE_NUMBER" {
			t.Fatalf("request body = %+v", body)
		}
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{"totalNoticeCount":1,"notices":[{"publication-number":"001-2026"}]}`))
	}))
	defer server.Close()

	client := NewClient(server.Client(), server.URL+"/v3/notices/search", server.URL)
	snapshot, err := client.SearchTED(context.Background(), TEDSearchRequest{Query: "CY=NO", Fields: []string{"publication-number"}, Limit: 25, PaginationMode: "PAGE_NUMBER"})
	if err != nil {
		t.Fatalf("SearchTED() error = %v", err)
	}
	if snapshot.Provider != "ted" || snapshot.QueryHash == "" || snapshot.ContentHash == "" {
		t.Fatalf("snapshot metadata incomplete: %+v", snapshot)
	}
}

func TestParseDoffinCSVAndDiffsVersionsAndRemovals(t *testing.T) {
	previous, err := ParseDoffinCSV([]byte("KunngjøringsID;Tittel;Status\nold-1;Old notice;PUBLISHED\nchanged-1;Old title;PUBLISHED\n"), "https://example.test/old.csv")
	if err != nil {
		t.Fatalf("parse previous: %v", err)
	}
	current, err := ParseDoffinCSV([]byte("KunngjøringsID;Tittel;Status\nchanged-1;New title;PUBLISHED\nnew-1;New notice;PUBLISHED\n"), "https://example.test/current.csv")
	if err != nil {
		t.Fatalf("parse current: %v", err)
	}

	diff := DiffDoffin(previous, current)
	if len(diff.Added) != 1 || diff.Added[0].ID != "new-1" {
		t.Fatalf("added = %+v", diff.Added)
	}
	if len(diff.Updated) != 1 || diff.Updated[0].ID != "changed-1" {
		t.Fatalf("updated = %+v", diff.Updated)
	}
	if len(diff.Removed) != 1 || diff.Removed[0] != "old-1" {
		t.Fatalf("removed = %+v", diff.Removed)
	}
}

func TestDoffinParserRejectsMissingStableIDAndOversizedRows(t *testing.T) {
	if _, err := ParseDoffinCSV([]byte("Tittel;Status\nNotice;PUBLISHED\n"), "https://example.test/file.csv"); err == nil {
		t.Fatal("ParseDoffinCSV() error = nil without stable ID column")
	}
	if _, err := ParseDoffinCSV([]byte("KunngjøringsID;Tittel\n1;"+strings.Repeat("x", 10001)+"\n"), "https://example.test/file.csv"); err == nil {
		t.Fatal("ParseDoffinCSV() error = nil for oversized field")
	}
}

func TestProcurementClientsRejectUnboundedInputs(t *testing.T) {
	client := NewClient(http.DefaultClient, "http://127.0.0.1:1", "http://127.0.0.1:1")
	if _, err := client.SearchTED(context.Background(), TEDSearchRequest{Query: strings.Repeat("x", 2001), Limit: 1}); err == nil {
		t.Fatal("SearchTED() error = nil for oversized query")
	}
	if _, err := client.FetchDoffinYear(context.Background(), 1999); err == nil {
		t.Fatal("FetchDoffinYear() error = nil for invalid year")
	}
}
