package handoff

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"testing"
)

func TestIntegrationClaimAndProgressUseInternalAuth(t *testing.T) {
	var progressSeen bool
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if got := r.Header.Get("X-Internal-API-Key"); got != "internal-key" {
			t.Fatalf("X-Internal-API-Key = %q, want internal-key", got)
		}
		w.Header().Set("Content-Type", "application/json")
		switch {
		case r.Method == http.MethodPost && r.URL.Path == "/internal/sync-jobs/claim":
			var body SyncClaimRequest
			if err := json.NewDecoder(r.Body).Decode(&body); err != nil {
				t.Fatalf("decode claim body: %v", err)
			}
			if body.Consumer != "data-plane-v2" || body.Target != "data-plane-v2" || body.OrganizationID != "org-1" {
				t.Fatalf("claim body = %#v, want data-plane-v2 org-1", body)
			}
			_, _ = w.Write([]byte(`{"success":true,"data":{"syncJob":{"id":"sync-1","organizationId":"org-1","providerKey":"github","status":"running"}}}`))
		case r.Method == http.MethodPatch && r.URL.Path == "/internal/sync-jobs/sync-1/progress":
			progressSeen = true
			var body SyncProgressRequest
			if err := json.NewDecoder(r.Body).Decode(&body); err != nil {
				t.Fatalf("decode progress body: %v", err)
			}
			if body.Consumer != "data-plane-v2" || body.Status != "completed" || len(body.Sources) != 1 {
				t.Fatalf("progress body = %#v, want completed with source", body)
			}
			_, _ = w.Write([]byte(`{"success":true,"data":{"syncJob":{"id":"sync-1","organizationId":"org-1","providerKey":"github","status":"completed"}}}`))
		default:
			t.Fatalf("unexpected request: %s %s", r.Method, r.URL.Path)
		}
	}))
	defer server.Close()

	client := NewIntegrationClient(server.URL, "internal-key", "", server.Client())
	claimed, err := client.ClaimSyncJob(context.Background(), SyncClaimRequest{
		Consumer:       "data-plane-v2",
		Target:         "data-plane-v2",
		OrganizationID: "org-1",
		ProviderKey:    "github",
	})
	if err != nil {
		t.Fatalf("ClaimSyncJob error = %v", err)
	}
	if claimed.ID != "sync-1" || claimed.Status != "running" {
		t.Fatalf("claimed = %#v, want sync-1 running", claimed)
	}

	updated, err := client.UpdateSyncProgress(context.Background(), "sync-1", SyncProgressRequest{
		Consumer: "data-plane-v2",
		Status:   "completed",
		Sources: []SyncSourceRef{{
			Provider: "github",
			Type:     "repository",
			SourceID: "repo-1",
			Status:   "ready",
		}},
	})
	if err != nil {
		t.Fatalf("UpdateSyncProgress error = %v", err)
	}
	if !progressSeen {
		t.Fatal("progress request was not seen")
	}
	if updated.Status != "completed" {
		t.Fatalf("updated status = %q, want completed", updated.Status)
	}
}
