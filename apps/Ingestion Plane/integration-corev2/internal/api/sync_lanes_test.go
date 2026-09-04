package api

import (
	"context"
	"encoding/json"
	"errors"
	"net/http/httptest"
	"strings"
	"testing"
	"time"

	"github.com/gofiber/fiber/v2"
	"github.com/rs/zerolog"

	"github.com/triodelab/integration-corev2/internal/handoff"
	"github.com/triodelab/integration-corev2/internal/store"
)

func microsoftLaneConnection(id string) store.Connection {
	return store.Connection{
		ID:                   id,
		ProviderKey:          "microsoft",
		ConnectorType:        "microsoft-graph",
		OrganizationID:       "org-1",
		WorkspaceID:          "org-1",
		UserID:               "user-1",
		Status:               "active",
		Capabilities:         []string{"profile.read", "sharepoint.read", "teams.read", "teams.messages.read", "mail.read", "mail.send"},
		Scopes:               []string{"Files.Read.All", "Sites.Read.All", "Mail.Read", "ChannelMessage.Read.All"},
		AccessTokenExpiresAt: time.Now().Add(time.Hour),
		CreatedAt:            time.Now().Add(-time.Hour),
		UpdatedAt:            time.Now().Add(-time.Hour),
	}
}

// Regression: the Outlook lane in Support said "Trenger oppmerksomhet" because
// the single connection-level last_sync_status carried the outcome of an
// unrelated SharePoint (finspo-core) job. Lanes must keep the two apart.
func TestConnectionSyncLanesKeepDocumentsFailureOutOfMailLane(t *testing.T) {
	repo := store.NewMemoryRepository()
	ctx := context.Background()
	connection := microsoftLaneConnection("conn-ms")
	connection.LastSyncStatus = "failed"
	if _, err := repo.UpsertConnection(ctx, connection); err != nil {
		t.Fatalf("UpsertConnection error: %v", err)
	}
	syncedAt := time.Now().Add(-10 * time.Minute).UTC()
	if err := repo.UpsertEmailSyncState(ctx, store.EmailSyncState{
		ConnectionID: "conn-ms",
		ProviderKey:  "microsoft",
		LastSyncedAt: syncedAt,
		UpdatedAt:    syncedAt,
	}); err != nil {
		t.Fatalf("UpsertEmailSyncState error: %v", err)
	}
	failedAt := time.Now().Add(-5 * time.Minute).UTC()
	if _, err := repo.CreateSyncJob(ctx, store.SyncJob{
		ID:             "sync-finspo",
		OrganizationID: "org-1",
		ConnectionID:   "conn-ms",
		UserID:         "user-1",
		ProviderKey:    "microsoft",
		Status:         "failed",
		Reason:         "manual",
		Mode:           "incremental",
		Metadata: map[string]any{
			"handoffTarget":  "finspo-core",
			"failure":        "finspo_worker",
			"failureCode":    "no_sources_registered",
			"failureMessage": "no SharePoint or OneDrive library is registered for this organization yet",
		},
		CreatedAt:   failedAt,
		UpdatedAt:   failedAt,
		CompletedAt: &failedAt,
	}); err != nil {
		t.Fatalf("CreateSyncJob error: %v", err)
	}

	views := connectionViews(ctx, repo, "org-1", []store.Connection{connection})
	if len(views) != 1 {
		t.Fatalf("views = %d, want 1", len(views))
	}
	lanes := views[0].SyncLanes

	mail, ok := lanes[syncLaneMail]
	if !ok {
		t.Fatalf("lanes = %#v, want a mail lane for a mail.read grant", lanes)
	}
	if mail.Status != syncLaneStatusSynced || mail.Source != syncLaneSourceEmailWorker || mail.LastError != "" {
		t.Fatalf("mail lane = %#v, want synced by email-worker with no error", mail)
	}
	if mail.LastSyncAt == nil || !mail.LastSyncAt.Equal(syncedAt) {
		t.Fatalf("mail lane lastSyncAt = %v, want %v", mail.LastSyncAt, syncedAt)
	}

	documents, ok := lanes[syncLaneDocuments]
	if !ok {
		t.Fatalf("lanes = %#v, want a documents lane", lanes)
	}
	if documents.Status != syncLaneStatusFailed || documents.Source != syncLaneSourceFinspo || documents.JobID != "sync-finspo" {
		t.Fatalf("documents lane = %#v, want failed finspo-core job sync-finspo", documents)
	}
	if documents.FailureCode != "no_sources_registered" || !strings.Contains(documents.LastError, "no SharePoint or OneDrive library") {
		t.Fatalf("documents lane = %#v, want the no_sources_registered code and message", documents)
	}

	collaboration, ok := lanes[syncLaneCollaboration]
	if !ok || collaboration.Status != syncLaneStatusPending {
		t.Fatalf("collaboration lane = %#v, want pending (Teams never attempted)", collaboration)
	}

	// The legacy single-cell status is still serialized for older readers,
	// but the lanes are the authority.
	raw, err := json.Marshal(views[0])
	if err != nil {
		t.Fatalf("Marshal error: %v", err)
	}
	var decoded map[string]any
	if err := json.Unmarshal(raw, &decoded); err != nil {
		t.Fatalf("Unmarshal error: %v", err)
	}
	if decoded["lastSyncStatus"] != "failed" {
		t.Fatalf("lastSyncStatus = %v, want legacy value preserved", decoded["lastSyncStatus"])
	}
	if _, ok := decoded["syncLanes"].(map[string]any)["mail"]; !ok {
		t.Fatalf("serialized view lacks syncLanes.mail: %s", raw)
	}
	if _, leaked := decoded["encryptedAccessToken"]; leaked {
		t.Fatalf("serialized view leaked token fields: %s", raw)
	}
}

func TestConnectionSyncLanesMailFailureComesFromEmailWorkerState(t *testing.T) {
	repo := store.NewMemoryRepository()
	ctx := context.Background()
	connection := microsoftLaneConnection("conn-mail")
	if _, err := repo.UpsertConnection(ctx, connection); err != nil {
		t.Fatalf("UpsertConnection error: %v", err)
	}
	now := time.Now().UTC()
	if err := repo.UpsertEmailSyncState(ctx, store.EmailSyncState{
		ConnectionID: "conn-mail",
		ProviderKey:  "microsoft",
		LastError:    "ingest message 42: ingest bridge returned unexpected status 401",
		FailureCount: 3,
		UpdatedAt:    now,
	}); err != nil {
		t.Fatalf("UpsertEmailSyncState error: %v", err)
	}
	// A completed finspo job must not paper over the mail failure either.
	completedAt := now.Add(-time.Minute)
	if _, err := repo.CreateSyncJob(ctx, store.SyncJob{
		ID: "sync-docs-ok", OrganizationID: "org-1", ConnectionID: "conn-mail", UserID: "user-1", ProviderKey: "microsoft",
		Status: "completed", Metadata: map[string]any{"handoffTarget": "finspo-core"},
		CreatedAt: completedAt, UpdatedAt: completedAt, CompletedAt: &completedAt,
	}); err != nil {
		t.Fatalf("CreateSyncJob error: %v", err)
	}

	lanes := connectionView1(ctx, repo, connection).SyncLanes
	if lanes[syncLaneMail].Status != syncLaneStatusFailed || !strings.Contains(lanes[syncLaneMail].LastError, "ingest bridge") {
		t.Fatalf("mail lane = %#v, want failed with the email worker's error", lanes[syncLaneMail])
	}
	if lanes[syncLaneDocuments].Status != syncLaneStatusSynced || lanes[syncLaneDocuments].LastSyncAt == nil {
		t.Fatalf("documents lane = %#v, want synced with a completion time", lanes[syncLaneDocuments])
	}
}

func TestConnectionSyncLanesManualInboxRefreshInFlightWins(t *testing.T) {
	repo := store.NewMemoryRepository()
	ctx := context.Background()
	connection := microsoftLaneConnection("conn-refresh")
	if _, err := repo.UpsertConnection(ctx, connection); err != nil {
		t.Fatalf("UpsertConnection error: %v", err)
	}
	now := time.Now().UTC()
	if _, err := repo.CreateSyncJob(ctx, store.SyncJob{
		ID: "sync-inbox", OrganizationID: "org-1", ConnectionID: "conn-refresh", UserID: "user-1", ProviderKey: "microsoft",
		Status: "waiting_provider", Mode: "inbox", Reason: "manual_inbox_refresh",
		Metadata:  map[string]any{"handoffTarget": "email-worker", "inboxChannel": "email"},
		CreatedAt: now, UpdatedAt: now,
	}); err != nil {
		t.Fatalf("CreateSyncJob error: %v", err)
	}
	lanes := connectionView1(ctx, repo, connection).SyncLanes
	if lanes[syncLaneMail].Status != syncLaneStatusRunning || lanes[syncLaneMail].JobID != "sync-inbox" {
		t.Fatalf("mail lane = %#v, want running manual refresh", lanes[syncLaneMail])
	}
	// An inbox-mode job is never a documents signal: the documents lane stays
	// "pending" (sharepoint.read granted, nothing attempted).
	if lanes[syncLaneDocuments].Status != syncLaneStatusPending || lanes[syncLaneDocuments].JobID != "" {
		t.Fatalf("documents lane = %#v, want pending without the inbox job", lanes[syncLaneDocuments])
	}
}

func TestConnectionSyncLanesOmittedWithoutGrants(t *testing.T) {
	connection := store.Connection{ID: "conn-min", ProviderKey: "microsoft", OrganizationID: "org-1", Capabilities: []string{"profile.read"}}
	if lanes := connectionSyncLanes(context.Background(), store.NewMemoryRepository(), connection, nil); lanes != nil {
		t.Fatalf("lanes = %#v, want nil for a profile-only connection", lanes)
	}
}

type fakeFinspoSourceLister struct {
	sources []handoff.FinspoSource
	err     error
	calls   int
}

func (f *fakeFinspoSourceLister) ListSources(context.Context, string, string) ([]handoff.FinspoSource, error) {
	f.calls++
	return f.sources, f.err
}

func seedMicrosoftConnectionForSync(t *testing.T, repo *store.MemoryRepository, id string) store.Connection {
	t.Helper()
	connection := microsoftLaneConnection(id)
	saved, err := repo.UpsertConnection(context.Background(), connection)
	if err != nil {
		t.Fatalf("UpsertConnection error: %v", err)
	}
	return saved
}

// The onboarding connect step used to queue a Microsoft sync right after
// OAuth; finspo-core had no library to sync, so every fresh connection ended
// with a failed job in Settings → Integrations. The route now refuses with a
// stable code the SPA turns into "velg bibliotek".
func TestConnectionSyncRefusesMicrosoftWithoutRegisteredSources(t *testing.T) {
	cfg, repo, service := testOAuthStack(t)
	finspo := &fakeFinspoSourceLister{}
	app := NewServer(ServerConfig{Config: cfg, Repo: repo, OAuth: service, Finspo: finspo})
	seedMicrosoftConnectionForSync(t, repo, "conn-ms-empty")

	req := httptest.NewRequest("POST", "/api/v1/connections/conn-ms-empty/sync", nil)
	req.Header.Set("X-Internal-API-Key", "dev-key")
	resp, err := app.Test(req)
	if err != nil {
		t.Fatalf("app.Test error: %v", err)
	}
	if resp.StatusCode != fiber.StatusConflict {
		t.Fatalf("status = %d, want 409", resp.StatusCode)
	}
	var body struct {
		Error struct {
			Code    string `json:"code"`
			Message string `json:"message"`
		} `json:"error"`
	}
	if err := json.NewDecoder(resp.Body).Decode(&body); err != nil {
		t.Fatalf("Decode error: %v", err)
	}
	if body.Error.Code != "no_sources_registered" || !strings.Contains(body.Error.Message, "library") {
		t.Fatalf("error = %#v, want no_sources_registered with library guidance", body.Error)
	}
	if finspo.calls != 1 {
		t.Fatalf("finspo ListSources calls = %d, want 1", finspo.calls)
	}
	jobs, err := repo.ListSyncJobs(context.Background(), store.SyncJobFilter{ConnectionID: "conn-ms-empty"})
	if err != nil {
		t.Fatalf("ListSyncJobs error: %v", err)
	}
	if len(jobs) != 0 {
		t.Fatalf("jobs = %d, want none queued for a refused sync", len(jobs))
	}
}

func TestConnectionSyncQueuesMicrosoftWhenSourcesExistOrFinspoUnknown(t *testing.T) {
	cases := []struct {
		name   string
		finspo FinspoSourceLister
	}{
		{name: "registered source", finspo: &fakeFinspoSourceLister{sources: []handoff.FinspoSource{{ID: "src-1", DriveID: "drive-1"}}}},
		{name: "finspo unreachable", finspo: &fakeFinspoSourceLister{err: errors.New("dial tcp: connection refused")}},
		{name: "finspo not configured", finspo: nil},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			cfg, repo, service := testOAuthStack(t)
			app := NewServer(ServerConfig{Config: cfg, Repo: repo, OAuth: service, Finspo: tc.finspo})
			seedMicrosoftConnectionForSync(t, repo, "conn-ms")

			req := httptest.NewRequest("POST", "/api/v1/connections/conn-ms/sync", nil)
			req.Header.Set("X-Internal-API-Key", "dev-key")
			resp, err := app.Test(req)
			if err != nil {
				t.Fatalf("app.Test error: %v", err)
			}
			if resp.StatusCode != fiber.StatusAccepted {
				t.Fatalf("status = %d, want 202", resp.StatusCode)
			}
			jobs, err := repo.ListSyncJobs(context.Background(), store.SyncJobFilter{ConnectionID: "conn-ms"})
			if err != nil {
				t.Fatalf("ListSyncJobs error: %v", err)
			}
			if len(jobs) != 1 || jobs[0].Metadata["handoffTarget"] != "finspo-core" {
				t.Fatalf("jobs = %#v, want one finspo-core hand-off", jobs)
			}
		})
	}
}

func TestConnectionsListCarriesSyncLanes(t *testing.T) {
	cfg, repo, service := testOAuthStack(t)
	app := NewServer(ServerConfig{Config: cfg, Repo: repo, OAuth: service})
	seedMicrosoftConnectionForSync(t, repo, "conn-lanes")
	now := time.Now().UTC()
	if _, err := repo.CreateSyncJob(context.Background(), store.SyncJob{
		ID: "sync-lanes", OrganizationID: "org-1", ConnectionID: "conn-lanes", UserID: "user-1", ProviderKey: "microsoft",
		Status: "waiting_provider", Metadata: map[string]any{"handoffTarget": "finspo-core"}, CreatedAt: now, UpdatedAt: now,
	}); err != nil {
		t.Fatalf("CreateSyncJob error: %v", err)
	}

	req := httptest.NewRequest("GET", "/api/v1/connections?organizationId=org-1", nil)
	req.Header.Set("X-Internal-API-Key", "dev-key")
	resp, err := app.Test(req)
	if err != nil {
		t.Fatalf("app.Test error: %v", err)
	}
	if resp.StatusCode != 200 {
		t.Fatalf("status = %d, want 200", resp.StatusCode)
	}
	var decoded struct {
		Data struct {
			Connections []struct {
				ID        string              `json:"id"`
				SyncLanes map[string]syncLane `json:"syncLanes"`
			} `json:"connections"`
		} `json:"data"`
	}
	if err := json.NewDecoder(resp.Body).Decode(&decoded); err != nil {
		t.Fatalf("Decode error: %v", err)
	}
	if len(decoded.Data.Connections) != 1 {
		t.Fatalf("connections = %d, want 1", len(decoded.Data.Connections))
	}
	lanes := decoded.Data.Connections[0].SyncLanes
	if lanes["documents"].Status != "running" || lanes["documents"].JobID != "sync-lanes" {
		t.Fatalf("documents lane = %#v, want running job sync-lanes", lanes["documents"])
	}
	if lanes["mail"].Status != "pending" {
		t.Fatalf("mail lane = %#v, want pending (never attempted)", lanes["mail"])
	}
}

func TestWorkerFailureMessageIsKeptOnTheJob(t *testing.T) {
	cfg, repo, service := testOAuthStack(t)
	app := NewServer(ServerConfig{Config: cfg, Repo: repo, OAuth: service})
	ctx := context.Background()
	now := time.Now().UTC()
	if _, err := repo.CreateSyncJob(ctx, store.SyncJob{
		ID: "sync-claimed", OrganizationID: "org-1", ConnectionID: "conn-x", UserID: "user-1", ProviderKey: "microsoft",
		Status:    "running",
		Metadata:  map[string]any{"handoffTarget": "finspo-core", "claimedBy": "finspo-core"},
		CreatedAt: now, UpdatedAt: now,
	}); err != nil {
		t.Fatalf("CreateSyncJob error: %v", err)
	}
	body := `{"consumer":"finspo-core","status":"failed","message":"no SharePoint or OneDrive library is registered","metadata":{"failure":"finspo_worker","failureCode":"no_sources_registered"}}`
	req := httptest.NewRequest("PATCH", "/internal/sync-jobs/sync-claimed/progress", strings.NewReader(body))
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("X-Internal-API-Key", "dev-key")
	resp, err := app.Test(req)
	if err != nil {
		t.Fatalf("app.Test error: %v", err)
	}
	if resp.StatusCode != 200 {
		t.Fatalf("status = %d, want 200", resp.StatusCode)
	}
	job, err := repo.GetSyncJob(ctx, "sync-claimed")
	if err != nil {
		t.Fatalf("GetSyncJob error: %v", err)
	}
	if job.Status != "failed" || job.Metadata["failureMessage"] != "no SharePoint or OneDrive library is registered" || job.Metadata["failureCode"] != "no_sources_registered" {
		t.Fatalf("job = %#v, want failed with failureMessage and failureCode", job)
	}
}

// Workers poll the claim route about every two seconds; an idle 404 is the
// bulk of integration-api's log volume at INFO. Only that exact idle signal
// is demoted — real claims and every other 404 stay at INFO.
func TestRequestLogLevelDemotesIdleClaimPolls(t *testing.T) {
	if got := requestLogLevel(fiber.MethodPost, "/internal/sync-jobs/claim", fiber.StatusNotFound); got != zerolog.DebugLevel {
		t.Fatalf("idle claim level = %v, want debug", got)
	}
	if got := requestLogLevel(fiber.MethodPost, "/internal/sync-jobs/claim", fiber.StatusOK); got != zerolog.InfoLevel {
		t.Fatalf("successful claim level = %v, want info", got)
	}
	if got := requestLogLevel(fiber.MethodGet, "/api/v1/connections/missing", fiber.StatusNotFound); got != zerolog.InfoLevel {
		t.Fatalf("unrelated 404 level = %v, want info", got)
	}
}
