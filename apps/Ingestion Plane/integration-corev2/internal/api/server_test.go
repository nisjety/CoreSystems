package api

import (
	"context"
	"crypto/ed25519"
	"crypto/hmac"
	"crypto/rand"
	"crypto/sha256"
	"encoding/base64"
	"encoding/hex"
	"encoding/json"
	"errors"
	"io"
	"net/http"
	"net/http/httptest"
	"slices"
	"strconv"
	"strings"
	"testing"
	"time"

	"github.com/gofiber/fiber/v2"
	"github.com/triodelab/integration-corev2/internal/actions"
	"github.com/triodelab/integration-corev2/internal/attestation"
	"github.com/triodelab/integration-corev2/internal/auth"
	"github.com/triodelab/integration-corev2/internal/config"
	secretcrypto "github.com/triodelab/integration-corev2/internal/crypto"
	"github.com/triodelab/integration-corev2/internal/events"
	"github.com/triodelab/integration-corev2/internal/oauth"
	"github.com/triodelab/integration-corev2/internal/store"
	"github.com/triodelab/integration-corev2/internal/webhookorg"
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

func TestProvidersCatalogIncludesMetaSDKConfig(t *testing.T) {
	cfg, repo, service := testOAuthStack(t)
	cfg.MetaJSSDKAppID = "meta-app-id"
	cfg.MetaJSSDKAPIVersion = "v23.0"
	cfg.MetaJSSDKLocale = "en_US"
	cfg.MetaBusinessLoginConfigID = "business-config-id"
	app := NewServer(ServerConfig{Config: cfg, Repo: repo, OAuth: service})

	req := httptest.NewRequest("GET", "/api/v1/providers", nil)
	resp, err := app.Test(req)
	if err != nil {
		t.Fatalf("app.Test error: %v", err)
	}
	if resp.StatusCode != 200 {
		t.Fatalf("status = %d, want 200", resp.StatusCode)
	}

	var decoded struct {
		Data struct {
			Providers []struct {
				Key     string `json:"key"`
				MetaSDK *struct {
					Enabled       bool   `json:"enabled"`
					AppID         string `json:"appId"`
					APIVersion    string `json:"apiVersion"`
					Locale        string `json:"locale"`
					LoginConfigID string `json:"loginConfigId"`
				} `json:"metaSdk"`
			} `json:"providers"`
		} `json:"data"`
	}
	if err := json.NewDecoder(resp.Body).Decode(&decoded); err != nil {
		t.Fatalf("Decode error: %v", err)
	}
	seen := map[string]bool{}
	for _, provider := range decoded.Data.Providers {
		if provider.Key != "facebook" && provider.Key != "instagram" && provider.Key != "whatsapp" && provider.Key != "meta-ads" {
			continue
		}
		if provider.MetaSDK == nil {
			t.Fatalf("%s provider missing metaSdk config", provider.Key)
		}
		if !provider.MetaSDK.Enabled || provider.MetaSDK.AppID != "meta-app-id" || provider.MetaSDK.APIVersion != "v23.0" || provider.MetaSDK.Locale != "en_US" || provider.MetaSDK.LoginConfigID != "business-config-id" {
			t.Fatalf("%s metaSdk = %#v, want configured sdk metadata", provider.Key, provider.MetaSDK)
		}
		seen[provider.Key] = true
	}
	if !seen["facebook"] || !seen["instagram"] || !seen["whatsapp"] || !seen["meta-ads"] {
		t.Fatalf("providers with metaSdk = %#v, want facebook, instagram, whatsapp, and meta-ads", seen)
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

func TestAuditMutationIDNeverTrustsCallerRequestIDForDeduplication(t *testing.T) {
	requestContext := context.WithValue(t.Context(), requestIDContextKey{}, "req-audit-id")
	first := auditMutationID(requestContext, "capabilities", "conn-1")
	second := auditMutationID(requestContext, "capabilities", "conn-1")
	if first == second {
		t.Fatalf("caller request ID suppressed a distinct audit intent: %q", first)
	}
	if !strings.HasPrefix(first, "audit:integration:capabilities:") {
		t.Fatalf("audit ID = %q, want bounded operation prefix", first)
	}

	backgroundFirst := auditMutationID(t.Context(), "capabilities", "conn-1")
	backgroundSecond := auditMutationID(t.Context(), "capabilities", "conn-1")
	if backgroundFirst == backgroundSecond {
		t.Fatalf("background audit IDs collided: %q", backgroundFirst)
	}
}

func TestPrepareAuditEventPreservesExplicitRequestID(t *testing.T) {
	event := prepareAuditEvent(context.WithValue(t.Context(), requestIDContextKey{}, "req-context"), store.AuditEvent{
		ID: "audit-explicit", OrganizationID: "org-1", EventType: "test", RequestID: "req-explicit",
	})
	if event.RequestID != "req-explicit" {
		t.Fatalf("RequestID = %q, want explicit value", event.RequestID)
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

func TestAuditOutboxTerminalStateDegradesHealthExposesMetricsAndCanBeRequeued(t *testing.T) {
	cfg, repo, service := testOAuthStack(t)
	queue := &fakeIntegrationAuditStore{stats: store.AuditOutboxStats{
		Pending: 3, Terminal: 2, OldestPendingAge: 90 * time.Second, OldestTerminalAge: 5 * time.Minute,
	}}
	outbox := NewAuditOutbox(queue, &fakeIntegrationAuditRecorder{}, nil)
	app := NewServer(ServerConfig{Config: cfg, Repo: repo, OAuth: service, AuditOutbox: outbox})

	healthRequest := httptest.NewRequest("GET", "/health/detailed", nil)
	healthResponse, err := app.Test(healthRequest)
	if err != nil {
		t.Fatalf("health request error: %v", err)
	}
	if healthResponse.StatusCode != fiber.StatusServiceUnavailable {
		t.Fatalf("health status = %d, want 503", healthResponse.StatusCode)
	}
	var healthBody struct {
		Status      string `json:"status"`
		AuditOutbox struct {
			Terminal int `json:"terminal"`
		} `json:"auditOutbox"`
	}
	if err := json.NewDecoder(healthResponse.Body).Decode(&healthBody); err != nil {
		t.Fatalf("decode health: %v", err)
	}
	if healthBody.Status != "degraded" || healthBody.AuditOutbox.Terminal != 2 {
		t.Fatalf("health = %#v, want terminal degradation", healthBody)
	}

	metricsRequest := httptest.NewRequest("GET", "/metrics", nil)
	metricsRequest.Header.Set("X-Internal-API-Key", "dev-key")
	metricsResponse, err := app.Test(metricsRequest)
	if err != nil {
		t.Fatalf("metrics request error: %v", err)
	}
	metricsBody, err := io.ReadAll(metricsResponse.Body)
	if err != nil {
		t.Fatalf("read metrics: %v", err)
	}
	metricsText := string(metricsBody)
	for _, expected := range []string{
		"integration_audit_outbox_pending 3",
		"integration_audit_outbox_terminal 2",
		"integration_audit_outbox_oldest_pending_seconds 90",
		"integration_audit_outbox_oldest_terminal_seconds 300",
	} {
		if !strings.Contains(metricsText, expected) {
			t.Fatalf("metrics missing %q: %s", expected, metricsText)
		}
	}

	requeueRequest := httptest.NewRequest("POST", "/internal/audit-outbox/requeue", strings.NewReader(`{"eventIds":["audit-1","audit-2"]}`))
	requeueRequest.Header.Set("Content-Type", "application/json")
	requeueRequest.Header.Set("X-Internal-API-Key", "dev-key")
	requeueResponse, err := app.Test(requeueRequest)
	if err != nil {
		t.Fatalf("requeue request error: %v", err)
	}
	if requeueResponse.StatusCode != fiber.StatusOK || len(queue.requeued) != 2 {
		t.Fatalf("requeue status/ids = %d/%#v, want 200/two", requeueResponse.StatusCode, queue.requeued)
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

func TestConnectionsListFiltersByProviderCategory(t *testing.T) {
	cfg, repo, service := testOAuthStack(t)
	for _, connection := range []store.Connection{
		{
			ID:                   "conn-ms",
			ProviderKey:          "microsoft",
			ConnectorType:        "microsoft-graph",
			OrganizationID:       "org-1",
			UserID:               "user-1",
			Status:               "active",
			AccessTokenExpiresAt: time.Now().Add(time.Hour),
		},
		{
			ID:                   "conn-linkedin",
			ProviderKey:          "linkedin",
			ConnectorType:        "linkedin",
			OrganizationID:       "org-1",
			UserID:               "user-1",
			Status:               "active",
			AccessTokenExpiresAt: time.Now().Add(time.Hour),
		},
		{
			ID:                   "conn-x",
			ProviderKey:          "x",
			ConnectorType:        "x",
			OrganizationID:       "org-1",
			UserID:               "user-2",
			Status:               "active",
			AccessTokenExpiresAt: time.Now().Add(time.Hour),
		},
	} {
		if _, err := repo.UpsertConnection(t.Context(), connection); err != nil {
			t.Fatalf("UpsertConnection(%s) error: %v", connection.ID, err)
		}
	}
	app := NewServer(ServerConfig{Config: cfg, Repo: repo, OAuth: service})

	req := httptest.NewRequest("GET", "/api/v1/connections?organizationId=org-1&category=social", nil)
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
			Connections []store.Connection `json:"connections"`
		} `json:"data"`
	}
	if err := json.NewDecoder(resp.Body).Decode(&decoded); err != nil {
		t.Fatalf("Decode error: %v", err)
	}
	if len(decoded.Data.Connections) != 2 {
		t.Fatalf("connections = %#v, want 2 social connections", decoded.Data.Connections)
	}
	for _, connection := range decoded.Data.Connections {
		if connection.ProviderKey != "linkedin" && connection.ProviderKey != "x" {
			t.Fatalf("connection provider = %s, want only social providers", connection.ProviderKey)
		}
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
		Config: cfg,
		Repo:   repo,
		OAuth:  service,
		Auth: fakeVerifier{principal: auth.Principal{
			UserID: "conversation-core", OrganizationID: "org-1", PrincipalType: "service", Scopes: []string{"integration:write"},
		}},
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

func TestWriteActionIdempotencyReceiptPreventsDuplicateProviderSend(t *testing.T) {
	providerCalls := 0
	providerServer := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		providerCalls++
		w.WriteHeader(http.StatusAccepted)
	}))
	defer providerServer.Close()

	cfg, repo, service := testOAuthStack(t)
	publicKey, privateKey, err := ed25519.GenerateKey(rand.Reader)
	if err != nil {
		t.Fatalf("GenerateKey error: %v", err)
	}
	cfg.ProviderWriteAttestationKeysJSON = apiTrustedKeysJSON(t, publicKey)
	cfg.AllowLegacyTenantKey = false
	cfg.MicrosoftGraphBaseURL = providerServer.URL
	vault, err := secretcrypto.NewVault(cfg.EncryptionKey)
	if err != nil {
		t.Fatalf("NewVault error: %v", err)
	}
	encryptedToken, err := vault.Encrypt("access-token", []byte("conn-ms-idempotent"))
	if err != nil {
		t.Fatalf("Encrypt error: %v", err)
	}
	_, _ = repo.UpsertConnection(t.Context(), store.Connection{
		ID:                   "conn-ms-idempotent",
		ProviderKey:          "microsoft",
		ConnectorType:        "microsoft-graph",
		OrganizationID:       "org-1",
		UserID:               "user-1",
		Status:               "active",
		Capabilities:         []string{"mail.send"},
		EncryptedAccessToken: encryptedToken,
		AccessTokenExpiresAt: time.Now().Add(time.Hour),
	})
	app := NewServer(ServerConfig{
		Config: cfg,
		Repo:   repo,
		OAuth:  service,
		Auth: fakeVerifier{principal: auth.Principal{
			UserID: "conversation-core", OrganizationID: "org-1", PrincipalType: "service", Scopes: []string{"integration:write"},
		}},
		Actions: actions.NewService(cfg, providerServer.Client()),
	})

	call := func(payload string) *http.Response {
		t.Helper()
		req := httptest.NewRequest("POST", "/api/v1/connections/conn-ms-idempotent/actions", strings.NewReader(payload))
		req.Header.Set("Content-Type", "application/json")
		req.Header.Set("Authorization", "Bearer conversation-service-token")
		resp, err := app.Test(req)
		if err != nil {
			t.Fatalf("app.Test error: %v", err)
		}
		return resp
	}

	idempotencyKey := "conversation:org-1:human-reply-1"
	body := map[string]any{"message": map[string]any{"subject": "hello"}}
	binding := attestation.Binding{
		PresenterService: "conversation-core", OrganizationID: "org-1", ConnectionID: "conn-ms-idempotent",
		ProviderKey: "microsoft", Operation: "mail.send", Body: body, IdempotencyKey: idempotencyKey,
	}
	claims := apiHumanIntentClaims(t, binding)
	claims.AuthorizationID = "human-reply-1"
	claims.ActionID = claims.AuthorizationID
	claims.JWTID = "attestation-human-reply-1"
	payloadBytes, _ := json.Marshal(map[string]any{
		"operation": "mail.send", "writeAttestation": signAPIWriteAttestation(t, privateKey, claims),
		"idempotencyKey": idempotencyKey, "body": body,
	})
	payload := string(payloadBytes)
	for attempt := 1; attempt <= 2; attempt++ {
		resp := call(payload)
		if resp.StatusCode != fiber.StatusOK {
			responseBody, _ := io.ReadAll(resp.Body)
			_ = resp.Body.Close()
			t.Fatalf("attempt %d status = %d body=%s, want 200", attempt, resp.StatusCode, responseBody)
		}
		_ = resp.Body.Close()
	}
	if providerCalls != 1 {
		t.Fatalf("provider calls = %d, want exactly 1 across an idempotent retry", providerCalls)
	}

	changedBody := map[string]any{"message": map[string]any{"subject": "changed"}}
	changedBinding := binding
	changedBinding.Body = changedBody
	changedClaims := claims
	changedClaims.PayloadSHA256, err = attestation.PayloadSHA256(changedBinding)
	if err != nil {
		t.Fatalf("PayloadSHA256 changed error: %v", err)
	}
	changedPayload, _ := json.Marshal(map[string]any{
		"operation": "mail.send", "writeAttestation": signAPIWriteAttestation(t, privateKey, changedClaims),
		"idempotencyKey": idempotencyKey, "body": changedBody,
	})
	changed := call(string(changedPayload))
	if changed.StatusCode != fiber.StatusConflict {
		defer changed.Body.Close()
		t.Fatalf("changed-payload status = %d, want 409 idempotency conflict", changed.StatusCode)
	}
	if code := readAPIErrorCode(t, changed); code != "idempotency_conflict" {
		t.Fatalf("changed-payload error code = %q, want idempotency_conflict", code)
	}
	if providerCalls != 1 {
		t.Fatalf("provider calls after changed payload = %d, want 1", providerCalls)
	}
}

func TestActionAuditEventIDSeparatesAuthorizedReceiptsForIdenticalPayload(t *testing.T) {
	first := store.ActionReceipt{
		OrganizationID: "org-1", IdempotencyKey: "reply:first-authorized-action",
		RequestSHA256: "same-payload", ConnectionID: "connection-1", ProviderKey: "microsoft",
		Operation: "mail.send", AttestationIssuer: "model-plane", AuthorizationKind: "human_approved_ai_action",
		AuthorizationID: "authorization-1", ApprovalID: "approval-1", ActionID: "action-1",
		ActorID: "user-1", PayloadSHA256: "same-payload",
	}
	second := first
	second.IdempotencyKey = "reply:second-authorized-action"
	second.AuthorizationID = "authorization-2"
	second.ApprovalID = "approval-2"
	second.ActionID = "action-2"

	firstRequested := actionAuditEventID("requested", first)
	if firstRequested == actionAuditEventID("requested", second) {
		t.Fatal("distinct durable authorization receipts collapsed to one audit event id")
	}
	if firstRequested == actionAuditEventID("executed", first) {
		t.Fatal("requested and executed stages collapsed to one audit event id")
	}
	rotatedSigner := first
	rotatedSigner.AttestationKeyID = "rotated-key"
	rotatedSigner.AttestationJTI = "fresh-jti"
	if firstRequested != actionAuditEventID("requested", rotatedSigner) {
		t.Fatal("signer rotation changed the durable action audit identity")
	}
}

func TestWriteActionProviderFailureBecomesUnknownAndBlocksBlindRetry(t *testing.T) {
	providerCalls := 0
	providerServer := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		providerCalls++
		w.WriteHeader(http.StatusServiceUnavailable)
	}))
	defer providerServer.Close()

	cfg, repo, service := testOAuthStack(t)
	publicKey, privateKey, err := ed25519.GenerateKey(rand.Reader)
	if err != nil {
		t.Fatalf("GenerateKey error: %v", err)
	}
	cfg.ProviderWriteAttestationKeysJSON = apiTrustedKeysJSON(t, publicKey)
	cfg.AllowLegacyTenantKey = false
	cfg.MicrosoftGraphBaseURL = providerServer.URL
	vault, err := secretcrypto.NewVault(cfg.EncryptionKey)
	if err != nil {
		t.Fatalf("NewVault error: %v", err)
	}
	encryptedToken, err := vault.Encrypt("access-token", []byte("conn-ms-unknown"))
	if err != nil {
		t.Fatalf("Encrypt error: %v", err)
	}
	_, err = repo.UpsertConnection(t.Context(), store.Connection{
		ID:                   "conn-ms-unknown",
		ProviderKey:          "microsoft",
		ConnectorType:        "microsoft-graph",
		OrganizationID:       "org-1",
		UserID:               "user-1",
		Status:               "active",
		Capabilities:         []string{"mail.send"},
		EncryptedAccessToken: encryptedToken,
		AccessTokenExpiresAt: time.Now().Add(time.Hour),
	})
	if err != nil {
		t.Fatalf("UpsertConnection error: %v", err)
	}
	app := NewServer(ServerConfig{
		Config: cfg,
		Repo:   repo,
		OAuth:  service,
		Auth: fakeVerifier{principal: auth.Principal{
			UserID: "conversation-core", OrganizationID: "org-1", PrincipalType: "service", Scopes: []string{"integration:write"},
		}},
		Actions: actions.NewService(cfg, providerServer.Client()),
	})
	idempotencyKey := "conversation:org-1:human-reply-unknown"
	body := map[string]any{"message": map[string]any{"subject": "hello"}}
	binding := attestation.Binding{
		PresenterService: "conversation-core", OrganizationID: "org-1", ConnectionID: "conn-ms-unknown",
		ProviderKey: "microsoft", Operation: "mail.send", Body: body, IdempotencyKey: idempotencyKey,
	}
	claims := apiHumanIntentClaims(t, binding)
	claims.AuthorizationID = "human-reply-unknown"
	claims.ActionID = claims.AuthorizationID
	claims.JWTID = "attestation-human-reply-unknown"
	payloadBytes, _ := json.Marshal(map[string]any{
		"operation": "mail.send", "writeAttestation": signAPIWriteAttestation(t, privateKey, claims),
		"idempotencyKey": idempotencyKey, "body": body,
	})
	payload := string(payloadBytes)

	call := func() *http.Response {
		t.Helper()
		req := httptest.NewRequest("POST", "/api/v1/connections/conn-ms-unknown/actions", strings.NewReader(payload))
		req.Header.Set("Content-Type", "application/json")
		req.Header.Set("Authorization", "Bearer conversation-service-token")
		resp, err := app.Test(req)
		if err != nil {
			t.Fatalf("app.Test error: %v", err)
		}
		return resp
	}

	first := call()
	if first.StatusCode != fiber.StatusBadGateway {
		defer first.Body.Close()
		t.Fatalf("first status = %d, want 502 provider failure", first.StatusCode)
	}
	if code := readAPIErrorCode(t, first); code != "action_failed" {
		t.Fatalf("first error code = %q, want action_failed", code)
	}

	retry := call()
	if retry.StatusCode != fiber.StatusConflict {
		defer retry.Body.Close()
		t.Fatalf("retry status = %d, want 409 unknown outcome", retry.StatusCode)
	}
	if code := readAPIErrorCode(t, retry); code != "action_outcome_unknown" {
		t.Fatalf("retry error code = %q, want action_outcome_unknown", code)
	}
	if providerCalls != 1 {
		t.Fatalf("provider calls = %d, want exactly 1 after ambiguous failure and retry", providerCalls)
	}
}

func TestWriteActionRequiresValidIdempotencyKeyBeforeTokenOrProvider(t *testing.T) {
	providerCalled := false
	providerServer := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		providerCalled = true
		w.WriteHeader(http.StatusOK)
	}))
	defer providerServer.Close()

	cfg, repo, service := testOAuthStack(t)
	cfg.MicrosoftGraphBaseURL = providerServer.URL
	_, err := repo.UpsertConnection(t.Context(), store.Connection{
		ID:             "conn-ms-missing-idempotency",
		ProviderKey:    "microsoft",
		ConnectorType:  "microsoft-graph",
		OrganizationID: "org-1",
		UserID:         "user-1",
		Status:         "active",
		Capabilities:   []string{"mail.send"},
	})
	if err != nil {
		t.Fatalf("UpsertConnection error: %v", err)
	}
	app := NewServer(ServerConfig{
		Config: cfg,
		Repo:   repo,
		OAuth:  service,
		Auth: fakeVerifier{principal: auth.Principal{
			UserID: "conversation-core", OrganizationID: "org-1", PrincipalType: "service", Scopes: []string{"integration:write"},
		}},
		Actions: actions.NewService(cfg, providerServer.Client()),
	})
	req := httptest.NewRequest("POST", "/api/v1/connections/conn-ms-missing-idempotency/actions", strings.NewReader(
		`{"operation":"mail.send","approvalId":"human-reply-1","body":{"message":{"subject":"hello"}}}`,
	))
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("Authorization", "Bearer conversation-service-token")
	response, err := app.Test(req)
	if err != nil {
		t.Fatalf("app.Test error: %v", err)
	}
	if response.StatusCode != fiber.StatusBadRequest {
		defer response.Body.Close()
		t.Fatalf("status = %d, want 400", response.StatusCode)
	}
	if code := readAPIErrorCode(t, response); code != "idempotency_key_required" {
		t.Fatalf("error code = %q, want idempotency_key_required", code)
	}
	if providerCalled {
		t.Fatal("provider was called without an idempotency key")
	}
}

type completeFailureRepository struct {
	store.Repository
}

type completeFailureTransaction struct {
	store.AuditTransaction
}

func (completeFailureTransaction) CompleteActionReceipt(context.Context, string, string, string) (store.ActionReceipt, error) {
	return store.ActionReceipt{}, errors.New("receipt finalize unavailable")
}

func (completeFailureRepository) CompleteActionReceipt(context.Context, string, string, string) (store.ActionReceipt, error) {
	return store.ActionReceipt{}, errors.New("receipt finalize unavailable")
}

func (r completeFailureRepository) WithAuditTransaction(ctx context.Context, fn func(store.AuditTransaction) error) error {
	return r.Repository.WithAuditTransaction(ctx, func(tx store.AuditTransaction) error {
		return fn(completeFailureTransaction{AuditTransaction: tx})
	})
}

func TestWriteActionReceiptFinalizeFailureBlocksBlindRetry(t *testing.T) {
	providerCalls := 0
	providerServer := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		providerCalls++
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{"id":"provider-message-1"}`))
	}))
	defer providerServer.Close()

	cfg, repo, service := testOAuthStack(t)
	publicKey, privateKey, err := ed25519.GenerateKey(rand.Reader)
	if err != nil {
		t.Fatalf("GenerateKey error: %v", err)
	}
	cfg.ProviderWriteAttestationKeysJSON = apiTrustedKeysJSON(t, publicKey)
	cfg.AllowLegacyTenantKey = false
	cfg.MicrosoftGraphBaseURL = providerServer.URL
	vault, err := secretcrypto.NewVault(cfg.EncryptionKey)
	if err != nil {
		t.Fatalf("NewVault error: %v", err)
	}
	encryptedToken, err := vault.Encrypt("access-token", []byte("conn-ms-finalize-failure"))
	if err != nil {
		t.Fatalf("Encrypt error: %v", err)
	}
	_, err = repo.UpsertConnection(t.Context(), store.Connection{
		ID:                   "conn-ms-finalize-failure",
		ProviderKey:          "microsoft",
		ConnectorType:        "microsoft-graph",
		OrganizationID:       "org-1",
		UserID:               "user-1",
		Status:               "active",
		Capabilities:         []string{"mail.send"},
		EncryptedAccessToken: encryptedToken,
		AccessTokenExpiresAt: time.Now().Add(time.Hour),
	})
	if err != nil {
		t.Fatalf("UpsertConnection error: %v", err)
	}
	app := NewServer(ServerConfig{
		Config: cfg,
		Repo:   completeFailureRepository{Repository: repo},
		OAuth:  service,
		Auth: fakeVerifier{principal: auth.Principal{
			UserID: "conversation-core", OrganizationID: "org-1", PrincipalType: "service", Scopes: []string{"integration:write"},
		}},
		Actions: actions.NewService(cfg, providerServer.Client()),
	})
	idempotencyKey := "conversation:org-1:human-reply-finalize"
	body := map[string]any{"message": map[string]any{"subject": "hello"}}
	binding := attestation.Binding{
		PresenterService: "conversation-core", OrganizationID: "org-1", ConnectionID: "conn-ms-finalize-failure",
		ProviderKey: "microsoft", Operation: "mail.send", Body: body, IdempotencyKey: idempotencyKey,
	}
	claims := apiHumanIntentClaims(t, binding)
	claims.AuthorizationID = "human-reply-finalize"
	claims.ActionID = claims.AuthorizationID
	claims.JWTID = "attestation-human-reply-finalize"
	payloadBytes, _ := json.Marshal(map[string]any{
		"operation": "mail.send", "writeAttestation": signAPIWriteAttestation(t, privateKey, claims),
		"idempotencyKey": idempotencyKey, "body": body,
	})
	payload := string(payloadBytes)
	for attempt := 1; attempt <= 2; attempt++ {
		req := httptest.NewRequest("POST", "/api/v1/connections/conn-ms-finalize-failure/actions", strings.NewReader(payload))
		req.Header.Set("Content-Type", "application/json")
		req.Header.Set("Authorization", "Bearer conversation-service-token")
		response, err := app.Test(req)
		if err != nil {
			t.Fatalf("attempt %d app.Test error: %v", attempt, err)
		}
		if response.StatusCode != fiber.StatusConflict {
			defer response.Body.Close()
			t.Fatalf("attempt %d status = %d, want 409", attempt, response.StatusCode)
		}
		if code := readAPIErrorCode(t, response); code != "action_outcome_unknown" {
			t.Fatalf("attempt %d error code = %q, want action_outcome_unknown", attempt, code)
		}
	}
	if providerCalls != 1 {
		t.Fatalf("provider calls = %d, want exactly 1 after receipt finalize failure and retry", providerCalls)
	}
	sawUnknownAudit := false
	for attempt := 0; attempt < 3; attempt++ {
		event, found, err := repo.ClaimAuditEvent(t.Context())
		if err != nil {
			t.Fatalf("ClaimAuditEvent: %v", err)
		}
		if !found {
			break
		}
		if event.EventType == "connection.action.unknown" {
			sawUnknownAudit = true
		}
	}
	if !sawUnknownAudit {
		t.Fatal("stale executing receipt was not durably reconciled to an unknown audit stage")
	}
}

func TestWriteActionAuditIntentFailureStopsBeforeProviderAndRemainsRetryable(t *testing.T) {
	providerCalls := 0
	providerServer := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		providerCalls++
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{"id":"provider-message-unexpected"}`))
	}))
	defer providerServer.Close()

	cfg, repo, service := testOAuthStack(t)
	publicKey, privateKey, err := ed25519.GenerateKey(rand.Reader)
	if err != nil {
		t.Fatalf("GenerateKey error: %v", err)
	}
	cfg.ProviderWriteAttestationKeysJSON = apiTrustedKeysJSON(t, publicKey)
	cfg.AllowLegacyTenantKey = false
	cfg.MicrosoftGraphBaseURL = providerServer.URL
	vault, err := secretcrypto.NewVault(cfg.EncryptionKey)
	if err != nil {
		t.Fatalf("NewVault error: %v", err)
	}
	encryptedToken, err := vault.Encrypt("access-token", []byte("conn-ms-audit-preflight"))
	if err != nil {
		t.Fatalf("Encrypt error: %v", err)
	}
	_, err = repo.UpsertConnection(t.Context(), store.Connection{
		ID: "conn-ms-audit-preflight", ProviderKey: "microsoft", ConnectorType: "microsoft-graph",
		OrganizationID: "org-1", UserID: "user-1", Status: "active",
		Capabilities: []string{"mail.send"}, EncryptedAccessToken: encryptedToken,
		AccessTokenExpiresAt: time.Now().Add(time.Hour),
	})
	if err != nil {
		t.Fatalf("UpsertConnection error: %v", err)
	}
	app := NewServer(ServerConfig{
		Config: cfg,
		Repo:   auditInsertFailureRepository{Repository: repo},
		OAuth:  service,
		Auth: fakeVerifier{principal: auth.Principal{
			UserID: "conversation-core", OrganizationID: "org-1", PrincipalType: "service", Scopes: []string{"integration:write"},
		}},
		Actions: actions.NewService(cfg, providerServer.Client()),
	})
	idempotencyKey := "conversation:org-1:audit-preflight"
	body := map[string]any{"message": map[string]any{"subject": "hello"}}
	binding := attestation.Binding{
		PresenterService: "conversation-core", OrganizationID: "org-1", ConnectionID: "conn-ms-audit-preflight",
		ProviderKey: "microsoft", Operation: "mail.send", Body: body, IdempotencyKey: idempotencyKey,
	}
	claims := apiHumanIntentClaims(t, binding)
	claims.AuthorizationID = "human-reply-audit-preflight"
	claims.ActionID = claims.AuthorizationID
	claims.JWTID = "attestation-audit-preflight"
	payloadBytes, _ := json.Marshal(map[string]any{
		"operation": "mail.send", "writeAttestation": signAPIWriteAttestation(t, privateKey, claims),
		"idempotencyKey": idempotencyKey, "body": body,
	})
	for attempt := 1; attempt <= 2; attempt++ {
		req := httptest.NewRequest("POST", "/api/v1/connections/conn-ms-audit-preflight/actions", strings.NewReader(string(payloadBytes)))
		req.Header.Set("Content-Type", "application/json")
		req.Header.Set("Authorization", "Bearer conversation-service-token")
		response, requestErr := app.Test(req)
		if requestErr != nil {
			t.Fatalf("attempt %d app.Test error: %v", attempt, requestErr)
		}
		if response.StatusCode != fiber.StatusServiceUnavailable {
			defer response.Body.Close()
			t.Fatalf("attempt %d status = %d, want 503", attempt, response.StatusCode)
		}
		if code := readAPIErrorCode(t, response); code != "action_pre_provider_retryable" {
			t.Fatalf("attempt %d error code = %q", attempt, code)
		}
	}
	if providerCalls != 0 {
		t.Fatalf("provider calls = %d, want zero when durable intent is unavailable", providerCalls)
	}
}

func TestMarkActionOutcomeUnknownRequiresExecutingReceipt(t *testing.T) {
	repo := store.NewMemoryRepository()
	err := markActionOutcomeUnknown(t.Context(), ServerConfig{Repo: repo}, store.Connection{
		ID: "conn-missing-receipt", OrganizationID: "org-1", UserID: "user-1", ProviderKey: "microsoft",
	}, store.ActionReceipt{
		OrganizationID: "org-1", IdempotencyKey: "missing-idempotency-key",
		ConnectionID: "conn-missing-receipt", Operation: "mail.send",
	})
	if !errors.Is(err, store.ErrNotFound) {
		t.Fatalf("markActionOutcomeUnknown error = %v, want ErrNotFound", err)
	}
}

func readAPIErrorCode(t *testing.T, response *http.Response) string {
	t.Helper()
	defer response.Body.Close()
	var decoded struct {
		Error struct {
			Code string `json:"code"`
		} `json:"error"`
	}
	if err := json.NewDecoder(response.Body).Decode(&decoded); err != nil {
		t.Fatalf("decode API error: %v", err)
	}
	return decoded.Error.Code
}

func TestUserBearerCannotSelfAssertProviderWriteApproval(t *testing.T) {
	called := false
	providerServer := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		called = true
		w.WriteHeader(http.StatusOK)
	}))
	defer providerServer.Close()

	cfg, repo, service := testOAuthStack(t)
	cfg.AllowLegacyTenantKey = false
	cfg.MicrosoftGraphBaseURL = providerServer.URL
	_, _ = repo.UpsertConnection(t.Context(), store.Connection{
		ID:                   "conn-ms-user-write",
		ProviderKey:          "microsoft",
		ConnectorType:        "microsoft-graph",
		OrganizationID:       "org-1",
		UserID:               "user-1",
		Status:               "active",
		Capabilities:         []string{"mail.send"},
		AccessTokenExpiresAt: time.Now().Add(time.Hour),
	})
	app := NewServer(ServerConfig{
		Config: cfg,
		Repo:   repo,
		OAuth:  service,
		Auth: fakeVerifier{principal: auth.Principal{
			UserID: "user-1", OrganizationID: "org-1", Role: "member",
		}},
		Actions: actions.NewService(cfg, providerServer.Client()),
	})

	req := httptest.NewRequest("POST", "/api/v1/connections/conn-ms-user-write/actions", strings.NewReader(`{"operation":"mail.send","body":{"approvalId":"caller-made-up"}}`))
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("Authorization", "Bearer verified-user-token")
	resp, err := app.Test(req)
	if err != nil {
		t.Fatalf("app.Test error: %v", err)
	}
	if resp.StatusCode != fiber.StatusForbidden {
		t.Fatalf("status = %d, want 403", resp.StatusCode)
	}
	if called {
		t.Fatal("provider was called for a user-supplied approval marker")
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

func TestSyncCancellationRollsBackWhenAuditIntentCannotPersist(t *testing.T) {
	cfg, repo, service := testOAuthStack(t)
	_, err := repo.UpsertConnection(t.Context(), store.Connection{
		ID: "conn-sync-cancel-rollback", ProviderKey: "notion", ConnectorType: "notion",
		OrganizationID: "org-1", UserID: "user-1", Status: "active",
	})
	if err != nil {
		t.Fatalf("UpsertConnection error: %v", err)
	}
	job, err := repo.CreateSyncJob(t.Context(), store.SyncJob{
		ID: "sync-cancel-rollback", OrganizationID: "org-1", ConnectionID: "conn-sync-cancel-rollback",
		UserID: "user-1", ProviderKey: "notion", Status: "handoff_data_plane", CreatedAt: time.Now().UTC(),
	})
	if err != nil {
		t.Fatalf("CreateSyncJob error: %v", err)
	}
	app := NewServer(ServerConfig{
		Config: cfg,
		Repo:   auditInsertFailureRepository{Repository: repo},
		OAuth:  service,
	})

	req := httptest.NewRequest("POST", "/api/v1/sync-jobs/"+job.ID+"/cancel", strings.NewReader(`{"reason":"rollback-test"}`))
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("X-Internal-API-Key", "dev-key")
	resp, err := app.Test(req)
	if err != nil {
		t.Fatalf("app.Test cancel error: %v", err)
	}
	defer resp.Body.Close()
	if resp.StatusCode != fiber.StatusInternalServerError {
		t.Fatalf("cancel status = %d, want 500", resp.StatusCode)
	}
	unchanged, err := repo.GetSyncJob(t.Context(), job.ID)
	if err != nil {
		t.Fatalf("GetSyncJob error: %v", err)
	}
	if unchanged.Status != "handoff_data_plane" || unchanged.CompletedAt != nil {
		t.Fatalf("sync cancellation committed without audit: status=%q completed=%v", unchanged.Status, unchanged.CompletedAt)
	}
}

func TestCancelSyncJobHandlesDefaultReasonAndTerminalReplay(t *testing.T) {
	repo := store.NewMemoryRepository()
	job, err := repo.CreateSyncJob(t.Context(), store.SyncJob{
		ID: "sync-cancel-default", OrganizationID: "org-1", ConnectionID: "conn-1",
		UserID: "user-1", ProviderKey: "notion", Status: "handoff_data_plane", CreatedAt: time.Now().UTC(),
	})
	if err != nil {
		t.Fatalf("CreateSyncJob error: %v", err)
	}
	cancelled, err := cancelSyncJob(t.Context(), ServerConfig{Repo: repo}, job, "")
	if err != nil {
		t.Fatalf("cancelSyncJob error: %v", err)
	}
	if cancelled.Metadata["cancelReason"] != "user_requested" {
		t.Fatalf("cancel reason = %#v, want user_requested", cancelled.Metadata["cancelReason"])
	}
	replayed, err := cancelSyncJob(t.Context(), ServerConfig{Repo: repo}, cancelled, "ignored")
	if err != nil {
		t.Fatalf("terminal replay error: %v", err)
	}
	if replayed.ID != cancelled.ID || replayed.Status != "cancelled" {
		t.Fatalf("terminal replay changed job: %#v", replayed)
	}
	if _, err := cancelSyncJob(t.Context(), ServerConfig{Repo: repo}, store.SyncJob{ID: "missing", Status: "running"}, "test"); !errors.Is(err, store.ErrNotFound) {
		t.Fatalf("missing job error = %v, want ErrNotFound", err)
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

func TestExtendTeamsInboxHistoryQueuesNextThirtyDays(t *testing.T) {
	cfg, repo, service := testOAuthStack(t)
	_, _ = repo.UpsertConnection(t.Context(), store.Connection{
		ID: "conn-ms", ProviderKey: "microsoft", OrganizationID: "org-1", UserID: "user-1",
		Status: "active", Capabilities: []string{"teams.messages.read"},
	})
	app := NewServer(ServerConfig{Config: cfg, Repo: repo, OAuth: service})

	for requestNumber, wantDays := range []int{60, 90} {
		req := httptest.NewRequest("POST", "/api/v1/connections/conn-ms/inbox-history", nil)
		req.Header.Set("X-Internal-API-Key", "dev-key")
		resp, err := app.Test(req)
		if err != nil {
			t.Fatalf("request %d error: %v", requestNumber+1, err)
		}
		if resp.StatusCode != fiber.StatusAccepted {
			t.Fatalf("request %d status = %d, want 202", requestNumber+1, resp.StatusCode)
		}
		var decoded struct {
			Data struct {
				History struct {
					Channel     string `json:"channel"`
					HistoryDays int    `json:"historyDays"`
					Queued      bool   `json:"queued"`
				} `json:"history"`
			} `json:"data"`
		}
		if err := json.NewDecoder(resp.Body).Decode(&decoded); err != nil {
			t.Fatalf("decode request %d: %v", requestNumber+1, err)
		}
		if decoded.Data.History.Channel != "teams" || !decoded.Data.History.Queued || decoded.Data.History.HistoryDays != wantDays {
			t.Fatalf("request %d history = %#v, want Teams %d days queued", requestNumber+1, decoded.Data.History, wantDays)
		}
	}
}

func TestExtendTeamsInboxHistoryRejectsUnprivilegedNonOwner(t *testing.T) {
	cfg, repo, service := testOAuthStack(t)
	_, _ = repo.UpsertConnection(t.Context(), store.Connection{
		ID: "conn-private-ms", ProviderKey: "microsoft", OrganizationID: "org-1", UserID: "connection-owner",
		Status: "active", Capabilities: []string{"teams.messages.read"},
	})
	app := NewServer(ServerConfig{
		Config: cfg, Repo: repo, OAuth: service,
		Auth: fakeVerifier{principal: auth.Principal{
			UserID: "other-member", OrganizationID: "org-1", Role: "member", PrincipalType: "user",
		}},
	})

	req := httptest.NewRequest("POST", "/api/v1/connections/conn-private-ms/inbox-history", nil)
	req.Header.Set("Authorization", "Bearer member-token")
	resp, err := app.Test(req)
	if err != nil {
		t.Fatalf("app.Test error: %v", err)
	}
	if resp.StatusCode != fiber.StatusForbidden {
		t.Fatalf("status = %d, want 403", resp.StatusCode)
	}
	if _, err := repo.GetEmailSyncState(t.Context(), "conn-private-ms:teams"); !errors.Is(err, store.ErrNotFound) {
		t.Fatalf("forbidden request persisted history state: %v", err)
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
	_, _ = repo.UpsertConnection(t.Context(), store.Connection{
		ID: "conn-slack", ProviderKey: "slack", OrganizationID: "org-1", TenantID: "team-1", Status: "active",
	})
	app := NewServer(ServerConfig{Config: cfg, Repo: repo, OAuth: service, WebhookOrg: &webhookorg.Resolver{Store: repo}})
	body := `{"type":"event_callback","team_id":"team-1","event":{"type":"team_join"}}`
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

func TestMetaWebhookChallengeIsVerified(t *testing.T) {
	cfg, repo, service := testOAuthStack(t)
	cfg.MetaWebhookVerifyToken = "verify-token"
	app := NewServer(ServerConfig{Config: cfg, Repo: repo, OAuth: service})

	req := httptest.NewRequest("GET", "/api/v1/webhooks/meta?hub.mode=subscribe&hub.verify_token=verify-token&hub.challenge=challenge-123", nil)
	resp, err := app.Test(req)
	if err != nil {
		t.Fatalf("app.Test error: %v", err)
	}
	if resp.StatusCode != 200 {
		t.Fatalf("status = %d, want 200", resp.StatusCode)
	}
	body, err := io.ReadAll(resp.Body)
	if err != nil {
		t.Fatalf("ReadAll error: %v", err)
	}
	if string(body) != "challenge-123" {
		t.Fatalf("body = %q, want challenge-123", string(body))
	}
}

func TestMetaWebhookSignatureIsVerified(t *testing.T) {
	cfg, repo, service := testOAuthStack(t)
	cfg.MetaWebhookSecret = "meta-webhook-secret"
	_, _ = repo.UpsertConnection(t.Context(), store.Connection{
		ID: "conn-meta", ProviderKey: "meta", OrganizationID: "org-authoritative", Status: "active",
		ProviderContext: map[string]string{"webhook_account_ids": "page-1"},
	})
	publisher := &fakeEventsPublisher{}
	app := NewServer(ServerConfig{Config: cfg, Repo: repo, OAuth: service, Events: publisher, WebhookOrg: &webhookorg.Resolver{Store: repo}})
	body := `{"object":"page","organizationId":"org-attacker","entry":[{"id":"page-1"}]}`
	mac := hmac.New(sha256.New, []byte(cfg.MetaWebhookSecret))
	_, _ = mac.Write([]byte(body))
	signature := "sha256=" + hex.EncodeToString(mac.Sum(nil))

	req := httptest.NewRequest("POST", "/api/v1/webhooks/meta", strings.NewReader(body))
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("X-Hub-Signature-256", signature)
	req.Header.Set("X-Org-ID", "org-attacker")
	resp, err := app.Test(req)
	if err != nil {
		t.Fatalf("app.Test error: %v", err)
	}
	if resp.StatusCode != 200 {
		t.Fatalf("status = %d, want 200", resp.StatusCode)
	}
	if len(publisher.events) != 1 || publisher.events[0].OrganizationID != "org-authoritative" {
		t.Fatalf("published events = %+v, want authoritative asset owner", publisher.events)
	}
}

func TestMetaWebhook_UnresolvedTenantIsRetryable(t *testing.T) {
	cfg, repo, service := testOAuthStack(t)
	cfg.MetaWebhookSecret = "meta-webhook-secret"
	app := NewServer(ServerConfig{Config: cfg, Repo: repo, OAuth: service})
	body := `{"object":"page","entry":[{"id":"unknown-page"}]}`
	mac := hmac.New(sha256.New, []byte(cfg.MetaWebhookSecret))
	_, _ = mac.Write([]byte(body))

	req := httptest.NewRequest("POST", "/api/v1/webhooks/meta", strings.NewReader(body))
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("X-Hub-Signature-256", "sha256="+hex.EncodeToString(mac.Sum(nil)))
	resp, err := app.Test(req)
	if err != nil {
		t.Fatalf("app.Test error: %v", err)
	}
	if resp.StatusCode != http.StatusServiceUnavailable {
		t.Fatalf("status = %d, want 503 so Meta retries unresolved tenant delivery", resp.StatusCode)
	}
}

func TestMetaWebhookPartitionsMultiTenantBatch(t *testing.T) {
	cfg, repo, service := testOAuthStack(t)
	cfg.MetaWebhookSecret = "meta-webhook-secret"
	for _, connection := range []store.Connection{
		{ID: "conn-a", ProviderKey: "meta", OrganizationID: "org-a", Status: "active", ProviderContext: map[string]string{"webhook_account_ids": "page-a"}},
		{ID: "conn-b", ProviderKey: "meta", OrganizationID: "org-b", Status: "active", ProviderContext: map[string]string{"webhook_account_ids": "page-b"}},
	} {
		if _, err := repo.UpsertConnection(t.Context(), connection); err != nil {
			t.Fatalf("UpsertConnection: %v", err)
		}
	}
	publisher := &fakeEventsPublisher{}
	app := NewServer(ServerConfig{Config: cfg, Repo: repo, OAuth: service, Events: publisher, WebhookOrg: &webhookorg.Resolver{Store: repo}})
	body := `{"object":"page","entry":[{"id":"page-a","messaging":[]},{"id":"page-b","messaging":[]}]}`
	mac := hmac.New(sha256.New, []byte(cfg.MetaWebhookSecret))
	_, _ = mac.Write([]byte(body))
	req := httptest.NewRequest("POST", "/api/v1/webhooks/meta", strings.NewReader(body))
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("X-Hub-Signature-256", "sha256="+hex.EncodeToString(mac.Sum(nil)))
	resp, err := app.Test(req)
	if err != nil || resp.StatusCode != http.StatusOK {
		t.Fatalf("response status=%v err=%v", resp.StatusCode, err)
	}
	if len(publisher.events) != 2 || publisher.events[0].OrganizationID != "org-a" || publisher.events[1].OrganizationID != "org-b" {
		t.Fatalf("published partitions = %+v", publisher.events)
	}
	if publisher.events[0].Data["webhookEventId"] == publisher.events[1].Data["webhookEventId"] {
		t.Fatalf("partition event ids collided: %+v", publisher.events)
	}
}

func TestWebhookPublishFailureIsRetryableAndDuplicateRepublishes(t *testing.T) {
	cfg, repo, service := testOAuthStack(t)
	cfg.MetaWebhookSecret = "meta-webhook-secret"
	_, _ = repo.UpsertConnection(t.Context(), store.Connection{
		ID: "conn-meta", ProviderKey: "meta", OrganizationID: "org-1", Status: "active",
		ProviderContext: map[string]string{"webhook_account_ids": "page-1"},
	})
	publisher := &fakeEventsPublisher{errs: []error{errors.New("nats unavailable"), nil}}
	app := NewServer(ServerConfig{Config: cfg, Repo: repo, OAuth: service, Events: publisher, WebhookOrg: &webhookorg.Resolver{Store: repo}})
	body := `{"object":"page","entry":[{"id":"page-1"}]}`
	mac := hmac.New(sha256.New, []byte(cfg.MetaWebhookSecret))
	_, _ = mac.Write([]byte(body))
	signature := "sha256=" + hex.EncodeToString(mac.Sum(nil))

	for attempt, wantStatus := range []int{http.StatusServiceUnavailable, http.StatusOK} {
		req := httptest.NewRequest("POST", "/api/v1/webhooks/meta", strings.NewReader(body))
		req.Header.Set("Content-Type", "application/json")
		req.Header.Set("X-Hub-Signature-256", signature)
		resp, err := app.Test(req)
		if err != nil {
			t.Fatalf("attempt %d app.Test error: %v", attempt+1, err)
		}
		if resp.StatusCode != wantStatus {
			t.Fatalf("attempt %d status = %d, want %d", attempt+1, resp.StatusCode, wantStatus)
		}
	}
	if len(publisher.events) != 2 {
		t.Fatalf("publish attempts = %d, want duplicate retry to republish", len(publisher.events))
	}
}

func TestCreateSyncJobOnlyRequiresMetaWebhookProvisioningForInboxCapabilities(t *testing.T) {
	repo := store.NewMemoryRepository()
	now := time.Now().UTC()
	base := store.Connection{
		ID: "conn-meta-publish", ProviderKey: "meta", OrganizationID: "org-1", UserID: "user-1",
		Status: "active", Capabilities: []string{"social.post.write"}, CreatedAt: now, UpdatedAt: now,
	}
	if _, err := repo.UpsertConnection(t.Context(), base); err != nil {
		t.Fatalf("UpsertConnection: %v", err)
	}
	if _, err := createSyncJob(t.Context(), ServerConfig{Repo: repo}, base, syncJobBody{}); err != nil {
		t.Fatalf("publishing-only Meta sync unexpectedly required webhook provisioning: %v", err)
	}
	base.ID = "conn-meta-inbox"
	base.Capabilities = []string{"social.inbox.read"}
	failedJob, err := createSyncJob(t.Context(), ServerConfig{Repo: repo}, base, syncJobBody{})
	if err == nil {
		t.Fatal("inbox-enabled Meta sync error = nil, want missing provisioning configuration")
	}
	if failedJob.Status != "failed" || failedJob.Metadata["failureCode"] != "meta_inbox_provisioning_failed" {
		t.Fatalf("failed provisioning job was not durable: %+v", failedJob)
	}
	persisted, getErr := repo.GetSyncJob(t.Context(), failedJob.ID)
	if getErr != nil || persisted.Status != "failed" {
		t.Fatalf("persisted failed job=%+v err=%v", persisted, getErr)
	}
}

type fakeMetaWebhookProvisioner struct {
	ids             []string
	subscribed      []string
	subscribeCalled int
}

func (f *fakeMetaWebhookProvisioner) ListWebhookAccountIDs(context.Context, store.Connection) ([]string, error) {
	return append([]string(nil), f.ids...), nil
}

func (f *fakeMetaWebhookProvisioner) SubscribeWebhookAccounts(_ context.Context, _ store.Connection, ids []string) error {
	f.subscribeCalled++
	f.subscribed = append([]string(nil), ids...)
	return nil
}

func TestCreateSyncJobProvisionsMetaAssetsBeforeQueueing(t *testing.T) {
	repo := store.NewMemoryRepository()
	now := time.Now().UTC()
	connection := store.Connection{
		ID: "conn-meta-inbox", ProviderKey: "meta", OrganizationID: "org-1", UserID: "user-1",
		Status: "active", Capabilities: []string{"social.inbox.read"}, CreatedAt: now, UpdatedAt: now,
	}
	if _, err := repo.UpsertConnection(t.Context(), connection); err != nil {
		t.Fatalf("UpsertConnection: %v", err)
	}
	provisioner := &fakeMetaWebhookProvisioner{ids: []string{"page-1", "ig-1"}}
	resolver := &webhookorg.Resolver{Store: repo, Meta: provisioner}
	job, err := createSyncJob(t.Context(), ServerConfig{Repo: repo, WebhookOrg: resolver}, connection, syncJobBody{Reason: "oauth_connected"})
	if err != nil {
		t.Fatalf("createSyncJob: %v", err)
	}
	if job.Status != "handoff_data_plane" || provisioner.subscribeCalled != 1 || !slices.Equal(provisioner.subscribed, provisioner.ids) {
		t.Fatalf("job=%+v provisioner=%+v", job, provisioner)
	}
	saved, err := repo.GetConnection(t.Context(), connection.ID)
	if err != nil || saved.ProviderContext["webhook_account_ids"] != "page-1,ig-1" {
		t.Fatalf("saved connection=%+v err=%v", saved, err)
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

func TestWebhookRejectsWhenSecretUnconfigured(t *testing.T) {
	cfg, repo, service := testOAuthStack(t)
	app := NewServer(ServerConfig{Config: cfg, Repo: repo, OAuth: service})

	// github has a signature scheme but no secret configured; notion has no
	// implemented scheme at all. Both must fail closed with 401.
	for _, provider := range []string{"github", "notion"} {
		req := httptest.NewRequest("POST", "/api/v1/webhooks/"+provider, strings.NewReader(`{"event":"x"}`))
		req.Header.Set("Content-Type", "application/json")
		resp, err := app.Test(req)
		if err != nil {
			t.Fatalf("app.Test error: %v", err)
		}
		if resp.StatusCode != 401 {
			t.Fatalf("provider %s: status = %d, want 401", provider, resp.StatusCode)
		}
	}
}

func TestInternalWebhookEventFetchRoundTrips(t *testing.T) {
	cfg, repo, service := testOAuthStack(t)
	cfg.AllowUnverifiedWebhooks = true
	app := NewServer(ServerConfig{Config: cfg, Repo: repo, OAuth: service})

	postReq := httptest.NewRequest("POST", "/api/v1/webhooks/github", strings.NewReader(`{"organizationId":"org-1","deliveryId":"delivery-42","zen":"hello"}`))
	postReq.Header.Set("Content-Type", "application/json")
	postResp, err := app.Test(postReq)
	if err != nil {
		t.Fatalf("app.Test (post webhook) error: %v", err)
	}
	if postResp.StatusCode != 200 {
		t.Fatalf("post status = %d, want 200", postResp.StatusCode)
	}
	var posted struct {
		Data struct {
			WebhookEventID string `json:"webhookEventId"`
		} `json:"data"`
	}
	if err := json.NewDecoder(postResp.Body).Decode(&posted); err != nil {
		t.Fatalf("decode post response: %v", err)
	}
	if posted.Data.WebhookEventID == "" {
		t.Fatal("post response missing webhookEventId")
	}

	getReq := httptest.NewRequest("GET", "/internal/webhooks/events/"+posted.Data.WebhookEventID+"?organizationId=org-1", nil)
	getReq.Header.Set("X-Internal-API-Key", "dev-key")
	getResp, err := app.Test(getReq)
	if err != nil {
		t.Fatalf("app.Test (get webhook event) error: %v", err)
	}
	if getResp.StatusCode != 200 {
		t.Fatalf("get status = %d, want 200", getResp.StatusCode)
	}
	var fetched struct {
		Data struct {
			WebhookEvent struct {
				ID      string         `json:"id"`
				Payload map[string]any `json:"payload"`
			} `json:"webhookEvent"`
		} `json:"data"`
	}
	if err := json.NewDecoder(getResp.Body).Decode(&fetched); err != nil {
		t.Fatalf("decode get response: %v", err)
	}
	if fetched.Data.WebhookEvent.ID != posted.Data.WebhookEventID {
		t.Fatalf("fetched id = %q, want %q", fetched.Data.WebhookEvent.ID, posted.Data.WebhookEventID)
	}
	if fetched.Data.WebhookEvent.Payload["zen"] != "hello" {
		t.Fatalf("fetched payload = %#v, want the original body echoed back", fetched.Data.WebhookEvent.Payload)
	}
}

func TestInternalWebhookEventFetchRequiresInternalAuth(t *testing.T) {
	cfg, repo, service := testOAuthStack(t)
	app := NewServer(ServerConfig{Config: cfg, Repo: repo, OAuth: service})
	req := httptest.NewRequest("GET", "/internal/webhooks/events/wh_nonexistent", nil)
	resp, err := app.Test(req)
	if err != nil {
		t.Fatalf("app.Test error: %v", err)
	}
	if resp.StatusCode != 401 {
		t.Fatalf("status = %d, want 401 without an internal key", resp.StatusCode)
	}
}

func TestInternalWebhookEventFetchScopesByOrganization(t *testing.T) {
	cfg, repo, service := testOAuthStack(t)
	cfg.AllowUnverifiedWebhooks = true
	app := NewServer(ServerConfig{Config: cfg, Repo: repo, OAuth: service})

	postReq := httptest.NewRequest("POST", "/api/v1/webhooks/github", strings.NewReader(`{"organizationId":"org-1","deliveryId":"delivery-99"}`))
	postReq.Header.Set("Content-Type", "application/json")
	postResp, err := app.Test(postReq)
	if err != nil {
		t.Fatalf("app.Test (post webhook) error: %v", err)
	}
	var posted struct {
		Data struct {
			WebhookEventID string `json:"webhookEventId"`
		} `json:"data"`
	}
	_ = json.NewDecoder(postResp.Body).Decode(&posted)

	getReq := httptest.NewRequest("GET", "/internal/webhooks/events/"+posted.Data.WebhookEventID+"?organizationId=org-2", nil)
	getReq.Header.Set("X-Internal-API-Key", "dev-key")
	getResp, err := app.Test(getReq)
	if err != nil {
		t.Fatalf("app.Test (get webhook event) error: %v", err)
	}
	if getResp.StatusCode != 404 {
		t.Fatalf("status = %d, want 404 for a mismatched organizationId", getResp.StatusCode)
	}
}

func TestWebhookRouteIsRateLimited(t *testing.T) {
	cfg, repo, service := testOAuthStack(t)
	cfg.RateLimitEnabled = true
	cfg.RateLimitMax = 1
	cfg.RateLimitWindow = time.Minute
	// This test exercises the limiter, not signatures: opt into the dev-only
	// unverified-webhook escape hatch (webhooks otherwise fail closed).
	cfg.AllowUnverifiedWebhooks = true
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

func TestInternalTokenBrokerRejectsConnectionOrgMismatch(t *testing.T) {
	cfg, repo, service := testOAuthStack(t)
	cfg.TokenLeaseConsumers = []string{"social-publisher"}
	_, err := repo.UpsertConnection(t.Context(), store.Connection{
		ID:             "conn-social-1",
		ProviderKey:    "x",
		ConnectorType:  "x",
		OrganizationID: "org-owner",
		UserID:         "user-1",
		Status:         "active",
	})
	if err != nil {
		t.Fatalf("UpsertConnection error: %v", err)
	}
	app := NewServer(ServerConfig{Config: cfg, Repo: repo, OAuth: service})

	req := httptest.NewRequest("POST", "/internal/connectors/token", strings.NewReader(`{"organizationId":"org-attacker","connectionId":"conn-social-1","connectorType":"x","consumer":"social-publisher"}`))
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

func TestInternalTokenBrokerPublishesRedactedLeaseEvent(t *testing.T) {
	cfg, repo, service := testOAuthStack(t)
	cfg.TokenLeaseConsumers = []string{"social-publisher"}
	vault, err := secretcrypto.NewVault(cfg.EncryptionKey)
	if err != nil {
		t.Fatalf("NewVault error: %v", err)
	}
	encrypted, err := vault.Encrypt("access-token", []byte("conn-social-lease"))
	if err != nil {
		t.Fatalf("Encrypt error: %v", err)
	}
	_, err = repo.UpsertConnection(t.Context(), store.Connection{
		ID:                   "conn-social-lease",
		ProviderKey:          "x",
		ConnectorType:        "x",
		OrganizationID:       "org-1",
		UserID:               "user-1",
		Status:               "active",
		EncryptedAccessToken: encrypted,
		AccessTokenExpiresAt: time.Now().Add(time.Hour),
	})
	if err != nil {
		t.Fatalf("UpsertConnection error: %v", err)
	}
	publisher := &fakeEventsPublisher{}
	app := NewServer(ServerConfig{Config: cfg, Repo: repo, OAuth: service, Events: publisher})

	req := httptest.NewRequest("POST", "/internal/connectors/token", strings.NewReader(`{"organizationId":"org-1","connectionId":"conn-social-lease","connectorType":"x","consumer":"social-publisher"}`))
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("X-Internal-API-Key", "dev-key")
	resp, err := app.Test(req)
	if err != nil {
		t.Fatalf("app.Test error: %v", err)
	}
	if resp.StatusCode != 200 {
		t.Fatalf("status = %d, want 200", resp.StatusCode)
	}
	if len(publisher.events) != 1 {
		t.Fatalf("events = %#v, want one token lease event", publisher.events)
	}
	event := publisher.events[0]
	if event.Type != "velion.ingestion.integration.token_lease_created" {
		t.Fatalf("event type = %s, want token lease created", event.Type)
	}
	if event.OrganizationID != "org-1" || event.ConnectionID != "conn-social-lease" || event.ProviderKey != "x" {
		t.Fatalf("event scope = %#v, want org/connection/provider", event)
	}
	encoded, err := json.Marshal(event)
	if err != nil {
		t.Fatalf("Marshal event error: %v", err)
	}
	if strings.Contains(string(encoded), "access-token") {
		t.Fatalf("event leaked access token: %s", encoded)
	}
}

func TestRecordTokenLeaseFailsClosedOnMissingConnectionOrAuditIntent(t *testing.T) {
	token := oauth.AccessTokenResult{
		ConnectionID: "conn-lease-rollback", AccessToken: "must-not-be-persisted", ExpiresAt: time.Now().Add(time.Hour),
	}
	missingRepo := store.NewMemoryRepository()
	if err := recordTokenLease(t.Context(), ServerConfig{Repo: missingRepo}, token, "social-publisher"); !errors.Is(err, store.ErrNotFound) {
		t.Fatalf("missing connection error = %v, want ErrNotFound", err)
	}

	repo := store.NewMemoryRepository()
	_, err := repo.UpsertConnection(t.Context(), store.Connection{
		ID: token.ConnectionID, ProviderKey: "x", ConnectorType: "x", OrganizationID: "org-1", UserID: "user-1", Status: "active",
	})
	if err != nil {
		t.Fatalf("UpsertConnection error: %v", err)
	}
	publisher := &fakeEventsPublisher{}
	err = recordTokenLease(t.Context(), ServerConfig{
		Repo:   auditInsertFailureRepository{Repository: repo},
		Events: publisher,
	}, token, "social-publisher")
	if err == nil {
		t.Fatal("audit persistence error = nil")
	}
	if len(publisher.events) != 0 {
		t.Fatalf("lease event published without committed audit intent: %#v", publisher.events)
	}
	if _, found, err := repo.ClaimAuditEvent(t.Context()); err != nil || found {
		t.Fatalf("audit outbox state after rollback: found=%v err=%v", found, err)
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

type auditInsertFailureTransaction struct {
	store.AuditTransaction
}

func (auditInsertFailureTransaction) InsertAuditEvent(context.Context, store.AuditEvent) error {
	return errors.New("audit insert unavailable")
}

type auditInsertFailureRepository struct {
	store.Repository
}

func (r auditInsertFailureRepository) InsertAuditEvent(context.Context, store.AuditEvent) error {
	return errors.New("audit insert unavailable")
}

func (r auditInsertFailureRepository) WithAuditTransaction(ctx context.Context, fn func(store.AuditTransaction) error) error {
	return r.Repository.WithAuditTransaction(ctx, func(tx store.AuditTransaction) error {
		return fn(auditInsertFailureTransaction{AuditTransaction: tx})
	})
}

func TestSCIMTokenCreateRollsBackWhenAuditIntentCannotPersist(t *testing.T) {
	cfg, repo, service := testOAuthStack(t)
	cfg.SCIMBearerToken = ""
	cfg.SCIMBearerTokens = map[string]string{}
	app := NewServer(ServerConfig{
		Config: cfg,
		Repo:   auditInsertFailureRepository{Repository: repo},
		OAuth:  service,
	})

	req := httptest.NewRequest("POST", "/api/v1/scim/tokens", strings.NewReader(`{"organizationId":"org-1","name":"must rollback"}`))
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("X-Internal-API-Key", "dev-key")
	resp, err := app.Test(req)
	if err != nil {
		t.Fatalf("app.Test error: %v", err)
	}
	defer resp.Body.Close()
	if resp.StatusCode != fiber.StatusInternalServerError {
		t.Fatalf("status = %d, want 500", resp.StatusCode)
	}
	tokens, err := repo.ListSCIMTokens(t.Context(), "org-1")
	if err != nil {
		t.Fatalf("ListSCIMTokens: %v", err)
	}
	if len(tokens) != 0 {
		t.Fatalf("SCIM token committed without audit intent: %#v", tokens)
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

func TestMetaActionCapabilityAndApprovalGates(t *testing.T) {
	tests := []struct {
		operation string
		wantCap   string
	}{
		{operation: "pages.post", wantCap: "social.post.write"},
		{operation: "whatsapp.messages.send", wantCap: "social.whatsapp.manage"},
		{operation: "messenger.messages.send", wantCap: "social.messenger.manage"},
		{operation: "instagram.messages.send", wantCap: "social.messenger.manage"},
		{operation: "ads.campaign.create", wantCap: "social.ads.manage"},
		{operation: "catalog.product.upsert", wantCap: "social.catalog.manage"},
		{operation: "threads.publish", wantCap: "social.threads.manage"},
		{operation: "live.create", wantCap: "social.live.manage"},
	}
	for _, tt := range tests {
		t.Run(tt.operation, func(t *testing.T) {
			gotCap, sensitive := requiredCapabilityForOperation("meta", tt.operation)
			if gotCap != tt.wantCap || !sensitive {
				t.Fatalf("requiredCapabilityForOperation = %q/%v, want %q/true", gotCap, sensitive, tt.wantCap)
			}
			if !actionRequiresApproval("meta", tt.operation) {
				t.Fatalf("actionRequiresApproval(%q) = false, want true", tt.operation)
			}
		})
	}
}

func TestGitHubActionCapabilityAndApprovalGates(t *testing.T) {
	tests := []struct {
		operation     string
		wantCap       string
		wantSensitive bool
		wantApproval  bool
	}{
		{operation: "contents.get", wantCap: "repo.contents.read", wantSensitive: true},
		{operation: "commits", wantCap: "commits.read", wantSensitive: true},
		{operation: "pulls.list", wantCap: "pulls.read", wantSensitive: true},
		{operation: "issues.list", wantCap: "issues.read", wantSensitive: true},
		{operation: "issues.create", wantCap: "issues.write", wantSensitive: true, wantApproval: true},
		{operation: "issues.update", wantCap: "issues.write", wantSensitive: true, wantApproval: true},
		{operation: "issues.comment.create", wantCap: "issues.write", wantSensitive: true, wantApproval: true},
	}
	for _, tt := range tests {
		t.Run(tt.operation, func(t *testing.T) {
			gotCap, sensitive := requiredCapabilityForOperation("github", tt.operation)
			if gotCap != tt.wantCap || sensitive != tt.wantSensitive {
				t.Fatalf("requiredCapabilityForOperation = %q/%v, want %q/%v", gotCap, sensitive, tt.wantCap, tt.wantSensitive)
			}
			if got := actionRequiresApproval("github", tt.operation); got != tt.wantApproval {
				t.Fatalf("actionRequiresApproval(%q) = %v, want %v", tt.operation, got, tt.wantApproval)
			}
		})
	}
}

func TestLinkedInActionCapabilityAndApprovalGates(t *testing.T) {
	tests := []struct {
		operation     string
		wantCap       string
		wantSensitive bool
		wantApproval  bool
	}{
		{operation: "profile", wantCap: "social.profile.read"},
		{operation: "identity", wantCap: "social.profile.verify", wantSensitive: true},
		{operation: "verification.report", wantCap: "social.verification.read", wantSensitive: true},
		{operation: "organization.acls", wantCap: "social.organization.read", wantSensitive: true},
		{operation: "posts.create", wantCap: "social.post.write", wantSensitive: true, wantApproval: true},
		{operation: "events.create", wantCap: "social.events.manage", wantSensitive: true, wantApproval: true},
		{operation: "ads.accounts", wantCap: "social.ads.read", wantSensitive: true},
		{operation: "ads.campaigns", wantCap: "social.ads.read", wantSensitive: true},
		{operation: "ads.campaign.create", wantCap: "social.ads.manage", wantSensitive: true, wantApproval: true},
		{operation: "conversions.create", wantCap: "social.conversions.manage", wantSensitive: true, wantApproval: true},
		{operation: "lead.forms", wantCap: "social.leads.read", wantSensitive: true},
	}
	for _, tt := range tests {
		t.Run(tt.operation, func(t *testing.T) {
			gotCap, sensitive := requiredCapabilityForOperation("linkedin", tt.operation)
			if gotCap != tt.wantCap || sensitive != tt.wantSensitive {
				t.Fatalf("requiredCapabilityForOperation = %q/%v, want %q/%v", gotCap, sensitive, tt.wantCap, tt.wantSensitive)
			}
			if got := actionRequiresApproval("linkedin", tt.operation); got != tt.wantApproval {
				t.Fatalf("actionRequiresApproval(%q) = %v, want %v", tt.operation, got, tt.wantApproval)
			}
		})
	}
}

func TestSnapchatActionCapabilityAndApprovalGates(t *testing.T) {
	tests := []struct {
		operation     string
		wantCap       string
		wantSensitive bool
		wantApproval  bool
	}{
		{operation: "organizations", wantCap: "social.profile.read"},
		{operation: "profile.spotlights", wantCap: "social.profile.read"},
		{operation: "spotlight.get", wantCap: "social.profile.read"},
		{operation: "adaccounts", wantCap: "social.ads.manage", wantSensitive: true},
		{operation: "creatives.list", wantCap: "social.ads.manage", wantSensitive: true},
		{operation: "ads.stats", wantCap: "social.analytics.read", wantSensitive: true},
		{operation: "ads.media.create", wantCap: "social.ads.manage", wantSensitive: true, wantApproval: true},
		{operation: "ads.creative.create", wantCap: "social.ads.manage", wantSensitive: true, wantApproval: true},
		{operation: "profile.media.create", wantCap: "social.media.upload", wantSensitive: true, wantApproval: true},
		{operation: "story.post", wantCap: "social.post.write", wantSensitive: true, wantApproval: true},
		{operation: "spotlight.post", wantCap: "social.post.write", wantSensitive: true, wantApproval: true},
		{operation: "saved_story.create", wantCap: "social.post.write", wantSensitive: true, wantApproval: true},
	}
	for _, tt := range tests {
		t.Run(tt.operation, func(t *testing.T) {
			gotCap, sensitive := requiredCapabilityForOperation("snapchat", tt.operation)
			if gotCap != tt.wantCap || sensitive != tt.wantSensitive {
				t.Fatalf("requiredCapabilityForOperation = %q/%v, want %q/%v", gotCap, sensitive, tt.wantCap, tt.wantSensitive)
			}
			if got := actionRequiresApproval("snapchat", tt.operation); got != tt.wantApproval {
				t.Fatalf("actionRequiresApproval(%q) = %v, want %v", tt.operation, got, tt.wantApproval)
			}
			// The provider-prefixed alias must classify identically.
			gotCapAlias, sensitiveAlias := requiredCapabilityForOperation("snapchat", "snapchat."+tt.operation)
			if gotCapAlias != tt.wantCap || sensitiveAlias != tt.wantSensitive {
				t.Fatalf("alias requiredCapabilityForOperation = %q/%v, want %q/%v", gotCapAlias, sensitiveAlias, tt.wantCap, tt.wantSensitive)
			}
		})
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

func TestGDPRDeleteRollsBackAllLocalMutationsWhenAuditIntentFails(t *testing.T) {
	cfg, repo, service := testOAuthStack(t)
	_, err := repo.UpsertConnection(t.Context(), store.Connection{
		ID: "conn-gdpr-rollback", ProviderKey: "github", ConnectorType: "github",
		OrganizationID: "org-1", UserID: "user-1", Status: "active",
		EncryptedAccessToken: "encrypted-token-fixture",
	})
	if err != nil {
		t.Fatalf("UpsertConnection error: %v", err)
	}
	_, err = repo.UpsertConnectionConsent(t.Context(), store.ConnectionConsent{
		ID: "consent-gdpr-rollback", OrganizationID: "org-1", ConnectionID: "conn-gdpr-rollback",
		UserID: "user-1", ProviderKey: "github", Source: "repositories", Purpose: "search", Granted: true,
	})
	if err != nil {
		t.Fatalf("UpsertConnectionConsent error: %v", err)
	}
	app := NewServer(ServerConfig{
		Config: cfg,
		Repo:   auditInsertFailureRepository{Repository: repo},
		OAuth:  service,
	})

	req := httptest.NewRequest("POST", "/internal/gdpr/delete", strings.NewReader(`{"organizationId":"org-1","userId":"user-1","reason":"rollback-test"}`))
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("X-Internal-API-Key", "dev-key")
	resp, err := app.Test(req)
	if err != nil {
		t.Fatalf("app.Test delete error: %v", err)
	}
	defer resp.Body.Close()
	if resp.StatusCode != fiber.StatusInternalServerError {
		t.Fatalf("delete status = %d, want 500", resp.StatusCode)
	}
	connection, err := repo.GetConnection(t.Context(), "conn-gdpr-rollback")
	if err != nil {
		t.Fatalf("GetConnection error: %v", err)
	}
	if connection.Status != "active" || connection.DeletedAt != nil || connection.EncryptedAccessToken == "" {
		t.Fatalf("connection committed without audit: status=%q deleted=%v token_empty=%v", connection.Status, connection.DeletedAt, connection.EncryptedAccessToken == "")
	}
	consents, err := repo.ListConnectionConsents(t.Context(), connection.ID)
	if err != nil {
		t.Fatalf("ListConnectionConsents error: %v", err)
	}
	if len(consents) != 1 || !consents[0].Granted || consents[0].RevokedAt != nil {
		t.Fatalf("consents committed without audit: %#v", consents)
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
		AllowLegacyTenantKey:      true,
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

type fakeEventsPublisher struct {
	events []events.Event
	errs   []error
}

func (f *fakeEventsPublisher) Publish(_ context.Context, event events.Event) error {
	f.events = append(f.events, event)
	if len(f.errs) >= len(f.events) {
		return f.errs[len(f.events)-1]
	}
	return nil
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
