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
	tokens := &fakeOrgTokenProvider{configured: true, tokens: []string{"token"}}
	if NewDocumentsClient("", tokens).Configured() {
		t.Error("empty baseURL must be unconfigured")
	}
	if NewDocumentsClient("http://x", nil).Configured() {
		t.Error("nil token provider must be unconfigured")
	}
	if !NewDocumentsClient("http://x", tokens).Configured() {
		t.Error("baseURL+token provider must be configured")
	}
	var nilClient *DocumentsClient
	if nilClient.Configured() {
		t.Error("nil client must be unconfigured")
	}
}

func TestDocumentsClient_UnconfiguredIsNoOp(t *testing.T) {
	// An unconfigured client must be a silent no-op (nil), never an error, so it
	// can be wired unconditionally.
	c := NewDocumentsClient("", nil)
	if err := c.CreateDocument(context.Background(), "org", CreateDocumentInput{Content: "x"}); err != nil {
		t.Fatalf("unconfigured client should no-op, got %v", err)
	}
}

func TestDocumentsClient_EmptyContentRejected(t *testing.T) {
	c := NewDocumentsClient("http://example.invalid", &fakeOrgTokenProvider{configured: true, tokens: []string{"token"}})
	err := c.CreateDocument(context.Background(), "org", CreateDocumentInput{Content: "   "})
	if err == nil {
		t.Fatal("empty content must be rejected before hitting the network")
	}
}

func TestDocumentsClient_CreateDocumentPostsExpectedRequest(t *testing.T) {
	var (
		gotPath          string
		gotAuthorization string
		gotIdentity      string
		gotPayload       map[string]any
	)
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		gotPath = r.URL.Path
		gotAuthorization = r.Header.Get("Authorization")
		gotIdentity = r.Header.Get("X-Org-ID") + r.Header.Get("X-User-ID") + r.Header.Get("X-Internal-Api-Key")
		body, _ := io.ReadAll(r.Body)
		_ = json.Unmarshal(body, &gotPayload)
		w.WriteHeader(http.StatusCreated)
		_, _ = w.Write([]byte(`{"document_id":"doc-1","status":"pending"}`))
	}))
	defer srv.Close()

	tokens := &fakeOrgTokenProvider{configured: true, tokens: []string{"service-token"}}
	c := NewDocumentsClient(srv.URL, tokens)
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
	if gotAuthorization != "Bearer service-token" {
		t.Errorf("Authorization = %q", gotAuthorization)
	}
	if len(tokens.orgs) != 1 || tokens.orgs[0] != "org-42" {
		t.Fatalf("token orgs = %#v, want verified org", tokens.orgs)
	}
	if gotIdentity != "" {
		t.Errorf("caller-selected identity/shared-key headers must be absent: %q", gotIdentity)
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

	c := NewDocumentsClient(srv.URL, &fakeOrgTokenProvider{configured: true, tokens: []string{"token"}})
	err := c.CreateDocument(context.Background(), "org", CreateDocumentInput{Content: "x"})
	if err == nil {
		t.Fatal("expected error on 400 response")
	}
}

func TestDocumentsClient_RetriesOnceWithFreshTokenOn401(t *testing.T) {
	requests := 0
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
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
	defer srv.Close()

	tokens := &fakeOrgTokenProvider{configured: true, tokens: []string{"stale-token", "fresh-token"}}
	c := NewDocumentsClient(srv.URL, tokens)
	err := c.CreateDocument(context.Background(), "org-1", CreateDocumentInput{Content: "real content"})
	if err != nil {
		t.Fatalf("CreateDocument: %v", err)
	}
	if requests != 2 || len(tokens.invalidated) != 1 || tokens.invalidated[0] != "org-1:stale-token" {
		t.Fatalf("requests=%d invalidated=%#v", requests, tokens.invalidated)
	}
}
