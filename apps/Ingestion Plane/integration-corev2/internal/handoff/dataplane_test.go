package handoff

import (
	"context"
	"encoding/json"
	"errors"
	"net/http"
	"net/http/httptest"
	"testing"
)

func TestDataPlaneCreateDocumentMapsInternalHeaders(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.Method != http.MethodPost {
			t.Fatalf("method = %s, want POST", r.Method)
		}
		if r.URL.Path != "/v1/documents" {
			t.Fatalf("path = %s, want /v1/documents", r.URL.Path)
		}
		if got := r.Header.Get("X-Internal-Api-Key"); got != "data-plane-key" {
			t.Fatalf("X-Internal-Api-Key = %q, want data-plane-key", got)
		}
		if got := r.Header.Get("X-Org-ID"); got != "org-1" {
			t.Fatalf("X-Org-ID = %q, want org-1", got)
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

func TestDataPlaneCreateDocumentReturnsErrNotConfigured(t *testing.T) {
	tests := []struct {
		name   string
		client *DataPlaneDocumentsClient
	}{
		{name: "nil client", client: nil},
		{name: "missing base URL", client: NewDataPlaneDocumentsClient("", "data-plane-key", "", nil)},
		{name: "missing internal api key", client: NewDataPlaneDocumentsClient("http://dpv2-documents-api:8010", "", "", nil)},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			_, err := tt.client.CreateDocument(context.Background(), DataPlaneDocumentRequest{OrgID: "org-1", Source: "github", Type: "repository", Title: "repo-1"})
			if !errors.Is(err, ErrNotConfigured) {
				t.Fatalf("CreateDocument error = %v, want ErrNotConfigured", err)
			}
		})
	}
}

func TestDataPlaneDocumentsClientConfigured(t *testing.T) {
	tests := []struct {
		name   string
		client *DataPlaneDocumentsClient
		want   bool
	}{
		{name: "nil client", client: nil, want: false},
		{name: "missing base URL", client: NewDataPlaneDocumentsClient("", "data-plane-key", "", nil), want: false},
		{name: "missing internal api key", client: NewDataPlaneDocumentsClient("http://dpv2-documents-api:8010", "", "", nil), want: false},
		{name: "fully configured", client: NewDataPlaneDocumentsClient("http://dpv2-documents-api:8010", "data-plane-key", "", nil), want: true},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			if got := tt.client.Configured(); got != tt.want {
				t.Fatalf("Configured() = %v, want %v", got, tt.want)
			}
		})
	}
}
