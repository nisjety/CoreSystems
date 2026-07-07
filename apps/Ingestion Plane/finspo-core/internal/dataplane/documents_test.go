package dataplane

import (
	"context"
	"encoding/json"
	"io"
	"net/http"
	"net/http/httptest"
	"testing"
)

func TestDocumentsClient_Configured(t *testing.T) {
	if NewDocumentsClient("", "k").Configured() {
		t.Error("empty baseURL must be unconfigured")
	}
	if NewDocumentsClient("http://x", "").Configured() {
		t.Error("empty apiKey must be unconfigured")
	}
	if !NewDocumentsClient("http://x", "k").Configured() {
		t.Error("baseURL+apiKey must be configured")
	}
	var nilClient *DocumentsClient
	if nilClient.Configured() {
		t.Error("nil client must be unconfigured")
	}
}

func TestDocumentsClient_UnconfiguredIsNoOp(t *testing.T) {
	// An unconfigured client must be a silent no-op (nil), never an error, so it
	// can be wired unconditionally.
	c := NewDocumentsClient("", "")
	if err := c.CreateDocument(context.Background(), "org", CreateDocumentInput{Content: "x"}); err != nil {
		t.Fatalf("unconfigured client should no-op, got %v", err)
	}
}

func TestDocumentsClient_EmptyContentRejected(t *testing.T) {
	c := NewDocumentsClient("http://example.invalid", "key")
	err := c.CreateDocument(context.Background(), "org", CreateDocumentInput{Content: "   "})
	if err == nil {
		t.Fatal("empty content must be rejected before hitting the network")
	}
}

func TestDocumentsClient_CreateDocumentPostsExpectedRequest(t *testing.T) {
	var (
		gotPath    string
		gotOrg     string
		gotAPIKey  string
		gotPayload map[string]any
	)
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		gotPath = r.URL.Path
		gotOrg = r.Header.Get("X-Org-ID")
		gotAPIKey = r.Header.Get("X-Internal-Api-Key")
		body, _ := io.ReadAll(r.Body)
		_ = json.Unmarshal(body, &gotPayload)
		w.WriteHeader(http.StatusCreated)
		_, _ = w.Write([]byte(`{"document_id":"doc-1","status":"pending"}`))
	}))
	defer srv.Close()

	c := NewDocumentsClient(srv.URL, "secret-key")
	err := c.CreateDocument(context.Background(), "org-42", CreateDocumentInput{
		Source:            "sharepoint",
		Type:              "sharepoint_file",
		Title:             "Q3 Plan",
		Content:           "the body",
		ZDRClassification: "internal",
		Metadata:          map[string]any{"connector": "sharepoint"},
		IdempotencyKey:    "finspo-sp:drive:item",
	})
	if err != nil {
		t.Fatalf("CreateDocument: %v", err)
	}
	if gotPath != "/v1/documents" {
		t.Errorf("path = %q, want /v1/documents", gotPath)
	}
	if gotOrg != "org-42" {
		t.Errorf("X-Org-ID = %q", gotOrg)
	}
	if gotAPIKey != "secret-key" {
		t.Errorf("X-Internal-Api-Key = %q", gotAPIKey)
	}
	if gotPayload["org_id"] != "org-42" || gotPayload["content"] != "the body" {
		t.Errorf("payload org_id/content wrong: %+v", gotPayload)
	}
	if gotPayload["zdr_classification"] != "internal" {
		t.Errorf("zdr_classification not forwarded: %+v", gotPayload)
	}
	if gotPayload["idempotency_key"] != "finspo-sp:drive:item" {
		t.Errorf("idempotency_key not forwarded: %+v", gotPayload)
	}
}

func TestDocumentsClient_Non2xxIsError(t *testing.T) {
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.WriteHeader(http.StatusBadRequest)
		_, _ = w.Write([]byte(`{"error":"content is required"}`))
	}))
	defer srv.Close()

	c := NewDocumentsClient(srv.URL, "key")
	err := c.CreateDocument(context.Background(), "org", CreateDocumentInput{Content: "x"})
	if err == nil {
		t.Fatal("expected error on 400 response")
	}
}
