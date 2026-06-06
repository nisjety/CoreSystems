package workers

import (
	"context"
	"errors"
	"net/http"
	"testing"

	"github.com/triodelab/integration-corev2/internal/handoff"
	"github.com/triodelab/integration-corev2/internal/store"
)

func TestFinspoWorkerProcessesClaimedMicrosoftJob(t *testing.T) {
	integration := &fakeIntegrationClient{
		claim: store.SyncJob{
			ID:             "sync-1",
			OrganizationID: "org-1",
			UserID:         "user-1",
			ProviderKey:    "microsoft",
			Checkpoint: map[string]any{
				"site_id":      "site-1",
				"drive_id":     "drive-1",
				"site_web_url": "https://contoso.sharepoint.com/sites/support",
				"drive_name":   "Support",
			},
		},
	}
	finspo := &fakeFinspoClient{
		source: handoff.FinspoSource{ID: "finspo-source-1", Status: "ready"},
		sync:   handoff.FinspoSyncResult{SourceID: "finspo-source-1", Status: "queued", JobID: "finspo-job-1"},
	}
	processed, err := FinspoWorker{Integration: integration, Finspo: finspo}.RunOnce(context.Background())
	if err != nil {
		t.Fatalf("RunOnce error = %v", err)
	}
	if !processed {
		t.Fatal("RunOnce processed = false, want true")
	}
	if integration.claimRequest.Consumer != "finspo-core" || integration.claimRequest.Target != "finspo-core" || integration.claimRequest.ProviderKey != "microsoft" {
		t.Fatalf("claimRequest = %#v, want finspo microsoft claim", integration.claimRequest)
	}
	if finspo.ensureRequest.SiteID != "site-1" || finspo.ensureRequest.DriveID != "drive-1" || finspo.ensureOrgID != "org-1" {
		t.Fatalf("ensure request = %#v org=%q, want source ids and org", finspo.ensureRequest, finspo.ensureOrgID)
	}
	if finspo.syncSourceID != "finspo-source-1" {
		t.Fatalf("syncSourceID = %q, want finspo-source-1", finspo.syncSourceID)
	}
	if integration.progressRequest.Status != "completed" || len(integration.progressRequest.Sources) != 1 {
		t.Fatalf("progressRequest = %#v, want completed with source ref", integration.progressRequest)
	}
	if integration.progressRequest.Checkpoint["finspoSyncJobId"] != "finspo-job-1" {
		t.Fatalf("checkpoint = %#v, want finspo job id", integration.progressRequest.Checkpoint)
	}
}

func TestFinspoWorkerIdlesWhenNoJobAvailable(t *testing.T) {
	integration := &fakeIntegrationClient{claimErr: &handoff.ServiceHTTPError{Service: "integration-core", StatusCode: http.StatusNotFound}}
	processed, err := FinspoWorker{Integration: integration, Finspo: &fakeFinspoClient{}}.RunOnce(context.Background())
	if err != nil {
		t.Fatalf("RunOnce error = %v", err)
	}
	if processed {
		t.Fatal("RunOnce processed = true, want false")
	}
}

func TestFinspoWorkerFailsJobWhenSourceIdentifiersMissing(t *testing.T) {
	integration := &fakeIntegrationClient{
		claim: store.SyncJob{
			ID:             "sync-1",
			OrganizationID: "org-1",
			ProviderKey:    "microsoft",
			Checkpoint:     map[string]any{"site_id": "site-1"},
		},
	}
	processed, err := FinspoWorker{Integration: integration, Finspo: &fakeFinspoClient{}}.RunOnce(context.Background())
	if err == nil {
		t.Fatal("RunOnce error = nil, want missing identifiers error")
	}
	if !processed {
		t.Fatal("RunOnce processed = false, want true")
	}
	if integration.progressRequest.Status != "failed" {
		t.Fatalf("progress status = %q, want failed", integration.progressRequest.Status)
	}
}

type fakeIntegrationClient struct {
	claim           store.SyncJob
	claimErr        error
	progressErr     error
	claimRequest    handoff.SyncClaimRequest
	progressJobID   string
	progressRequest handoff.SyncProgressRequest
}

func (c *fakeIntegrationClient) ClaimSyncJob(_ context.Context, input handoff.SyncClaimRequest) (store.SyncJob, error) {
	c.claimRequest = input
	if c.claimErr != nil {
		return store.SyncJob{}, c.claimErr
	}
	return c.claim, nil
}

func (c *fakeIntegrationClient) UpdateSyncProgress(_ context.Context, jobID string, input handoff.SyncProgressRequest) (store.SyncJob, error) {
	c.progressJobID = jobID
	c.progressRequest = input
	if c.progressErr != nil {
		return store.SyncJob{}, c.progressErr
	}
	if c.claim.ID == "" {
		return store.SyncJob{}, errors.New("missing claimed job")
	}
	c.claim.Status = input.Status
	return c.claim, nil
}

type fakeFinspoClient struct {
	source        handoff.FinspoSource
	sync          handoff.FinspoSyncResult
	ensureErr     error
	syncErr       error
	ensureOrgID   string
	ensureUserID  string
	ensureRequest handoff.FinspoSourceRequest
	syncSourceID  string
}

func (c *fakeFinspoClient) EnsureSource(_ context.Context, orgID, userID string, input handoff.FinspoSourceRequest) (handoff.FinspoSource, error) {
	c.ensureOrgID = orgID
	c.ensureUserID = userID
	c.ensureRequest = input
	if c.ensureErr != nil {
		return handoff.FinspoSource{}, c.ensureErr
	}
	return c.source, nil
}

func (c *fakeFinspoClient) SyncSource(_ context.Context, _ string, _ string, sourceID string) (handoff.FinspoSyncResult, error) {
	c.syncSourceID = sourceID
	if c.syncErr != nil {
		return handoff.FinspoSyncResult{}, c.syncErr
	}
	return c.sync, nil
}
