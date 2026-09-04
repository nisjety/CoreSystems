package workers

import (
	"bytes"
	"context"
	"errors"
	"net/http"
	"strings"
	"testing"

	"github.com/rs/zerolog"

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

func TestFinspoWorkerProcessSkipsDataPlaneForwardWhenClientNil(t *testing.T) {
	// FinspoWorker.DataPlane is nil by default (not set). RunOnce must still
	// succeed and must not panic on a nil DataPlane client.
	integration := &fakeIntegrationClient{
		claim: store.SyncJob{
			ID:             "sync-1",
			OrganizationID: "org-1",
			UserID:         "user-1",
			ProviderKey:    "microsoft",
			Checkpoint: map[string]any{
				"site_id":  "site-1",
				"drive_id": "drive-1",
			},
		},
	}
	finspo := &fakeFinspoClient{
		source: handoff.FinspoSource{ID: "finspo-source-1", Status: "ready"},
		sync:   handoff.FinspoSyncResult{SourceID: "finspo-source-1", Status: "queued", JobID: "finspo-job-1"},
	}
	processed, err := FinspoWorker{Integration: integration, Finspo: finspo}.RunOnce(context.Background())
	if err != nil {
		t.Fatalf("RunOnce error = %v, want nil with DataPlane unset", err)
	}
	if !processed {
		t.Fatal("RunOnce processed = false, want true")
	}
}

func TestFinspoWorkerProcessSkipsDataPlaneForwardWhenNotConfigured(t *testing.T) {
	var buf bytes.Buffer
	logger := zerolog.New(&buf)
	integration := &fakeIntegrationClient{
		claim: store.SyncJob{
			ID:             "sync-1",
			OrganizationID: "org-1",
			UserID:         "user-1",
			ProviderKey:    "microsoft",
			Checkpoint: map[string]any{
				"site_id":  "site-1",
				"drive_id": "drive-1",
			},
		},
	}
	finspo := &fakeFinspoClient{
		source: handoff.FinspoSource{ID: "finspo-source-1", Status: "ready"},
		sync:   handoff.FinspoSyncResult{SourceID: "finspo-source-1", Status: "queued", JobID: "finspo-job-1"},
	}
	dataPlane := &fakeDataPlaneClient{configured: false}
	processed, err := FinspoWorker{Integration: integration, Finspo: finspo, DataPlane: dataPlane, Logger: &logger}.RunOnce(context.Background())
	if err != nil {
		t.Fatalf("RunOnce error = %v", err)
	}
	if !processed {
		t.Fatal("RunOnce processed = false, want true")
	}
	if !dataPlane.configuredCalled {
		t.Fatal("Configured() was not called, want the worker to check before forwarding")
	}
	if strings.Contains(buf.String(), "Data Plane documents client is configured") {
		t.Fatalf("log output = %q, want no Data Plane forward log when unconfigured", buf.String())
	}
}

func TestFinspoWorkerProcessLogsSkippedDataPlaneForwardWhenConfigured(t *testing.T) {
	var buf bytes.Buffer
	logger := zerolog.New(&buf)
	integration := &fakeIntegrationClient{
		claim: store.SyncJob{
			ID:             "sync-1",
			OrganizationID: "org-1",
			UserID:         "user-1",
			ProviderKey:    "microsoft",
			Checkpoint: map[string]any{
				"site_id":  "site-1",
				"drive_id": "drive-1",
			},
		},
	}
	finspo := &fakeFinspoClient{
		source: handoff.FinspoSource{ID: "finspo-source-1", Status: "ready"},
		sync:   handoff.FinspoSyncResult{SourceID: "finspo-source-1", Status: "queued", JobID: "finspo-job-1"},
	}
	dataPlane := &fakeDataPlaneClient{configured: true}
	processed, err := FinspoWorker{Integration: integration, Finspo: finspo, DataPlane: dataPlane, Logger: &logger}.RunOnce(context.Background())
	if err != nil {
		t.Fatalf("RunOnce error = %v", err)
	}
	if !processed {
		t.Fatal("RunOnce processed = false, want true")
	}
	if !dataPlane.configuredCalled {
		t.Fatal("Configured() was not called")
	}
	logged := buf.String()
	if !strings.Contains(logged, "Data Plane documents client is configured but skipped") {
		t.Fatalf("log output = %q, want an explicit skipped-forward log line", logged)
	}
	if !strings.Contains(logged, "finspo-source-1") {
		t.Fatalf("log output = %q, want the source id included", logged)
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

// TestFinspoWorkerFansOutOverRegisteredSourcesWhenJobHasNoSiteDrive guards
// the Synkroniser-button path: a generic per-connection sync job (no
// site_id/drive_id metadata) must sync every source the org registered in
// finspo-core instead of failing outright — the pre-fallback behavior left
// every manual sync dead with "requires SharePoint site_id and drive_id".
func TestFinspoWorkerFansOutOverRegisteredSourcesWhenJobHasNoSiteDrive(t *testing.T) {
	integration := &fakeIntegrationClient{
		claim: store.SyncJob{
			ID:             "sync-2",
			OrganizationID: "org-1",
			UserID:         "user-1",
			ProviderKey:    "microsoft",
		},
	}
	finspo := &fakeFinspoClient{
		listSources: []handoff.FinspoSource{
			{ID: "src-a", DriveID: "drive-a", DriveName: "Documents"},
			{ID: "src-b", DriveID: "drive-b", DriveName: "Policies"},
		},
		sync: handoff.FinspoSyncResult{Status: "queued"},
	}
	processed, err := FinspoWorker{Integration: integration, Finspo: finspo}.RunOnce(context.Background())
	if err != nil {
		t.Fatalf("RunOnce error = %v", err)
	}
	if !processed {
		t.Fatal("RunOnce processed = false, want true")
	}
	if !finspo.listCalled {
		t.Fatal("ListSources was never called for the metadata-less job")
	}
	if len(finspo.syncedIDs) != 2 || finspo.syncedIDs[0] != "src-a" || finspo.syncedIDs[1] != "src-b" {
		t.Fatalf("syncedIDs = %v, want both registered sources", finspo.syncedIDs)
	}
	if integration.progressRequest.Status != "completed" || len(integration.progressRequest.Sources) != 2 {
		t.Fatalf("progress = %#v, want completed with 2 source refs", integration.progressRequest)
	}
}

// TestFinspoWorkerFailsMetadataLessJobWhenNothingRegistered pins the empty
// fallback: with no registered sources the job fails with an actionable
// message rather than completing as a silent no-op.
func TestFinspoWorkerFailsMetadataLessJobWhenNothingRegistered(t *testing.T) {
	integration := &fakeIntegrationClient{
		claim: store.SyncJob{
			ID:             "sync-3",
			OrganizationID: "org-1",
			UserID:         "user-1",
			ProviderKey:    "microsoft",
		},
	}
	finspo := &fakeFinspoClient{}
	processed, err := FinspoWorker{Integration: integration, Finspo: finspo}.RunOnce(context.Background())
	if err == nil {
		t.Fatal("RunOnce error = nil, want registration guidance error")
	}
	if !processed {
		t.Fatal("RunOnce processed = false, want true (job was claimed)")
	}
	if integration.progressRequest.Status != "failed" {
		t.Fatalf("progress status = %q, want failed", integration.progressRequest.Status)
	}
	if !strings.Contains(integration.progressRequest.Message, "no SharePoint or OneDrive library is registered") {
		t.Fatalf("failure message %q lacks registration guidance", integration.progressRequest.Message)
	}
	// The stable code is what integration-api's sync lanes and the SPA key
	// on ("pick a library"), independent of the human-readable message.
	if integration.progressRequest.Metadata["failureCode"] != "no_sources_registered" {
		t.Fatalf("failure metadata = %#v, want failureCode no_sources_registered", integration.progressRequest.Metadata)
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
	listSources   []handoff.FinspoSource
	listErr       error
	listCalled    bool
	syncedIDs     []string
}

func (c *fakeFinspoClient) ListSources(_ context.Context, _ string, _ string) ([]handoff.FinspoSource, error) {
	c.listCalled = true
	if c.listErr != nil {
		return nil, c.listErr
	}
	return c.listSources, nil
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
	c.syncedIDs = append(c.syncedIDs, sourceID)
	if c.syncErr != nil {
		return handoff.FinspoSyncResult{}, c.syncErr
	}
	return c.sync, nil
}

type fakeDataPlaneClient struct {
	configured       bool
	configuredCalled bool
}

func (c *fakeDataPlaneClient) Configured() bool {
	c.configuredCalled = true
	return c.configured
}
