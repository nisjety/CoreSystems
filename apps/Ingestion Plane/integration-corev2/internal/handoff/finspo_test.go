package handoff

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
)

func TestFinspoEnsureSourceMapsHeadersAndBody(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.Method != http.MethodPost {
			t.Fatalf("method = %s, want POST", r.Method)
		}
		if r.URL.Path != "/api/v1/sources" {
			t.Fatalf("path = %s, want /api/v1/sources", r.URL.Path)
		}
		if got := r.Header.Get("X-API-Key"); got != "finspo-secret" {
			t.Fatalf("X-API-Key = %q, want finspo-secret", got)
		}
		if got := r.Header.Get("X-Org-ID"); got != "org-1" {
			t.Fatalf("X-Org-ID = %q, want org-1", got)
		}
		if got := r.Header.Get("X-User-ID"); got != "user-1" {
			t.Fatalf("X-User-ID = %q, want user-1", got)
		}
		var body FinspoSourceRequest
		if err := json.NewDecoder(r.Body).Decode(&body); err != nil {
			t.Fatalf("decode body: %v", err)
		}
		if body.SiteID != "site-1" || body.DriveID != "drive-1" || body.TenantID != "tenant-1" {
			t.Fatalf("body = %#v, want site/drive/tenant", body)
		}
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{"success":true,"data":{"id":"source-1","site_id":"site-1","drive_id":"drive-1","status":"ready"}}`))
	}))
	defer server.Close()

	client := NewFinspoClient(server.URL, "finspo-secret", "X-API-Key", server.Client())
	source, err := client.EnsureSource(context.Background(), "org-1", "user-1", FinspoSourceRequest{
		SiteID:   "site-1",
		DriveID:  "drive-1",
		TenantID: "tenant-1",
	})
	if err != nil {
		t.Fatalf("EnsureSource error = %v", err)
	}
	if source.ID != "source-1" || source.Status != "ready" {
		t.Fatalf("source = %#v, want source-1 ready", source)
	}
}

func TestFinspoSyncSourceMapsPath(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.EscapedPath() != "/api/v1/sources/source%2F1/sync" {
			t.Fatalf("path = %s, want escaped source sync path", r.URL.EscapedPath())
		}
		if got := r.Header.Get("X-API-Key"); got != "finspo-secret" {
			t.Fatalf("X-API-Key = %q, want finspo-secret", got)
		}
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{"success":true,"data":{"source_id":"source/1","status":"queued","job_id":"finspo-job-1"}}`))
	}))
	defer server.Close()

	client := NewFinspoClient(server.URL, "finspo-secret", "", server.Client())
	result, err := client.SyncSource(context.Background(), "org-1", "", "source/1")
	if err != nil {
		t.Fatalf("SyncSource error = %v", err)
	}
	if result.SourceID != "source/1" || result.JobID != "finspo-job-1" {
		t.Fatalf("result = %#v, want source/1 job", result)
	}
}

func TestFinspoErrorsDoNotLeakAPIKey(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		http.Error(w, "finspo-secret should not appear in caller error", http.StatusForbidden)
	}))
	defer server.Close()

	client := NewFinspoClient(server.URL, "finspo-secret", "", server.Client())
	_, err := client.EnsureSource(context.Background(), "org-1", "user-1", FinspoSourceRequest{SiteID: "site-1", DriveID: "drive-1"})
	if err == nil {
		t.Fatal("EnsureSource error = nil, want error")
	}
	if got := err.Error(); got == "" || strings.Contains(got, "finspo-secret") {
		t.Fatalf("error = %q, want redacted service error", got)
	}
}
