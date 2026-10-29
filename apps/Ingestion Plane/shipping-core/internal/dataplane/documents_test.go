package dataplane

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"testing"
)

func TestClient_NotConfigured_FailsFastWithoutRequest(t *testing.T) {
	c := New(Config{})
	if c.Configured() {
		t.Fatal("expected Configured()=false with empty Config")
	}
	if _, err := c.CreateDocument(context.Background(), DocumentRequest{Content: "x"}); err == nil {
		t.Fatal("expected an error when not configured")
	}
}

func TestClient_CreateDocument_RequiresContent(t *testing.T) {
	c := New(Config{BaseURL: "http://unused.invalid", AuthCoreURL: "http://auth.invalid", ServiceID: "shipping-core", ServiceCredential: "k"})
	if _, err := c.CreateDocument(context.Background(), DocumentRequest{OrgID: "org1"}); err == nil {
		t.Fatal("expected an error for empty Content")
	}
}

func TestClient_CreateDocument_SendsHeadersAndBody(t *testing.T) {
	var gotPath, gotAuth, gotOrgHeader string
	var gotBody DocumentRequest
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path == "/api/data-plane/internal-token" {
			if r.Header.Get("X-Service-ID") != "shipping-core" || r.Header.Get("X-Service-API-Key") != "service-key" {
				t.Fatalf("service credential headers missing")
			}
			_ = json.NewEncoder(w).Encode(map[string]string{"token": "data-plane-token"})
			return
		}
		gotPath = r.URL.Path
		gotAuth = r.Header.Get("Authorization")
		gotOrgHeader = r.Header.Get("X-Org-ID")
		_ = json.NewDecoder(r.Body).Decode(&gotBody)
		w.Header().Set("Content-Type", "application/json")
		w.WriteHeader(http.StatusCreated)
		_ = json.NewEncoder(w).Encode(Document{ID: "doc_1", DocumentID: "doc_1"})
	}))
	defer server.Close()

	c := New(Config{BaseURL: server.URL, AuthCoreURL: server.URL, ServiceID: "shipping-core", ServiceCredential: "service-key"})
	doc, err := c.CreateDocument(context.Background(), DocumentRequest{
		OrgID: "shipping-core-system", Source: "shipping-core", Type: "shipping-delivery",
		Title: "Booking b1 delivered", Content: "Booking b1 via Bring delivered on 2026-07-08, estimated 2026-07-08 (on time).",
	})
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if gotPath != "/v1/documents" {
		t.Errorf("path = %q, want /v1/documents", gotPath)
	}
	if gotAuth != "Bearer data-plane-token" {
		t.Errorf("Authorization = %q", gotAuth)
	}
	if gotOrgHeader != "shipping-core-system" {
		t.Errorf("X-Org-ID = %q", gotOrgHeader)
	}
	if gotBody.Content == "" {
		t.Error("request body missing Content")
	}
	if doc.ID != "doc_1" {
		t.Errorf("Document.ID = %q", doc.ID)
	}
}

func TestClient_CreateDocument_UpstreamErrorSurfaced(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path == "/api/data-plane/internal-token" {
			_ = json.NewEncoder(w).Encode(map[string]string{"token": "data-plane-token"})
			return
		}
		w.WriteHeader(http.StatusBadRequest)
		_, _ = w.Write([]byte(`{"error":"content is required"}`))
	}))
	defer server.Close()

	c := New(Config{BaseURL: server.URL, AuthCoreURL: server.URL, ServiceID: "shipping-core", ServiceCredential: "service-key"})
	_, err := c.CreateDocument(context.Background(), DocumentRequest{Content: "real content"})
	if err == nil {
		t.Fatal("expected an error for a 400 response")
	}
}
