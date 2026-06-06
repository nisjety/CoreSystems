package handoff

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"testing"
)

func TestDataPlaneCreateDocumentMapsInternalHeaders(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.Method != http.MethodPost {
			t.Fatalf("method = %s, want POST", r.Method)
		}
		if r.URL.Path != "/internal/v1/documents" {
			t.Fatalf("path = %s, want /internal/v1/documents", r.URL.Path)
		}
		if got := r.Header.Get("X-Internal-Api-Key"); got != "data-plane-key" {
			t.Fatalf("X-Internal-Api-Key = %q, want data-plane-key", got)
		}
		if got := r.Header.Get("X-Org-Id"); got != "org-1" {
			t.Fatalf("X-Org-Id = %q, want org-1", got)
		}
		var body DataPlaneDocumentRequest
		if err := json.NewDecoder(r.Body).Decode(&body); err != nil {
			t.Fatalf("decode body: %v", err)
		}
		if body.OrgID != "org-1" || body.Source != "github" || body.Type != "repository" {
			t.Fatalf("body = %#v, want org/source/type", body)
		}
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{"document_id":"doc-1","org_id":"org-1","source":"github","type":"repository","title":"repo-1","metadata":{"safe":true}}`))
	}))
	defer server.Close()

	client := NewDataPlaneDocumentsClient(server.URL, "data-plane-key", "", server.Client())
	document, err := client.CreateDocument(context.Background(), DataPlaneDocumentRequest{
		OrgID:    "org-1",
		Source:   "github",
		Type:     "repository",
		Title:    "repo-1",
		Metadata: map[string]any{"safe": true},
	})
	if err != nil {
		t.Fatalf("CreateDocument error = %v", err)
	}
	if document.DocumentID != "doc-1" || document.Title != "repo-1" {
		t.Fatalf("document = %#v, want doc-1 repo-1", document)
	}
}

func TestDataPlaneCreateDocumentAcceptsEnvelope(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{"success":true,"data":{"document_id":"doc-2","org_id":"org-1","source":"notion","type":"page","title":"Home"}}`))
	}))
	defer server.Close()

	client := NewDataPlaneDocumentsClient(server.URL, "data-plane-key", "", server.Client())
	document, err := client.CreateDocument(context.Background(), DataPlaneDocumentRequest{OrgID: "org-1", Source: "notion", Type: "page", Title: "Home"})
	if err != nil {
		t.Fatalf("CreateDocument error = %v", err)
	}
	if document.DocumentID != "doc-2" || document.Source != "notion" {
		t.Fatalf("document = %#v, want doc-2 notion", document)
	}
}
