package api

import (
	"context"
	"crypto/hmac"
	"crypto/sha256"
	"encoding/base64"
	"encoding/hex"
	"encoding/json"
	"io"
	"net/http"
	"net/http/httptest"
	"strconv"
	"strings"
	"testing"
	"time"

	"github.com/gofiber/fiber/v2"
	"github.com/triodelab/integration-corev2/internal/actions"
	"github.com/triodelab/integration-corev2/internal/auth"
	"github.com/triodelab/integration-corev2/internal/config"
	secretcrypto "github.com/triodelab/integration-corev2/internal/crypto"
	"github.com/triodelab/integration-corev2/internal/oauth"
	"github.com/triodelab/integration-corev2/internal/store"
)

func TestProvidersCatalogIsPublic(t *testing.T) {
	app := testServer(t)
	req := httptest.NewRequest("GET", "/api/v1/providers", nil)
	resp, err := app.Test(req)
	if err != nil {
		t.Fatalf("app.Test error: %v", err)
	}
	if resp.StatusCode != 200 {
		t.Fatalf("status = %d, want 200", resp.StatusCode)
	}
}

func TestHealthIsPublic(t *testing.T) {
	app := testServer(t)
	req := httptest.NewRequest("GET", "/health", nil)
	resp, err := app.Test(req)
	if err != nil {
		t.Fatalf("app.Test error: %v", err)
	}
	if resp.StatusCode != 200 {
		t.Fatalf("status = %d, want 200", resp.StatusCode)
	}
}

func TestRequestIDPropagatesToResponseEnvelope(t *testing.T) {
	app := testServer(t)
	req := httptest.NewRequest("GET", "/api/v1/providers", nil)
	req.Header.Set("X-Request-ID", "req-test-123")
	resp, err := app.Test(req)
	if err != nil {
		t.Fatalf("app.Test error: %v", err)
	}
	if got := resp.Header.Get("X-Request-ID"); got != "req-test-123" {
		t.Fatalf("X-Request-ID = %q, want req-test-123", got)
	}
	var decoded struct {
		Meta struct {
			RequestID string `json:"requestId"`
		} `json:"meta"`
	}
	if err := json.NewDecoder(resp.Body).Decode(&decoded); err != nil {
		t.Fatalf("Decode error: %v", err)
	}
	if decoded.Meta.RequestID != "req-test-123" {
		t.Fatalf("meta.requestId = %q, want req-test-123", decoded.Meta.RequestID)
	}
}

func TestMetricsRequireInternalAuthAndExposeRequestCounts(t *testing.T) {
	app := testServer(t)
	req := httptest.NewRequest("GET", "/api/v1/providers", nil)
	if _, err := app.Test(req); err != nil {
		t.Fatalf("app.Test providers error: %v", err)
	}

	req = httptest.NewRequest("GET", "/metrics", nil)
	resp, err := app.Test(req)
	if err != nil {
		t.Fatalf("app.Test unauth metrics error: %v", err)
	}
	if resp.StatusCode != 401 {
		t.Fatalf("unauth metrics status = %d, want 401", resp.StatusCode)
	}

	req = httptest.NewRequest("GET", "/metrics", nil)
	req.Header.Set("X-Internal-API-Key", "dev-key")
	resp, err = app.Test(req)
	if err != nil {
		t.Fatalf("app.Test metrics error: %v", err)
	}
	if resp.StatusCode != 200 {
		t.Fatalf("metrics status = %d, want 200", resp.StatusCode)
	}
	body, err := io.ReadAll(resp.Body)
	if err != nil {
		t.Fatalf("ReadAll metrics error: %v", err)
	}
	text := string(body)
	if !strings.Contains(text, "integration_http_requests_total") || !strings.Contains(text, `path="/api/v1/providers"`) {
		t.Fatalf("metrics body missing expected request counter: %s", text)
	}
}

func TestCreateConnectSessionReturnsDirectOAuthURL(t *testing.T) {
	app := testServer(t)
	body := `{"organizationId":"org-1","workspaceId":"org-1","userId":"user-1","userEmail":"a@example.com","bundles":["inbox"]}`
	req := httptest.NewRequest("POST", "/api/v1/providers/microsoft/connect-session", strings.NewReader(body))
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("X-Internal-API-Key", "dev-key")
	resp, err := app.Test(req)
	if err != nil {
		t.Fatalf("app.Test error: %v", err)
	}
	if resp.StatusCode != 200 {
		t.Fatalf("status = %d, want 200", resp.StatusCode)
	}
}

func TestCreateConnectSessionRejectsProviderWithoutCredentials(t *testing.T) {
	app := testServer(t)
	body := `{"organizationId":"org-1","workspaceId":"org-1","userId":"user-1","userEmail":"a@example.com","bundles":["onboarding"]}`
	req := httptest.NewRequest("POST", "/api/v1/providers/slack/connect-session", strings.NewReader(body))
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("X-Internal-API-Key", "dev-key")
	resp, err := app.Test(req)
	if err != nil {
		t.Fatalf("app.Test error: %v", err)
	}
	if resp.StatusCode != 400 {
		t.Fatalf("status = %d, want 400", resp.StatusCode)
	}
}

func TestCreateConnectSessionRejectsInboundProvider(t *testing.T) {
	app := testServer(t)
	body := `{"organizationId":"org-1","workspaceId":"org-1","userId":"user-1","userEmail":"a@example.com","bundles":["full"]}`
	req := httptest.NewRequest("POST", "/api/v1/providers/scim/connect-session", strings.NewReader(body))
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("X-Internal-API-Key", "dev-key")
	resp, err := app.Test(req)
	if err != nil {
		t.Fatalf("app.Test error: %v", err)
	}
	if resp.StatusCode != 400 {
		t.Fatalf("status = %d, want 400", resp.StatusCode)
	}
}

func TestCreateConnectSessionBearerUsesPrincipalAndPlan(t *testing.T) {
	cfg, repo, service := testOAuthStack(t)
	app := NewServer(ServerConfig{
		Config: cfg,
		Repo:   repo,
		OAuth:  service,
		Auth: fakeVerifier{principal: auth.Principal{
			UserID:         "user-real",
			OrganizationID: "org-1",
			WorkspaceID:    "workspace-1",
			Role:           "admin",
			Email:          "real@example.com",
		}},
		Org: fakeOrgClient{plan: "pro"},
	})

	body := `{"organizationId":"org-1","workspaceId":"workspace-1","userId":"spoofed","userEmail":"spoofed@example.com","bundles":["inbox"]}`
	req := httptest.NewRequest("POST", "/api/v1/providers/microsoft/connect-session", strings.NewReader(body))
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("Authorization", "Bearer valid-token")
	resp, err := app.Test(req)
	if err != nil {
		t.Fatalf("app.Test error: %v", err)
	}
	if resp.StatusCode != 200 {
		t.Fatalf("status = %d, want 200", resp.StatusCode)
	}

	var decoded struct {
		Data struct {
			SessionToken string `json:"sessionToken"`
		} `json:"data"`
	}
	if err := json.NewDecoder(resp.Body).Decode(&decoded); err != nil {
		t.Fatalf("Decode error: %v", err)
	}
	session, err := repo.GetConnectSessionByID(t.Context(), decoded.Data.SessionToken)
	if err != nil {
		t.Fatalf("GetConnectSessionByID error: %v", err)
	}
	if session.UserID != "user-real" || session.UserEmail != "real@example.com" {
		t.Fatalf("session identity = %s/%s, want control-plane principal", session.UserID, session.UserEmail)
	}
}

func TestCreateConnectSessionRejectsBearerForOtherOrganization(t *testing.T) {
	cfg, repo, service := testOAuthStack(t)
	app := NewServer(ServerConfig{
		Config: cfg,
		Repo:   repo,
		OAuth:  service,
		Auth: fakeVerifier{principal: auth.Principal{
			UserID:         "user-1",
			OrganizationID: "org-1",
			WorkspaceID:    "workspace-1",
			Role:           "admin",
			Email:          "real@example.com",
		}},
		Org: fakeOrgClient{plan: "pro"},
	})

	body := `{"organizationId":"org-2","workspaceId":"workspace-2","userId":"user-1","userEmail":"real@example.com","bundles":["inbox"]}`
	req := httptest.NewRequest("POST", "/api/v1/providers/microsoft/connect-session", strings.NewReader(body))
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("Authorization", "Bearer valid-token")
	resp, err := app.Test(req)
	if err != nil {
		t.Fatalf("app.Test error: %v", err)
	}
	if resp.StatusCode != 403 {
		t.Fatalf("status = %d, want 403", resp.StatusCode)
	}
}

func TestCreateConnectSessionRejectsFreePlanForBearer(t *testing.T) {
	cfg, repo, service := testOAuthStack(t)
	app := NewServer(ServerConfig{
		Config: cfg,
		Repo:   repo,
		OAuth:  service,
		Auth: fakeVerifier{principal: auth.Principal{
			UserID:         "user-1",
			OrganizationID: "org-1",
			WorkspaceID:    "workspace-1",
			Role:           "member",
			Email:          "real@example.com",
		}},
		Org: fakeOrgClient{plan: "free"},
	})

	body := `{"organizationId":"org-1","workspaceId":"workspace-1","userId":"user-1","userEmail":"real@example.com","bundles":["inbox"]}`
	req := httptest.NewRequest("POST", "/api/v1/providers/microsoft/connect-session", strings.NewReader(body))
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("Authorization", "Bearer valid-token")
	resp, err := app.Test(req)
	if err != nil {
		t.Fatalf("app.Test error: %v", err)
	}
	if resp.StatusCode != 403 {
		t.Fatalf("status = %d, want 403", resp.StatusCode)
	}
}

func TestConnectionsListScopesBearerToPrincipalOrganization(t *testing.T) {
	cfg, repo, service := testOAuthStack(t)
	_, _ = repo.UpsertConnection(t.Context(), store.Connection{
		ID:                   "conn-1",
		ProviderKey:          "microsoft",
		ConnectorType:        "microsoft-graph",
		OrganizationID:       "org-1",
		UserID:               "user-1",
		Status:               "active",
		AccessTokenExpiresAt: time.Now().Add(time.Hour),
	})
	_, _ = repo.UpsertConnection(t.Context(), store.Connection{
		ID:                   "conn-2",
		ProviderKey:          "microsoft",
		ConnectorType:        "microsoft-graph",
		OrganizationID:       "org-2",
		UserID:               "user-2",
		Status:               "active",
		AccessTokenExpiresAt: time.Now().Add(time.Hour),
	})
	app := NewServer(ServerConfig{
		Config: cfg,
		Repo:   repo,
		OAuth:  service,
		Auth: fakeVerifier{principal: auth.Principal{
			UserID:         "user-1",
			OrganizationID: "org-1",
			WorkspaceID:    "workspace-1",
			Role:           "member",
			Email:          "real@example.com",
		}},
	})

	req := httptest.NewRequest("GET", "/api/v1/connections?organizationId=org-2", nil)
	req.Header.Set("Authorization", "Bearer valid-token")
	resp, err := app.Test(req)
	if err != nil {
		t.Fatalf("app.Test error: %v", err)
	}
	if resp.StatusCode != 200 {
		t.Fatalf("status = %d, want 200", resp.StatusCode)
	}
	var decoded struct {
		Data struct {
			Connections []store.Connection `json:"connections"`
		} `json:"data"`
	}
	if err := json.NewDecoder(resp.Body).Decode(&decoded); err != nil {
		t.Fatalf("Decode error: %v", err)
	}
	if len(decoded.Data.Connections) != 1 || decoded.Data.Connections[0].OrganizationID != "org-1" {
		t.Fatalf("connections = %#v, want only org-1", decoded.Data.Connections)
	}
}

func TestConnectionStatusRejectsCrossOrgBearer(t *testing.T) {
	cfg, repo, service := testOAuthStack(t)
	_, _ = repo.UpsertConnection(t.Context(), store.Connection{
		ID:                   "conn-other-org",
		ProviderKey:          "microsoft",
		ConnectorType:        "microsoft-graph",
		OrganizationID:       "org-2",
		UserID:               "user-2",
		Status:               "active",
		AccessTokenExpiresAt: time.Now().Add(time.Hour),
	})
	app := NewServer(ServerConfig{
		Config: cfg,
		Repo:   repo,
		OAuth:  service,
		Auth: fakeVerifier{principal: auth.Principal{
			UserID:         "user-1",
			OrganizationID: "org-1",
			WorkspaceID:    "workspace-1",
			Role:           "member",
			Email:          "real@example.com",
		}},
	})

	req := httptest.NewRequest("GET", "/api/v1/connections/conn-other-org/status", nil)
	req.Header.Set("Authorization", "Bearer valid-token")
	resp, err := app.Test(req)
	if err != nil {
		t.Fatalf("app.Test error: %v", err)
	}
	if resp.StatusCode != 403 {
		t.Fatalf("status = %d, want 403", resp.StatusCode)
	}
}

func TestConnectionCapabilitiesCanBeUpdated(t *testing.T) {
	cfg, repo, service := testOAuthStack(t)
	_, err := repo.UpsertConnection(t.Context(), store.Connection{
		ID:                   "conn-microsoft",
		ProviderKey:          "microsoft",
		ConnectorType:        "microsoft-graph",
		OrganizationID:       "org-1",
		WorkspaceID:          "org-1",
		UserID:               "user-1",
		Status:               "active",
		AccessTokenExpiresAt: time.Now().Add(time.Hour),
	})
	if err != nil {
		t.Fatalf("UpsertConnection error: %v", err)
	}
	app := NewServer(ServerConfig{Config: cfg, Repo: repo, OAuth: service})

	req := httptest.NewRequest("PATCH", "/api/v1/connections/conn-microsoft/capabilities", strings.NewReader(`{"capabilities":["profile.read","mail.read"]}`))
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("X-Internal-API-Key", "dev-key")
	resp, err := app.Test(req)
	if err != nil {
		t.Fatalf("app.Test error: %v", err)
	}
	if resp.StatusCode != 200 {
		t.Fatalf("status = %d, want 200", resp.StatusCode)
	}
	updated, err := repo.GetConnection(t.Context(), "conn-microsoft")
	if err != nil {
		t.Fatalf("GetConnection error: %v", err)
	}
	if got := strings.Join(updated.Capabilities, ","); got != "profile.read,mail.read" {
		t.Fatalf("capabilities = %q, want profile.read,mail.read", got)
	}
}

func TestConnectionCapabilitiesRejectUnknownCapability(t *testing.T) {
	cfg, repo, service := testOAuthStack(t)
	_, _ = repo.UpsertConnection(t.Context(), store.Connection{
		ID:                   "conn-microsoft",
		ProviderKey:          "microsoft",
		ConnectorType:        "microsoft-graph",
		OrganizationID:       "org-1",
		UserID:               "user-1",
		Status:               "active",
		AccessTokenExpiresAt: time.Now().Add(time.Hour),
	})
	app := NewServer(ServerConfig{Config: cfg, Repo: repo, OAuth: service})

	req := httptest.NewRequest("PATCH", "/api/v1/connections/conn-microsoft/capabilities", strings.NewReader(`{"capabilities":["raw.proxy"]}`))
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("X-Internal-API-Key", "dev-key")
	resp, err := app.Test(req)
	if err != nil {
		t.Fatalf("app.Test error: %v", err)
	}
	if resp.StatusCode != 400 {
		t.Fatalf("status = %d, want 400", resp.StatusCode)
	}
}

func TestSensitiveActionRequiresCapabilityBeforeProviderCall(t *testing.T) {
	called := false
	providerServer := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		called = true
		w.WriteHeader(http.StatusOK)
		_, _ = w.Write([]byte(`{"value":[]}`))
	}))
	defer providerServer.Close()

	cfg, repo, service := testOAuthStack(t)
	cfg.MicrosoftGraphBaseURL = providerServer.URL
	_, _ = repo.UpsertConnection(t.Context(), store.Connection{
		ID:                   "conn-ms",
		ProviderKey:          "microsoft",
		ConnectorType:        "microsoft-graph",
		OrganizationID:       "org-1",
		UserID:               "user-1",
		Status:               "active",
		Capabilities:         []string{"profile.read"},
		AccessTokenExpiresAt: time.Now().Add(time.Hour),
	})
	app := NewServer(ServerConfig{
		Config:  cfg,
		Repo:    repo,
		OAuth:   service,
		Actions: actions.NewService(cfg, providerServer.Client()),
	})

	req := httptest.NewRequest("POST", "/api/v1/connections/conn-ms/actions", strings.NewReader(`{"operation":"mail.messages"}`))
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("X-Internal-API-Key", "dev-key")
	resp, err := app.Test(req)
	if err != nil {
		t.Fatalf("app.Test error: %v", err)
	}
	if resp.StatusCode != 403 {
		t.Fatalf("status = %d, want 403", resp.StatusCode)
	}
	if called {
		t.Fatal("provider was called before capability denial")
	}
}

func TestWriteActionRequiresApprovalBeforeTokenLookup(t *testing.T) {
	called := false
	providerServer := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		called = true
		w.WriteHeader(http.StatusOK)
		_, _ = w.Write([]byte(`{"ok":true}`))
	}))
	defer providerServer.Close()

	cfg, repo, service := testOAuthStack(t)
	cfg.MicrosoftGraphBaseURL = providerServer.URL
	_, _ = repo.UpsertConnection(t.Context(), store.Connection{
		ID:                   "conn-ms",
		ProviderKey:          "microsoft",
		ConnectorType:        "microsoft-graph",
		OrganizationID:       "org-1",
		UserID:               "user-1",
		Status:               "active",
		Capabilities:         []string{"mail.send"},
		AccessTokenExpiresAt: time.Now().Add(time.Hour),
	})
	app := NewServer(ServerConfig{
		Config:  cfg,
		Repo:    repo,
		OAuth:   service,
		Actions: actions.NewService(cfg, providerServer.Client()),
	})

	req := httptest.NewRequest("POST", "/api/v1/connections/conn-ms/actions", strings.NewReader(`{"operation":"mail.send","body":{"message":{"subject":"hello"}}}`))
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("X-Internal-API-Key", "dev-key")
	resp, err := app.Test(req)
	if err != nil {
		t.Fatalf("app.Test error: %v", err)
	}
	if resp.StatusCode != 403 {
		t.Fatalf("status = %d, want 403", resp.StatusCode)
	}
	if called {
		t.Fatal("provider was called before approval denial")
	}
}

func TestConnectionConsentUpsertAndList(t *testing.T) {
	cfg, repo, service := testOAuthStack(t)
	_, _ = repo.UpsertConnection(t.Context(), store.Connection{
		ID:                   "conn-github",
		ProviderKey:          "github",
		ConnectorType:        "github",
		OrganizationID:       "org-1",
		UserID:               "user-1",
		Status:               "active",
		AccessTokenExpiresAt: time.Now().Add(time.Hour),
	})
	app := NewServer(ServerConfig{Config: cfg, Repo: repo, OAuth: service})

	req := httptest.NewRequest("POST", "/api/v1/connections/conn-github/consents", strings.NewReader(`{"source":"repo_metadata","purpose":"knowledge_preview","granted":true}`))
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("X-Internal-API-Key", "dev-key")
	resp, err := app.Test(req)
	if err != nil {
		t.Fatalf("app.Test error: %v", err)
	}
	if resp.StatusCode != 200 {
		t.Fatalf("status = %d, want 200", resp.StatusCode)
	}

	req = httptest.NewRequest("GET", "/api/v1/connections/conn-github/consents", nil)
	req.Header.Set("X-Internal-API-Key", "dev-key")
	resp, err = app.Test(req)
	if err != nil {
		t.Fatalf("app.Test error: %v", err)
	}
	var decoded struct {
		Data struct {
			Consents []store.ConnectionConsent `json:"consents"`
		} `json:"data"`
	}
	if err := json.NewDecoder(resp.Body).Decode(&decoded); err != nil {
		t.Fatalf("Decode error: %v", err)
	}
	if len(decoded.Data.Consents) != 1 || !decoded.Data.Consents[0].Granted {
		t.Fatalf("consents = %#v, want one granted consent", decoded.Data.Consents)
	}
}

func TestSyncJobCreateAndEvents(t *testing.T) {
	cfg, repo, service := testOAuthStack(t)
	_, _ = repo.UpsertConnection(t.Context(), store.Connection{
		ID:                   "conn-notion",
		ProviderKey:          "notion",
		ConnectorType:        "notion",
		OrganizationID:       "org-1",
		UserID:               "user-1",
		Status:               "active",
		AccessTokenExpiresAt: time.Now().Add(time.Hour),
	})
	app := NewServer(ServerConfig{Config: cfg, Repo: repo, OAuth: service})

	req := httptest.NewRequest("POST", "/api/v1/sync-jobs", strings.NewReader(`{"connectionId":"conn-notion","reason":"manual","mode":"incremental"}`))
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("X-Internal-API-Key", "dev-key")
	resp, err := app.Test(req)
	if err != nil {
		t.Fatalf("app.Test error: %v", err)
	}
	if resp.StatusCode != 202 {
		t.Fatalf("status = %d, want 202", resp.StatusCode)
	}
	var decoded struct {
		Data struct {
			SyncJob store.SyncJob `json:"syncJob"`
		} `json:"data"`
	}
	if err := json.NewDecoder(resp.Body).Decode(&decoded); err != nil {
		t.Fatalf("Decode error: %v", err)
	}
	if decoded.Data.SyncJob.Status != "handoff_data_plane" {
		t.Fatalf("sync status = %q, want handoff_data_plane", decoded.Data.SyncJob.Status)
	}

	req = httptest.NewRequest("GET", "/api/v1/sync-jobs/"+decoded.Data.SyncJob.ID+"/events", nil)
	req.Header.Set("X-Internal-API-Key", "dev-key")
	resp, err = app.Test(req)
	if err != nil {
		t.Fatalf("app.Test error: %v", err)
	}
	if got := resp.Header.Get("Content-Type"); !strings.Contains(got, "text/event-stream") {
		t.Fatalf("content-type = %q, want text/event-stream", got)
	}

	req = httptest.NewRequest("POST", "/api/v1/sync-jobs/"+decoded.Data.SyncJob.ID+"/cancel", strings.NewReader(`{"reason":"user_requested"}`))
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("X-Internal-API-Key", "dev-key")
	resp, err = app.Test(req)
	if err != nil {
		t.Fatalf("app.Test cancel error: %v", err)
	}
	if resp.StatusCode != 200 {
		t.Fatalf("cancel status = %d, want 200", resp.StatusCode)
	}
	var cancelled struct {
		Data struct {
			SyncJob store.SyncJob `json:"syncJob"`
		} `json:"data"`
	}
	if err := json.NewDecoder(resp.Body).Decode(&cancelled); err != nil {
		t.Fatalf("Decode cancel error: %v", err)
	}
	if cancelled.Data.SyncJob.Status != "cancelled" {
		t.Fatalf("cancelled status = %q, want cancelled", cancelled.Data.SyncJob.Status)
	}

	req = httptest.NewRequest("POST", "/api/v1/sync-jobs/"+decoded.Data.SyncJob.ID+"/retry", nil)
	req.Header.Set("X-Internal-API-Key", "dev-key")
	resp, err = app.Test(req)
	if err != nil {
		t.Fatalf("app.Test retry error: %v", err)
	}
	if resp.StatusCode != 202 {
		t.Fatalf("retry status = %d, want 202", resp.StatusCode)
	}
	var retried struct {
		Data struct {
			SyncJob store.SyncJob `json:"syncJob"`
		} `json:"data"`
	}
	if err := json.NewDecoder(resp.Body).Decode(&retried); err != nil {
		t.Fatalf("Decode retry error: %v", err)
	}
	if retried.Data.SyncJob.ID == decoded.Data.SyncJob.ID {
		t.Fatal("retry reused original sync job id")
	}
	if retried.Data.SyncJob.Status != "handoff_data_plane" {
		t.Fatalf("retry status = %q, want handoff_data_plane", retried.Data.SyncJob.Status)
	}
	if retried.Data.SyncJob.Metadata["retryOf"] != decoded.Data.SyncJob.ID {
		t.Fatalf("retryOf = %#v, want %s", retried.Data.SyncJob.Metadata["retryOf"], decoded.Data.SyncJob.ID)
	}
}

func TestMicrosoftSyncJobWaitsForFinspoHandoff(t *testing.T) {
	cfg, repo, service := testOAuthStack(t)
	_, _ = repo.UpsertConnection(t.Context(), store.Connection{
		ID:                   "conn-ms",
		ProviderKey:          "microsoft",
		ConnectorType:        "microsoft-graph",
		OrganizationID:       "org-1",
		UserID:               "user-1",
		Status:               "active",
		AccessTokenExpiresAt: time.Now().Add(time.Hour),
	})
	app := NewServer(ServerConfig{Config: cfg, Repo: repo, OAuth: service})

	req := httptest.NewRequest("POST", "/api/v1/connections/conn-ms/sync", nil)
	req.Header.Set("X-Internal-API-Key", "dev-key")
	resp, err := app.Test(req)
	if err != nil {
		t.Fatalf("app.Test error: %v", err)
	}
	if resp.StatusCode != 202 {
		t.Fatalf("status = %d, want 202", resp.StatusCode)
	}
	var decoded struct {
		Data struct {
			SyncJob store.SyncJob `json:"syncJob"`
		} `json:"data"`
	}
	if err := json.NewDecoder(resp.Body).Decode(&decoded); err != nil {
		t.Fatalf("Decode error: %v", err)
	}
	if decoded.Data.SyncJob.Status != "waiting_provider" {
		t.Fatalf("sync status = %q, want waiting_provider", decoded.Data.SyncJob.Status)
	}
	if decoded.Data.SyncJob.Metadata["handoffTarget"] != "finspo-core" {
		t.Fatalf("handoff target = %#v, want finspo-core", decoded.Data.SyncJob.Metadata["handoffTarget"])
	}
}

func TestMicrosoftSyncJobCarriesSharePointSourceIdentifiers(t *testing.T) {
	cfg, repo, service := testOAuthStack(t)
	_, _ = repo.UpsertConnection(t.Context(), store.Connection{
		ID:                   "conn-ms",
		ProviderKey:          "microsoft",
		ConnectorType:        "microsoft-graph",
		OrganizationID:       "org-1",
		UserID:               "user-1",
		Status:               "active",
		AccessTokenExpiresAt: time.Now().Add(time.Hour),
	})
	app := NewServer(ServerConfig{Config: cfg, Repo: repo, OAuth: service})

	body := `{
		"connectionId":"conn-ms",
		"reason":"settings_user_requested",
		"mode":"incremental",
		"checkpoint":{"siteId":"site-1"},
		"metadata":{
			"driveId":"drive-1",
			"driveName":"Shared Documents",
			"siteWebUrl":"https://contoso.sharepoint.com/sites/support"
		}
	}`
	req := httptest.NewRequest("POST", "/api/v1/sync-jobs", strings.NewReader(body))
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("X-Internal-API-Key", "dev-key")
	resp, err := app.Test(req)
	if err != nil {
		t.Fatalf("app.Test create error: %v", err)
	}
	if resp.StatusCode != 202 {
		t.Fatalf("create status = %d, want 202", resp.StatusCode)
	}
	var created struct {
		Data struct {
			SyncJob store.SyncJob `json:"syncJob"`
		} `json:"data"`
	}
	if err := json.NewDecoder(resp.Body).Decode(&created); err != nil {
		t.Fatalf("Decode create error: %v", err)
	}
	if created.Data.SyncJob.Checkpoint["site_id"] != "site-1" {
		t.Fatalf("site_id = %#v, want site-1", created.Data.SyncJob.Checkpoint["site_id"])
	}
	if created.Data.SyncJob.Checkpoint["drive_id"] != "drive-1" {
		t.Fatalf("drive_id = %#v, want drive-1", created.Data.SyncJob.Checkpoint["drive_id"])
	}
	if created.Data.SyncJob.Checkpoint["drive_name"] != "Shared Documents" {
		t.Fatalf("drive_name = %#v, want Shared Documents", created.Data.SyncJob.Checkpoint["drive_name"])
	}

	req = httptest.NewRequest("POST", "/internal/sync-jobs/claim", strings.NewReader(`{"consumer":"finspo-core","target":"finspo-core","providerKey":"microsoft","organizationId":"org-1"}`))
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("X-Internal-API-Key", "dev-key")
	resp, err = app.Test(req)
	if err != nil {
		t.Fatalf("app.Test claim error: %v", err)
	}
	if resp.StatusCode != 200 {
		t.Fatalf("claim status = %d, want 200", resp.StatusCode)
	}
	var claimed struct {
		Data struct {
			SyncJob store.SyncJob `json:"syncJob"`
		} `json:"data"`
	}
	if err := json.NewDecoder(resp.Body).Decode(&claimed); err != nil {
		t.Fatalf("Decode claim error: %v", err)
	}
	if claimed.Data.SyncJob.Checkpoint["site_id"] != "site-1" || claimed.Data.SyncJob.Checkpoint["drive_id"] != "drive-1" {
		t.Fatalf("claimed checkpoint = %#v, want site and drive identifiers", claimed.Data.SyncJob.Checkpoint)
	}
}

func TestInternalSyncClaimRejectsBrowserAccess(t *testing.T) {
	app := testServer(t)
	req := httptest.NewRequest("POST", "/internal/sync-jobs/claim", strings.NewReader(`{"consumer":"data-plane-v2"}`))
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("Authorization", "Bearer browser-token")
	resp, err := app.Test(req)
	if err != nil {
		t.Fatalf("app.Test error: %v", err)
	}
	if resp.StatusCode != 401 {
		t.Fatalf("status = %d, want 401", resp.StatusCode)
	}
}

func TestInternalWorkerClaimsAndCompletesSyncJob(t *testing.T) {
	cfg, repo, service := testOAuthStack(t)
	_, _ = repo.UpsertConnection(t.Context(), store.Connection{
		ID:                   "conn-notion",
		ProviderKey:          "notion",
		ConnectorType:        "notion",
		OrganizationID:       "org-1",
		UserID:               "user-1",
		Status:               "active",
		AccessTokenExpiresAt: time.Now().Add(time.Hour),
	})
	app := NewServer(ServerConfig{Config: cfg, Repo: repo, OAuth: service})

	req := httptest.NewRequest("POST", "/api/v1/sync-jobs", strings.NewReader(`{"connectionId":"conn-notion","reason":"manual","mode":"incremental"}`))
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("X-Internal-API-Key", "dev-key")
	resp, err := app.Test(req)
	if err != nil {
		t.Fatalf("app.Test create error: %v", err)
	}
	if resp.StatusCode != 202 {
		t.Fatalf("create status = %d, want 202", resp.StatusCode)
	}

	req = httptest.NewRequest("POST", "/internal/sync-jobs/claim", strings.NewReader(`{"consumer":"data-plane-v2","target":"data-plane-v2","organizationId":"org-1"}`))
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("X-Internal-API-Key", "dev-key")
	resp, err = app.Test(req)
	if err != nil {
		t.Fatalf("app.Test claim error: %v", err)
	}
	if resp.StatusCode != 200 {
		t.Fatalf("claim status = %d, want 200", resp.StatusCode)
	}
	var claimed struct {
		Data struct {
			SyncJob store.SyncJob `json:"syncJob"`
		} `json:"data"`
	}
	if err := json.NewDecoder(resp.Body).Decode(&claimed); err != nil {
		t.Fatalf("Decode claim error: %v", err)
	}
	if claimed.Data.SyncJob.Status != "running" {
		t.Fatalf("claimed status = %q, want running", claimed.Data.SyncJob.Status)
	}
	if claimed.Data.SyncJob.Metadata["claimedBy"] != "data-plane-v2" {
		t.Fatalf("claimedBy = %#v, want data-plane-v2", claimed.Data.SyncJob.Metadata["claimedBy"])
	}

	req = httptest.NewRequest("POST", "/internal/sync-jobs/claim", strings.NewReader(`{"consumer":"data-plane-v2","target":"data-plane-v2","organizationId":"org-1"}`))
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("X-Internal-API-Key", "dev-key")
	resp, err = app.Test(req)
	if err != nil {
		t.Fatalf("app.Test duplicate claim error: %v", err)
	}
	if resp.StatusCode != 404 {
		t.Fatalf("duplicate claim status = %d, want 404", resp.StatusCode)
	}

	progressBody := `{
		"consumer":"data-plane-v2",
		"status":"completed",
		"checkpoint":{"cursor":"cursor-1"},
		"sources":[
			{"provider":"notion","type":"workspace","sourceId":"dp-source-1","externalId":"workspace-1","title":"Allowed workspace"},
			{"provider":"notion","type":"workspace","sourceId":"dp-source-1","externalId":"workspace-1","title":"Allowed workspace"}
		]
	}`
	req = httptest.NewRequest("PATCH", "/internal/sync-jobs/"+claimed.Data.SyncJob.ID+"/progress", strings.NewReader(progressBody))
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("X-Internal-API-Key", "dev-key")
	resp, err = app.Test(req)
	if err != nil {
		t.Fatalf("app.Test progress error: %v", err)
	}
	if resp.StatusCode != 200 {
		t.Fatalf("progress status = %d, want 200", resp.StatusCode)
	}
	var progressed struct {
		Data struct {
			SyncJob store.SyncJob `json:"syncJob"`
		} `json:"data"`
	}
	if err := json.NewDecoder(resp.Body).Decode(&progressed); err != nil {
		t.Fatalf("Decode progress error: %v", err)
	}
	if progressed.Data.SyncJob.Status != "completed" {
		t.Fatalf("progress status = %q, want completed", progressed.Data.SyncJob.Status)
	}
	if progressed.Data.SyncJob.Checkpoint["cursor"] != "cursor-1" {
		t.Fatalf("cursor = %#v, want cursor-1", progressed.Data.SyncJob.Checkpoint["cursor"])
	}
	if got := syncSourceRefCount(progressed.Data.SyncJob.Checkpoint); got != 1 {
		t.Fatalf("source refs = %d, want 1", got)
	}
}

func TestFinspoWorkerClaimsMicrosoftSyncJob(t *testing.T) {
	cfg, repo, service := testOAuthStack(t)
	_, _ = repo.UpsertConnection(t.Context(), store.Connection{
		ID:                   "conn-ms",
		ProviderKey:          "microsoft",
		ConnectorType:        "microsoft-graph",
		OrganizationID:       "org-1",
		UserID:               "user-1",
		Status:               "active",
		AccessTokenExpiresAt: time.Now().Add(time.Hour),
	})
	app := NewServer(ServerConfig{Config: cfg, Repo: repo, OAuth: service})

	req := httptest.NewRequest("POST", "/api/v1/connections/conn-ms/sync", nil)
	req.Header.Set("X-Internal-API-Key", "dev-key")
	resp, err := app.Test(req)
	if err != nil {
		t.Fatalf("app.Test create error: %v", err)
	}
	if resp.StatusCode != 202 {
		t.Fatalf("create status = %d, want 202", resp.StatusCode)
	}

	req = httptest.NewRequest("POST", "/internal/sync-jobs/claim", strings.NewReader(`{"consumer":"finspo-core","target":"finspo-core","providerKey":"microsoft","organizationId":"org-1"}`))
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("X-Internal-API-Key", "dev-key")
	resp, err = app.Test(req)
	if err != nil {
		t.Fatalf("app.Test claim error: %v", err)
	}
	if resp.StatusCode != 200 {
		t.Fatalf("claim status = %d, want 200", resp.StatusCode)
	}
	var claimed struct {
		Data struct {
			SyncJob store.SyncJob `json:"syncJob"`
		} `json:"data"`
	}
	if err := json.NewDecoder(resp.Body).Decode(&claimed); err != nil {
		t.Fatalf("Decode claim error: %v", err)
	}
	if claimed.Data.SyncJob.Status != "running" {
		t.Fatalf("claimed status = %q, want running", claimed.Data.SyncJob.Status)
	}
	if claimed.Data.SyncJob.Metadata["handoffTarget"] != "finspo-core" {
		t.Fatalf("handoff target = %#v, want finspo-core", claimed.Data.SyncJob.Metadata["handoffTarget"])
	}
}

func TestStripeWebhookSignatureIsVerified(t *testing.T) {
	cfg, repo, service := testOAuthStack(t)
	cfg.StripeWebhookSecret = "whsec_test"
	app := NewServer(ServerConfig{Config: cfg, Repo: repo, OAuth: service})
	body := `{"type":"account.updated","organizationId":"org-1"}`
	timestamp := "1760000000"
	mac := hmac.New(sha256.New, []byte(cfg.StripeWebhookSecret))
	_, _ = mac.Write([]byte(timestamp + "." + body))
	signature := hex.EncodeToString(mac.Sum(nil))

	req := httptest.NewRequest("POST", "/api/v1/webhooks/stripe", strings.NewReader(body))
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("Stripe-Signature", "t="+timestamp+",v1="+signature)
	resp, err := app.Test(req)
	if err != nil {
		t.Fatalf("app.Test error: %v", err)
	}
	if resp.StatusCode != 200 {
		t.Fatalf("status = %d, want 200", resp.StatusCode)
	}
}

func TestStripeWebhookReplayIsIdempotent(t *testing.T) {
	cfg, repo, service := testOAuthStack(t)
	cfg.StripeWebhookSecret = "whsec_test"
	app := NewServer(ServerConfig{Config: cfg, Repo: repo, OAuth: service})
	body := `{"id":"evt_1","type":"account.updated","organizationId":"org-1"}`
	timestamp := "1760000000"
	mac := hmac.New(sha256.New, []byte(cfg.StripeWebhookSecret))
	_, _ = mac.Write([]byte(timestamp + "." + body))
	signature := hex.EncodeToString(mac.Sum(nil))

	for i := 0; i < 2; i++ {
		req := httptest.NewRequest("POST", "/api/v1/webhooks/stripe", strings.NewReader(body))
		req.Header.Set("Content-Type", "application/json")
		req.Header.Set("Stripe-Signature", "t="+timestamp+",v1="+signature)
		resp, err := app.Test(req)
		if err != nil {
			t.Fatalf("app.Test error: %v", err)
		}
		if resp.StatusCode != 200 {
			t.Fatalf("status[%d] = %d, want 200", i, resp.StatusCode)
		}
		if i == 1 {
			var decoded struct {
				Data struct {
					Duplicate bool `json:"duplicate"`
				} `json:"data"`
			}
			if err := json.NewDecoder(resp.Body).Decode(&decoded); err != nil {
				t.Fatalf("Decode replay response error: %v", err)
			}
			if !decoded.Data.Duplicate {
				t.Fatal("second webhook did not report duplicate replay")
			}
		}
	}
}

func TestStripeWebhookRejectsInvalidSignature(t *testing.T) {
	cfg, repo, service := testOAuthStack(t)
	cfg.StripeWebhookSecret = "whsec_test"
	app := NewServer(ServerConfig{Config: cfg, Repo: repo, OAuth: service})

	req := httptest.NewRequest("POST", "/api/v1/webhooks/stripe", strings.NewReader(`{"type":"account.updated"}`))
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("Stripe-Signature", "t=1760000000,v1=bad")
	resp, err := app.Test(req)
	if err != nil {
		t.Fatalf("app.Test error: %v", err)
	}
	if resp.StatusCode != 401 {
		t.Fatalf("status = %d, want 401", resp.StatusCode)
	}
}

func TestSlackWebhookSignatureIsVerified(t *testing.T) {
	cfg, repo, service := testOAuthStack(t)
	cfg.SlackSigningSecret = "slack-signing-secret"
	app := NewServer(ServerConfig{Config: cfg, Repo: repo, OAuth: service})
	body := `{"event":"team_join","organizationId":"org-1"}`
	timestamp := time.Now().Unix()
	base := "v0:" + strconv.FormatInt(timestamp, 10) + ":" + body
	mac := hmac.New(sha256.New, []byte(cfg.SlackSigningSecret))
	_, _ = mac.Write([]byte(base))
	signature := "v0=" + hex.EncodeToString(mac.Sum(nil))

	req := httptest.NewRequest("POST", "/api/v1/webhooks/slack", strings.NewReader(body))
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("X-Slack-Request-Timestamp", strconv.FormatInt(timestamp, 10))
	req.Header.Set("X-Slack-Signature", signature)
	resp, err := app.Test(req)
	if err != nil {
		t.Fatalf("app.Test error: %v", err)
	}
	if resp.StatusCode != 200 {
		t.Fatalf("status = %d, want 200", resp.StatusCode)
	}
}

func TestGitHubWebhookSignatureIsVerified(t *testing.T) {
	cfg, repo, service := testOAuthStack(t)
	cfg.GitHubWebhookSecret = "github-webhook-secret"
	app := NewServer(ServerConfig{Config: cfg, Repo: repo, OAuth: service})
	body := `{"organizationId":"org-1","repository":{"name":"demo"}}`
	mac := hmac.New(sha256.New, []byte(cfg.GitHubWebhookSecret))
	_, _ = mac.Write([]byte(body))
	signature := "sha256=" + hex.EncodeToString(mac.Sum(nil))

	req := httptest.NewRequest("POST", "/api/v1/webhooks/github", strings.NewReader(body))
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("X-Hub-Signature-256", signature)
	req.Header.Set("X-GitHub-Delivery", "delivery-1")
	req.Header.Set("X-GitHub-Event", "push")
	resp, err := app.Test(req)
	if err != nil {
		t.Fatalf("app.Test error: %v", err)
	}
	if resp.StatusCode != 200 {
		t.Fatalf("status = %d, want 200", resp.StatusCode)
	}
}

func TestGitHubWebhookRejectsInvalidSignature(t *testing.T) {
	cfg, repo, service := testOAuthStack(t)
	cfg.GitHubWebhookSecret = "github-webhook-secret"
	app := NewServer(ServerConfig{Config: cfg, Repo: repo, OAuth: service})

	req := httptest.NewRequest("POST", "/api/v1/webhooks/github", strings.NewReader(`{"repository":{"name":"demo"}}`))
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("X-Hub-Signature-256", "sha256=bad")
	resp, err := app.Test(req)
	if err != nil {
		t.Fatalf("app.Test error: %v", err)
	}
	if resp.StatusCode != 401 {
		t.Fatalf("status = %d, want 401", resp.StatusCode)
	}
}

func TestShopifyWebhookSignatureIsVerified(t *testing.T) {
	cfg, repo, service := testOAuthStack(t)
	cfg.ShopifyWebhookSecret = "shopify-webhook-secret"
	app := NewServer(ServerConfig{Config: cfg, Repo: repo, OAuth: service})
	body := `{"id":"shopify-event-1","organizationId":"org-1"}`
	mac := hmac.New(sha256.New, []byte(cfg.ShopifyWebhookSecret))
	_, _ = mac.Write([]byte(body))
	signature := base64.StdEncoding.EncodeToString(mac.Sum(nil))

	req := httptest.NewRequest("POST", "/api/v1/webhooks/shopify", strings.NewReader(body))
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("X-Shopify-Hmac-Sha256", signature)
	req.Header.Set("X-Shopify-Topic", "products/update")
	resp, err := app.Test(req)
	if err != nil {
		t.Fatalf("app.Test error: %v", err)
	}
	if resp.StatusCode != 200 {
		t.Fatalf("status = %d, want 200", resp.StatusCode)
	}
}

func TestWebhookRouteIsRateLimited(t *testing.T) {
	cfg, repo, service := testOAuthStack(t)
	cfg.RateLimitEnabled = true
	cfg.RateLimitMax = 1
	cfg.RateLimitWindow = time.Minute
	app := NewServer(ServerConfig{Config: cfg, Repo: repo, OAuth: service})

	for i := 0; i < 2; i++ {
		req := httptest.NewRequest("POST", "/api/v1/webhooks/github", strings.NewReader(`{"deliveryId":"delivery-1"}`))
		req.Header.Set("Content-Type", "application/json")
		resp, err := app.Test(req)
		if err != nil {
			t.Fatalf("app.Test error: %v", err)
		}
		if i == 0 && resp.StatusCode != 200 {
			t.Fatalf("status[0] = %d, want 200", resp.StatusCode)
		}
		if i == 1 && resp.StatusCode != 429 {
			t.Fatalf("status[1] = %d, want 429", resp.StatusCode)
		}
	}
}

func TestInternalTokenBrokerRejectsBrowserAccess(t *testing.T) {
	app := testServer(t)
	req := httptest.NewRequest("POST", "/internal/connectors/token", strings.NewReader(`{"organizationId":"org-1"}`))
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("Authorization", "Bearer browser-token")
	resp, err := app.Test(req)
	if err != nil {
		t.Fatalf("app.Test error: %v", err)
	}
	if resp.StatusCode != 401 {
		t.Fatalf("status = %d, want 401", resp.StatusCode)
	}
}

func TestInternalTokenBrokerRequiresConsumer(t *testing.T) {
	app := testServer(t)
	req := httptest.NewRequest("POST", "/internal/connectors/token", strings.NewReader(`{"organizationId":"org-1","connectorType":"microsoft-graph"}`))
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("X-Internal-API-Key", "dev-key")
	resp, err := app.Test(req)
	if err != nil {
		t.Fatalf("app.Test error: %v", err)
	}
	if resp.StatusCode != 400 {
		t.Fatalf("status = %d, want 400", resp.StatusCode)
	}
}

func TestInternalTokenBrokerRejectsUnapprovedConsumer(t *testing.T) {
	cfg, repo, service := testOAuthStack(t)
	cfg.TokenLeaseConsumers = []string{"finspo-core"}
	app := NewServer(ServerConfig{Config: cfg, Repo: repo, OAuth: service})

	req := httptest.NewRequest("POST", "/internal/connectors/token", strings.NewReader(`{"organizationId":"org-1","connectorType":"microsoft-graph","consumer":"unknown-worker"}`))
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("X-Internal-API-Key", "dev-key")
	resp, err := app.Test(req)
	if err != nil {
		t.Fatalf("app.Test error: %v", err)
	}
	if resp.StatusCode != 403 {
		t.Fatalf("status = %d, want 403", resp.StatusCode)
	}
}

func TestSCIMProvisioningRequiresBearerToken(t *testing.T) {
	cfg, repo, service := testOAuthStack(t)
	cfg.SCIMBearerToken = "scim-secret"
	app := NewServer(ServerConfig{Config: cfg, Repo: repo, OAuth: service})

	req := httptest.NewRequest("POST", "/api/v1/scim/v2/Users", strings.NewReader(`{"userName":"ada@example.com"}`))
	req.Header.Set("Content-Type", "application/scim+json")
	resp, err := app.Test(req)
	if err != nil {
		t.Fatalf("app.Test error: %v", err)
	}
	if resp.StatusCode != 401 {
		t.Fatalf("status = %d, want 401", resp.StatusCode)
	}
}

func TestSCIMProvisioningStoresInboundEvent(t *testing.T) {
	cfg, repo, service := testOAuthStack(t)
	cfg.SCIMBearerToken = "scim-secret"
	app := NewServer(ServerConfig{Config: cfg, Repo: repo, OAuth: service})

	req := httptest.NewRequest("POST", "/api/v1/scim/v2/Users", strings.NewReader(`{"externalId":"u-1","userName":"ada@example.com","active":true}`))
	req.Header.Set("Content-Type", "application/scim+json")
	req.Header.Set("Authorization", "Bearer scim-secret")
	req.Header.Set("X-Org-ID", "org-1")
	resp, err := app.Test(req)
	if err != nil {
		t.Fatalf("app.Test error: %v", err)
	}
	if resp.StatusCode != 201 {
		t.Fatalf("status = %d, want 201", resp.StatusCode)
	}
	var decoded struct {
		ID       string `json:"id"`
		UserName string `json:"userName"`
	}
	if err := json.NewDecoder(resp.Body).Decode(&decoded); err != nil {
		t.Fatalf("Decode error: %v", err)
	}
	if decoded.UserName != "ada@example.com" {
		t.Fatalf("userName = %q, want ada@example.com", decoded.UserName)
	}
}

func TestSCIMTokenCRUDAndInboundAuth(t *testing.T) {
	cfg, repo, service := testOAuthStack(t)
	cfg.SCIMBearerToken = ""
	cfg.SCIMBearerTokens = map[string]string{}
	app := NewServer(ServerConfig{Config: cfg, Repo: repo, OAuth: service})

	req := httptest.NewRequest("POST", "/api/v1/scim/tokens", strings.NewReader(`{"organizationId":"org-1","name":"Okta SCIM"}`))
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("X-Internal-API-Key", "dev-key")
	resp, err := app.Test(req)
	if err != nil {
		t.Fatalf("app.Test create token error: %v", err)
	}
	if resp.StatusCode != 201 {
		t.Fatalf("create token status = %d, want 201", resp.StatusCode)
	}
	var created struct {
		Data struct {
			Token       store.SCIMToken `json:"token"`
			BearerToken string          `json:"bearerToken"`
		} `json:"data"`
	}
	if err := json.NewDecoder(resp.Body).Decode(&created); err != nil {
		t.Fatalf("Decode create token error: %v", err)
	}
	if !strings.HasPrefix(created.Data.BearerToken, "scim_") {
		t.Fatalf("bearer token = %q, want scim_ prefix", created.Data.BearerToken)
	}
	if created.Data.Token.TokenPrefix == "" || len(created.Data.Token.TokenPrefix) >= len(created.Data.BearerToken) {
		t.Fatalf("token prefix = %q, want short non-empty prefix", created.Data.Token.TokenPrefix)
	}

	req = httptest.NewRequest("GET", "/api/v1/scim/tokens?organizationId=org-1", nil)
	req.Header.Set("X-Internal-API-Key", "dev-key")
	resp, err = app.Test(req)
	if err != nil {
		t.Fatalf("app.Test list token error: %v", err)
	}
	if resp.StatusCode != 200 {
		t.Fatalf("list token status = %d, want 200", resp.StatusCode)
	}
	var listed struct {
		Data struct {
			Tokens []store.SCIMToken `json:"tokens"`
		} `json:"data"`
	}
	if err := json.NewDecoder(resp.Body).Decode(&listed); err != nil {
		t.Fatalf("Decode list token error: %v", err)
	}
	if len(listed.Data.Tokens) != 1 || listed.Data.Tokens[0].ID != created.Data.Token.ID {
		t.Fatalf("tokens = %#v, want created token", listed.Data.Tokens)
	}

	req = httptest.NewRequest("POST", "/api/v1/scim/v2/Users", strings.NewReader(`{"externalId":"u-1","userName":"ada@example.com","active":true}`))
	req.Header.Set("Content-Type", "application/scim+json")
	req.Header.Set("Authorization", "Bearer "+created.Data.BearerToken)
	req.Header.Set("X-Org-ID", "org-1")
	resp, err = app.Test(req)
	if err != nil {
		t.Fatalf("app.Test scim inbound error: %v", err)
	}
	if resp.StatusCode != 201 {
		t.Fatalf("scim inbound status = %d, want 201", resp.StatusCode)
	}
	tokenAfterUse, err := repo.FindActiveSCIMTokenByHash(t.Context(), "org-1", scimTokenHash(created.Data.BearerToken))
	if err != nil {
		t.Fatalf("FindActiveSCIMTokenByHash error: %v", err)
	}
	if tokenAfterUse.LastUsedAt == nil {
		t.Fatal("LastUsedAt was not updated after inbound SCIM auth")
	}

	req = httptest.NewRequest("DELETE", "/api/v1/scim/tokens/"+created.Data.Token.ID+"?organizationId=org-1", nil)
	req.Header.Set("X-Internal-API-Key", "dev-key")
	resp, err = app.Test(req)
	if err != nil {
		t.Fatalf("app.Test revoke token error: %v", err)
	}
	if resp.StatusCode != 200 {
		t.Fatalf("revoke status = %d, want 200", resp.StatusCode)
	}

	req = httptest.NewRequest("POST", "/api/v1/scim/v2/Users", strings.NewReader(`{"externalId":"u-2","userName":"grace@example.com","active":true}`))
	req.Header.Set("Content-Type", "application/scim+json")
	req.Header.Set("Authorization", "Bearer "+created.Data.BearerToken)
	req.Header.Set("X-Org-ID", "org-1")
	resp, err = app.Test(req)
	if err != nil {
		t.Fatalf("app.Test revoked scim inbound error: %v", err)
	}
	if resp.StatusCode != 401 {
		t.Fatalf("revoked scim status = %d, want 401", resp.StatusCode)
	}
}

func TestSCIMProvisioningUsesOrgScopedBearerToken(t *testing.T) {
	cfg, repo, service := testOAuthStack(t)
	cfg.SCIMBearerTokens = map[string]string{"org-1": "org-scim-secret"}
	app := NewServer(ServerConfig{Config: cfg, Repo: repo, OAuth: service})

	req := httptest.NewRequest("POST", "/api/v1/scim/v2/Users", strings.NewReader(`{"userName":"ada@example.com"}`))
	req.Header.Set("Content-Type", "application/scim+json")
	req.Header.Set("Authorization", "Bearer org-scim-secret")
	req.Header.Set("X-Org-ID", "org-1")
	resp, err := app.Test(req)
	if err != nil {
		t.Fatalf("app.Test error: %v", err)
	}
	if resp.StatusCode != 201 {
		t.Fatalf("status = %d, want 201", resp.StatusCode)
	}
}

func TestSCIMProvisioningRejectsWrongOrgScopedBearerToken(t *testing.T) {
	cfg, repo, service := testOAuthStack(t)
	cfg.SCIMBearerTokens = map[string]string{"org-1": "org-scim-secret"}
	app := NewServer(ServerConfig{Config: cfg, Repo: repo, OAuth: service})

	req := httptest.NewRequest("POST", "/api/v1/scim/v2/Users", strings.NewReader(`{"userName":"ada@example.com"}`))
	req.Header.Set("Content-Type", "application/scim+json")
	req.Header.Set("Authorization", "Bearer org-scim-secret")
	req.Header.Set("X-Org-ID", "org-2")
	resp, err := app.Test(req)
	if err != nil {
		t.Fatalf("app.Test error: %v", err)
	}
	if resp.StatusCode != 401 {
		t.Fatalf("status = %d, want 401", resp.StatusCode)
	}
}

func TestSCIMProvisioningRequiresOrganizationForOrgScopedBearerToken(t *testing.T) {
	cfg, repo, service := testOAuthStack(t)
	cfg.SCIMBearerTokens = map[string]string{"org-1": "org-scim-secret"}
	app := NewServer(ServerConfig{Config: cfg, Repo: repo, OAuth: service})

	req := httptest.NewRequest("POST", "/api/v1/scim/v2/Users", strings.NewReader(`{"userName":"ada@example.com"}`))
	req.Header.Set("Content-Type", "application/scim+json")
	req.Header.Set("Authorization", "Bearer org-scim-secret")
	resp, err := app.Test(req)
	if err != nil {
		t.Fatalf("app.Test error: %v", err)
	}
	if resp.StatusCode != 400 {
		t.Fatalf("status = %d, want 400", resp.StatusCode)
	}
}

func TestLegacySlackChannelsRouteUsesOrgConnection(t *testing.T) {
	providerServer := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/conversations.list" {
			t.Fatalf("path = %s, want /conversations.list", r.URL.Path)
		}
		if got := r.URL.Query().Get("types"); got != "public_channel" {
			t.Fatalf("types = %q, want public_channel", got)
		}
		if got := r.Header.Get("Authorization"); got != "Bearer access-token" {
			t.Fatalf("Authorization = %q, want Bearer access-token", got)
		}
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{"ok":true,"channels":[]}`))
	}))
	defer providerServer.Close()

	cfg := testConfig()
	cfg.SlackAPIBaseURL = providerServer.URL
	vault, err := secretcrypto.NewVault(cfg.EncryptionKey)
	if err != nil {
		t.Fatalf("NewVault error: %v", err)
	}
	encrypted, err := vault.Encrypt("access-token", []byte("conn-slack"))
	if err != nil {
		t.Fatalf("Encrypt error: %v", err)
	}
	repo := store.NewMemoryRepository()
	_, err = repo.UpsertConnection(t.Context(), store.Connection{
		ID:                   "conn-slack",
		ProviderKey:          "slack",
		ConnectorType:        "slack",
		OrganizationID:       "org-1",
		WorkspaceID:          "org-1",
		UserID:               "user-1",
		Status:               "active",
		EncryptedAccessToken: encrypted,
		AccessTokenExpiresAt: time.Now().Add(time.Hour),
	})
	if err != nil {
		t.Fatalf("UpsertConnection error: %v", err)
	}
	service := oauth.NewService(cfg, repo, vault, oauth.NewMicrosoftClient(oauth.MicrosoftClientConfig{}))
	app := NewServer(ServerConfig{
		Config:  cfg,
		Repo:    repo,
		OAuth:   service,
		Actions: actions.NewService(cfg, providerServer.Client()),
	})

	req := httptest.NewRequest("GET", "/integrations/slack/channels?organizationId=org-1&types=public_channel", nil)
	req.Header.Set("X-Internal-API-Key", "dev-key")
	resp, err := app.Test(req)
	if err != nil {
		t.Fatalf("app.Test error: %v", err)
	}
	if resp.StatusCode != 200 {
		t.Fatalf("status = %d, want 200", resp.StatusCode)
	}
}

func TestLegacyProxyRouteIsNotSupported(t *testing.T) {
	app := testServer(t)
	req := httptest.NewRequest("POST", "/integrations/slack/proxy", strings.NewReader(`{"method":"channels.list"}`))
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("X-Internal-API-Key", "dev-key")
	resp, err := app.Test(req)
	if err != nil {
		t.Fatalf("app.Test error: %v", err)
	}
	if resp.StatusCode != 410 {
		t.Fatalf("status = %d, want 410", resp.StatusCode)
	}
}

func TestGDPRExportAndDeleteRedactsAndClearsTokens(t *testing.T) {
	cfg, repo, service := testOAuthStack(t)
	vault, err := secretcrypto.NewVault(cfg.EncryptionKey)
	if err != nil {
		t.Fatalf("NewVault error: %v", err)
	}
	accessToken, err := vault.Encrypt("access-token", []byte("conn-gdpr"))
	if err != nil {
		t.Fatalf("Encrypt access error: %v", err)
	}
	refreshToken, err := vault.Encrypt("refresh-token", []byte("conn-gdpr"))
	if err != nil {
		t.Fatalf("Encrypt refresh error: %v", err)
	}
	_, _ = repo.UpsertConnection(t.Context(), store.Connection{
		ID:                    "conn-gdpr",
		ProviderKey:           "microsoft",
		ConnectorType:         "microsoft-graph",
		OrganizationID:        "org-1",
		UserID:                "user-1",
		Status:                "active",
		EncryptedAccessToken:  accessToken,
		EncryptedRefreshToken: refreshToken,
		AccessTokenExpiresAt:  time.Now().Add(time.Hour),
	})
	_, _ = repo.UpsertConnectionConsent(t.Context(), store.ConnectionConsent{
		ID:             "consent-gdpr",
		OrganizationID: "org-1",
		ConnectionID:   "conn-gdpr",
		UserID:         "user-1",
		ProviderKey:    "microsoft",
		Source:         "mail",
		Purpose:        "inbox",
		Granted:        true,
	})
	app := NewServer(ServerConfig{Config: cfg, Repo: repo, OAuth: service})

	req := httptest.NewRequest("GET", "/internal/gdpr/export?organizationId=org-1&userId=user-1", nil)
	req.Header.Set("X-Internal-API-Key", "dev-key")
	resp, err := app.Test(req)
	if err != nil {
		t.Fatalf("app.Test export error: %v", err)
	}
	if resp.StatusCode != 200 {
		t.Fatalf("export status = %d, want 200", resp.StatusCode)
	}
	body, err := io.ReadAll(resp.Body)
	if err != nil {
		t.Fatalf("ReadAll export error: %v", err)
	}
	bodyText := string(body)
	if strings.Contains(bodyText, accessToken) || strings.Contains(bodyText, refreshToken) || strings.Contains(bodyText, "EncryptedAccessToken") {
		t.Fatalf("export leaked token material: %s", bodyText)
	}

	req = httptest.NewRequest("POST", "/internal/gdpr/delete", strings.NewReader(`{"organizationId":"org-1","userId":"user-1","reason":"test"}`))
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("X-Internal-API-Key", "dev-key")
	resp, err = app.Test(req)
	if err != nil {
		t.Fatalf("app.Test delete error: %v", err)
	}
	if resp.StatusCode != 200 {
		t.Fatalf("delete status = %d, want 200", resp.StatusCode)
	}
	connection, err := repo.GetConnection(t.Context(), "conn-gdpr")
	if err != nil {
		t.Fatalf("GetConnection error: %v", err)
	}
	if connection.DeletedAt == nil || connection.Status != "deleted" {
		t.Fatalf("connection deletion state = %s/%v, want deleted", connection.Status, connection.DeletedAt)
	}
	if connection.EncryptedAccessToken != "" || connection.EncryptedRefreshToken != "" {
		t.Fatal("deleted connection retained encrypted provider token material")
	}
	consents, err := repo.ListConnectionConsents(t.Context(), "conn-gdpr")
	if err != nil {
		t.Fatalf("ListConnectionConsents error: %v", err)
	}
	if len(consents) != 1 || consents[0].Granted || consents[0].RevokedAt == nil {
		t.Fatalf("consents = %#v, want revoked consent", consents)
	}
}

func testServer(t *testing.T) *fiber.App {
	t.Helper()
	cfg, repo, service := testOAuthStack(t)
	return NewServer(ServerConfig{Config: cfg, Repo: repo, OAuth: service})
}

func testOAuthStack(t *testing.T) (config.Config, *store.MemoryRepository, *oauth.Service) {
	t.Helper()
	cfg := testConfig()
	vault, err := secretcrypto.NewVault(cfg.EncryptionKey)
	if err != nil {
		t.Fatalf("NewVault error: %v", err)
	}
	repo := store.NewMemoryRepository()
	service := oauth.NewService(cfg, repo, vault, oauth.NewMicrosoftClient(oauth.MicrosoftClientConfig{
		ClientID:         cfg.MicrosoftClientID,
		ClientSecret:     cfg.MicrosoftClientSecret,
		AuthorizationURL: cfg.MicrosoftAuthorizationURL,
		TokenURL:         cfg.MicrosoftTokenURL,
		GraphBaseURL:     cfg.MicrosoftGraphBaseURL,
	}))
	return cfg, repo, service
}

func testConfig() config.Config {
	return config.Config{
		ServiceName:               "integration-corev2",
		InternalAPIKey:            "dev-key",
		InternalAPIKeyHeader:      "X-Internal-API-Key",
		PublicBaseURL:             "http://localhost:3026",
		MicrosoftClientID:         "client",
		MicrosoftClientSecret:     "secret",
		MicrosoftAuthorizationURL: "https://login.microsoftonline.com/common/oauth2/v2.0/authorize",
		MicrosoftTokenURL:         "https://login.microsoftonline.com/common/oauth2/v2.0/token",
		MicrosoftGraphBaseURL:     "https://graph.microsoft.com",
		SessionTTL:                10 * time.Minute,
		TokenRefreshSkew:          2 * time.Minute,
		EncryptionKey:             []byte("12345678901234567890123456789012"),
	}
}

type fakeVerifier struct {
	principal auth.Principal
	err       error
}

func (f fakeVerifier) VerifyToken(context.Context, string) (auth.Principal, error) {
	if f.err != nil {
		return auth.Principal{}, f.err
	}
	return f.principal, nil
}

type fakeOrgClient struct {
	plan string
	err  error
}

func (f fakeOrgClient) GetOrgPlan(context.Context, string, string) (auth.OrgPlan, error) {
	if f.err != nil {
		return auth.OrgPlan{}, f.err
	}
	return auth.OrgPlan{Plan: f.plan, Quotas: map[string]auth.Quota{}, Entitlements: map[string]bool{}}, nil
}
