package api

import (
	"crypto/ed25519"
	"crypto/rand"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
	"time"

	"github.com/gofiber/fiber/v2"

	"github.com/triodelab/integration-corev2/internal/actions"
	"github.com/triodelab/integration-corev2/internal/attestation"
	"github.com/triodelab/integration-corev2/internal/auth"
	secretcrypto "github.com/triodelab/integration-corev2/internal/crypto"
	"github.com/triodelab/integration-corev2/internal/store"
)

// G1: the capabilities a connection records must be ones its token holds, and
// a provider that rejects the stored token or its scopes must surface a
// reconnect-shaped error instead of a generic bad gateway.

func patchCapabilities(t *testing.T, app *fiber.App, connectionID, body string) (int, string) {
	t.Helper()
	req := httptest.NewRequest("PATCH", "/api/v1/connections/"+connectionID+"/capabilities", strings.NewReader(body))
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("X-Internal-API-Key", "dev-key")
	req.Header.Set("X-User-ID", "user-1")
	resp, err := app.Test(req)
	if err != nil {
		t.Fatalf("app.Test error: %v", err)
	}
	status := resp.StatusCode
	if status == 200 {
		_ = resp.Body.Close()
		return status, ""
	}
	return status, readAPIErrorCode(t, resp)
}

func seedMicrosoftWithGrants(t *testing.T, repo *store.MemoryRepository, capabilities, scopes []string) {
	t.Helper()
	if _, err := repo.UpsertConnection(t.Context(), store.Connection{
		ID: "conn-microsoft", ProviderKey: "microsoft", ConnectorType: "microsoft-graph",
		OrganizationID: "org-1", WorkspaceID: "org-1", UserID: "user-1", Status: "active",
		Capabilities: capabilities, Scopes: scopes, AccessTokenExpiresAt: time.Now().Add(time.Hour),
	}); err != nil {
		t.Fatalf("UpsertConnection error: %v", err)
	}
}

func TestConnectionCapabilitiesRejectCapabilityTheTokenDoesNotHold(t *testing.T) {
	cfg, repo, service := testOAuthStack(t)
	seedMicrosoftWithGrants(t, repo,
		[]string{"profile.read", "mail.read"},
		[]string{"openid", "profile", "email", "offline_access", "User.Read", "Mail.Read"})
	app := NewServer(ServerConfig{Config: cfg, Repo: repo, OAuth: service})

	// mail.send needs Mail.Send and sharepoint.write needs Files.ReadWrite.All;
	// neither was ever granted, so neither may be recorded.
	for _, requested := range []string{
		`{"capabilities":["profile.read","mail.read","mail.send"]}`,
		`{"capabilities":["profile.read","sharepoint.write"]}`,
	} {
		status, code := patchCapabilities(t, app, "conn-microsoft", requested)
		if status != 400 {
			t.Fatalf("PATCH %s status = %d, want 400", requested, status)
		}
		if code != "capability_not_granted" {
			t.Fatalf("PATCH %s code = %q, want capability_not_granted", requested, code)
		}
	}
	unchanged, err := repo.GetConnection(t.Context(), "conn-microsoft")
	if err != nil {
		t.Fatalf("GetConnection error: %v", err)
	}
	if got := strings.Join(unchanged.Capabilities, ","); got != "profile.read,mail.read" {
		t.Fatalf("capabilities after rejected PATCH = %q, want unchanged", got)
	}
}

func TestConnectionCapabilitiesAcceptGrantedAndScopelessCapabilitiesAndRemovals(t *testing.T) {
	cfg, repo, service := testOAuthStack(t)
	seedMicrosoftWithGrants(t, repo,
		[]string{"profile.read", "mail.read", "mail.send"},
		[]string{"openid", "profile", "email", "offline_access", "User.Read", "Mail.Read", "Mail.Send", "Calendars.Read"})
	app := NewServer(ServerConfig{Config: cfg, Repo: repo, OAuth: service})

	// calendar.read is not recorded yet but Calendars.Read was granted: allowed.
	// Dropping mail.send is always allowed.
	status, code := patchCapabilities(t, app, "conn-microsoft", `{"capabilities":["profile.read","mail.read","calendar.read"]}`)
	if status != 200 {
		t.Fatalf("PATCH granted capability status = %d (%s), want 200", status, code)
	}
	updated, _ := repo.GetConnection(t.Context(), "conn-microsoft")
	if got := strings.Join(updated.Capabilities, ","); got != "profile.read,mail.read,calendar.read" {
		t.Fatalf("capabilities = %q, want profile.read,mail.read,calendar.read", got)
	}
}

func TestConnectionCapabilitiesKeepRecordedCapabilityEvenWhenScopesAreNotRecorded(t *testing.T) {
	cfg, repo, service := testOAuthStack(t)
	// Legacy shape: capabilities recorded without any scope list. Toggling one
	// capability off must not start failing because of that older drift, but a
	// capability that is not recorded yet still needs its scopes.
	seedMicrosoftWithGrants(t, repo, []string{"profile.read", "mail.read", "mail.send"}, nil)
	app := NewServer(ServerConfig{Config: cfg, Repo: repo, OAuth: service})

	if status, code := patchCapabilities(t, app, "conn-microsoft", `{"capabilities":["profile.read","mail.read"]}`); status != 200 {
		t.Fatalf("PATCH removing a capability status = %d (%s), want 200", status, code)
	}
	if status, code := patchCapabilities(t, app, "conn-microsoft", `{"capabilities":["profile.read","mail.read","mail.send"]}`); status != 400 || code != "capability_not_granted" {
		t.Fatalf("PATCH re-adding a capability with no recorded scopes = %d/%s, want 400/capability_not_granted", status, code)
	}
}

func TestConnectionCapabilitiesAllowCapabilitiesThatNeedNoScopes(t *testing.T) {
	cfg, repo, service := testOAuthStack(t)
	if _, err := repo.UpsertConnection(t.Context(), store.Connection{
		ID: "conn-notion", ProviderKey: "notion", ConnectorType: "notion",
		OrganizationID: "org-1", WorkspaceID: "org-1", UserID: "user-1", Status: "active",
		AccessTokenExpiresAt: time.Now().Add(time.Hour),
	}); err != nil {
		t.Fatalf("UpsertConnection error: %v", err)
	}
	app := NewServer(ServerConfig{Config: cfg, Repo: repo, OAuth: service})

	if status, code := patchCapabilities(t, app, "conn-notion", `{"capabilities":["workspace.read","content.read"]}`); status != 200 {
		t.Fatalf("PATCH notion status = %d (%s), want 200: Notion capabilities carry no scopes", status, code)
	}
}

// The shape of the one live Microsoft connection on the dev stack (read from
// integration_connections on 2026-10-01): every capability it records must
// still be accepted, and an ungranted write must still be refused.
func TestConnectionCapabilitiesLiveMicrosoftConnectionShapeStillUpdates(t *testing.T) {
	cfg, repo, service := testOAuthStack(t)
	seedMicrosoftWithGrants(t, repo,
		[]string{"mail.read", "mail.send", "profile.read", "sharepoint.read", "teams.messages.read", "teams.read"},
		[]string{"Channel.ReadBasic.All", "ChannelMessage.Read.All", "Chat.Read", "Files.Read.All", "Mail.Read", "Mail.Send", "Sites.Read.All", "Team.ReadBasic.All", "User.Read", "email", "offline_access", "openid", "profile"})
	app := NewServer(ServerConfig{Config: cfg, Repo: repo, OAuth: service})

	if status, code := patchCapabilities(t, app, "conn-microsoft", `{"capabilities":["mail.read","mail.send","profile.read","sharepoint.read","teams.messages.read","teams.read"]}`); status != 200 {
		t.Fatalf("PATCH of the live capability set status = %d (%s), want 200", status, code)
	}
	if status, code := patchCapabilities(t, app, "conn-microsoft", `{"capabilities":["mail.read","sharepoint.write"]}`); status != 400 || code != "capability_not_granted" {
		t.Fatalf("PATCH adding sharepoint.write = %d/%s, want 400/capability_not_granted", status, code)
	}
}

func TestConnectSessionIsAdditiveByDefaultAndReplacesOnRequest(t *testing.T) {
	cfg, repo, service := testOAuthStack(t)
	seedMicrosoftWithGrants(t, repo,
		[]string{"profile.read", "sharepoint.read", "sharepoint.write"},
		[]string{"openid", "profile", "email", "offline_access", "User.Read", "Files.Read.All", "Sites.Read.All", "Files.ReadWrite.All", "Sites.ReadWrite.All"})
	app := NewServer(ServerConfig{Config: cfg, Repo: repo, OAuth: service})

	create := func(body string) store.ConnectSession {
		t.Helper()
		req := httptest.NewRequest("POST", "/api/v1/providers/microsoft/connect-session", strings.NewReader(body))
		req.Header.Set("Content-Type", "application/json")
		req.Header.Set("X-Internal-API-Key", "dev-key")
		resp, err := app.Test(req)
		if err != nil {
			t.Fatalf("app.Test error: %v", err)
		}
		defer resp.Body.Close()
		if resp.StatusCode != 200 {
			t.Fatalf("status = %d, want 200", resp.StatusCode)
		}
		var decoded struct {
			Data struct {
				SessionToken string `json:"sessionToken"`
			} `json:"data"`
		}
		if err := json.NewDecoder(resp.Body).Decode(&decoded); err != nil {
			t.Fatalf("decode connect session: %v", err)
		}
		session, err := repo.GetConnectSessionByID(t.Context(), decoded.Data.SessionToken)
		if err != nil {
			t.Fatalf("GetConnectSessionByID error: %v", err)
		}
		return session
	}
	has := func(values []string, want string) bool {
		for _, value := range values {
			if value == want {
				return true
			}
		}
		return false
	}

	additive := create(`{"organizationId":"org-1","workspaceId":"org-1","userId":"user-1","userEmail":"a@example.com","bundles":["inbox"]}`)
	if !has(additive.Capabilities, "sharepoint.write") || !has(additive.Scopes, "Files.ReadWrite.All") || !has(additive.Capabilities, "mail.send") {
		t.Fatalf("default connect session = %v / %v, want the granted write kept and the inbox bundle added", additive.Capabilities, additive.Scopes)
	}
	replaced := create(`{"organizationId":"org-1","workspaceId":"org-1","userId":"user-1","userEmail":"a@example.com","bundles":["inbox"],"replaceCapabilities":true}`)
	if has(replaced.Capabilities, "sharepoint.write") || has(replaced.Scopes, "Files.ReadWrite.All") || !has(replaced.Capabilities, "mail.send") {
		t.Fatalf("replace connect session = %v / %v, want exactly the requested bundle", replaced.Capabilities, replaced.Scopes)
	}
}

type providerRejection struct {
	status  int
	headers map[string]string
	body    string
}

func (r providerRejection) server(calls *int) *httptest.Server {
	return httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		*calls++
		for key, value := range r.headers {
			w.Header().Set(key, value)
		}
		w.Header().Set("Content-Type", "application/json")
		w.WriteHeader(r.status)
		_, _ = w.Write([]byte(r.body))
	}))
}

func readReconnectError(t *testing.T, response *http.Response) (code string, details map[string]any) {
	t.Helper()
	defer response.Body.Close()
	var decoded struct {
		Error map[string]any `json:"error"`
	}
	if err := json.NewDecoder(response.Body).Decode(&decoded); err != nil {
		t.Fatalf("decode API error: %v", err)
	}
	code, _ = decoded.Error["code"].(string)
	return code, decoded.Error
}

func TestProviderReadRejectionAsksForReconnectOnlyWhenReconnectingCouldHelp(t *testing.T) {
	cases := []struct {
		name       string
		provider   string
		connector  string
		capability string
		operation  string
		rejection  providerRejection
		wantStatus int
		wantCode   string
	}{
		{
			name: "graph 401", provider: "microsoft", connector: "microsoft-graph", capability: "mail.read", operation: "mail.messages",
			rejection:  providerRejection{status: 401, body: `{"error":{"code":"InvalidAuthenticationToken","message":"token expired"}}`},
			wantStatus: 409, wantCode: "reconnect_required",
		},
		{
			name: "graph 403 missing scope", provider: "microsoft", connector: "microsoft-graph", capability: "mail.read", operation: "mail.messages",
			rejection:  providerRejection{status: 403, body: `{"error":{"code":"ErrorAccessDenied","message":"Access is denied."}}`},
			wantStatus: 409, wantCode: "reconnect_required",
		},
		{
			name: "graph 403 with a non-json body", provider: "microsoft", connector: "microsoft-graph", capability: "mail.read", operation: "mail.messages",
			rejection:  providerRejection{status: 403, body: `<html>Forbidden</html>`},
			wantStatus: 409, wantCode: "reconnect_required",
		},
		{
			name: "google 403 insufficient permissions", provider: "google", connector: "google-workspace", capability: "gmail.read", operation: "gmail.messages",
			rejection:  providerRejection{status: 403, body: `{"error":{"code":403,"status":"PERMISSION_DENIED","errors":[{"reason":"insufficientPermissions"}]}}`},
			wantStatus: 409, wantCode: "reconnect_required",
		},
		{
			name: "google 403 rate limit is not a permission problem", provider: "google", connector: "google-workspace", capability: "gmail.read", operation: "gmail.messages",
			rejection:  providerRejection{status: 403, body: `{"error":{"code":403,"status":"RESOURCE_EXHAUSTED","errors":[{"reason":"rateLimitExceeded"}]}}`},
			wantStatus: 502, wantCode: "action_failed",
		},
		{
			name: "google 403 user rate limit", provider: "google", connector: "google-workspace", capability: "gmail.read", operation: "gmail.messages",
			rejection:  providerRejection{status: 403, body: `{"error":{"code":403,"errors":[{"reason":"userRateLimitExceeded"}]}}`},
			wantStatus: 502, wantCode: "action_failed",
		},
		{
			name: "403 that carries Retry-After is throttling", provider: "microsoft", connector: "microsoft-graph", capability: "mail.read", operation: "mail.messages",
			rejection:  providerRejection{status: 403, headers: map[string]string{"Retry-After": "30"}, body: `{"error":{"code":"activityLimitReached"}}`},
			wantStatus: 502, wantCode: "action_failed",
		},
		{
			name: "graph 500 stays a bad gateway", provider: "microsoft", connector: "microsoft-graph", capability: "mail.read", operation: "mail.messages",
			rejection:  providerRejection{status: 500, body: `{"error":{"code":"generalException"}}`},
			wantStatus: 502, wantCode: "action_failed",
		},
		{
			name: "graph 404 stays a bad gateway", provider: "microsoft", connector: "microsoft-graph", capability: "mail.read", operation: "mail.messages",
			rejection:  providerRejection{status: 404, body: `{"error":{"code":"itemNotFound"}}`},
			wantStatus: 502, wantCode: "action_failed",
		},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			calls := 0
			providerServer := tc.rejection.server(&calls)
			defer providerServer.Close()
			cfg, repo, service := testOAuthStack(t)
			cfg.MicrosoftGraphBaseURL = providerServer.URL
			cfg.GoogleAPIBaseURL = providerServer.URL
			vault, err := secretcrypto.NewVault(cfg.EncryptionKey)
			if err != nil {
				t.Fatalf("NewVault error: %v", err)
			}
			encrypted, err := vault.Encrypt("access-token", []byte("conn-read"))
			if err != nil {
				t.Fatalf("Encrypt error: %v", err)
			}
			if _, err := repo.UpsertConnection(t.Context(), store.Connection{
				ID: "conn-read", ProviderKey: tc.provider, ConnectorType: tc.connector,
				OrganizationID: "org-1", UserID: "user-1", Status: "active", Capabilities: []string{tc.capability},
				EncryptedAccessToken: encrypted, AccessTokenExpiresAt: time.Now().Add(time.Hour),
			}); err != nil {
				t.Fatalf("UpsertConnection error: %v", err)
			}
			app := NewServer(ServerConfig{Config: cfg, Repo: repo, OAuth: service, Actions: actions.NewService(cfg, providerServer.Client())})

			req := httptest.NewRequest("POST", "/api/v1/connections/conn-read/actions", strings.NewReader(`{"operation":"`+tc.operation+`"}`))
			req.Header.Set("Content-Type", "application/json")
			req.Header.Set("X-Internal-API-Key", "dev-key")
			req.Header.Set("X-User-ID", "user-1")
			resp, err := app.Test(req)
			if err != nil {
				t.Fatalf("app.Test error: %v", err)
			}
			if resp.StatusCode != tc.wantStatus {
				t.Fatalf("status = %d, want %d", resp.StatusCode, tc.wantStatus)
			}
			code, details := readReconnectError(t, resp)
			if code != tc.wantCode {
				t.Fatalf("error code = %q, want %q", code, tc.wantCode)
			}
			if calls != 1 {
				t.Fatalf("provider calls = %d, want 1", calls)
			}
			if tc.wantCode == "reconnect_required" {
				if details["reconnectRequired"] != true {
					t.Fatalf("details = %v, want reconnectRequired=true", details)
				}
				if int(details["providerStatus"].(float64)) != tc.rejection.status {
					t.Fatalf("providerStatus = %v, want %d", details["providerStatus"], tc.rejection.status)
				}
			}
		})
	}
}

func TestProviderWriteRejectedForScopeAsksForReconnect(t *testing.T) {
	calls := 0
	providerServer := providerRejection{status: 403, body: `{"error":{"code":"ErrorAccessDenied"}}`}.server(&calls)
	defer providerServer.Close()

	publicKey, privateKey, err := ed25519.GenerateKey(rand.Reader)
	if err != nil {
		t.Fatalf("GenerateKey error: %v", err)
	}
	cfg, repo, oauthService := testOAuthStack(t)
	cfg.AllowLegacyTenantKey = false
	cfg.MicrosoftGraphBaseURL = providerServer.URL
	cfg.ProviderWriteAttestationKeysJSON = apiTrustedKeysJSON(t, publicKey)
	vault, err := secretcrypto.NewVault(cfg.EncryptionKey)
	if err != nil {
		t.Fatalf("NewVault error: %v", err)
	}
	encrypted, err := vault.Encrypt("access-token", []byte("conn-write-403"))
	if err != nil {
		t.Fatalf("Encrypt error: %v", err)
	}
	if _, err := repo.UpsertConnection(t.Context(), store.Connection{
		ID: "conn-write-403", ProviderKey: "microsoft", ConnectorType: "microsoft-graph",
		OrganizationID: "org-1", UserID: "user-1", Status: "active", Capabilities: []string{"mail.send"},
		EncryptedAccessToken: encrypted, AccessTokenExpiresAt: time.Now().Add(time.Hour),
	}); err != nil {
		t.Fatalf("UpsertConnection error: %v", err)
	}
	app := NewServer(ServerConfig{
		Config: cfg, Repo: repo, OAuth: oauthService,
		Auth: fakeVerifier{principal: auth.Principal{
			UserID: "conversation-core", OrganizationID: "org-1", PrincipalType: "service", Scopes: []string{"integration:write"},
		}},
		Actions: actions.NewService(cfg, providerServer.Client()),
	})
	body := map[string]any{"message": map[string]any{"subject": "needs a scope the token lacks"}}
	binding := attestation.Binding{
		PresenterService: "conversation-core", OrganizationID: "org-1", ConnectionID: "conn-write-403",
		ProviderKey: "microsoft", Operation: "mail.send", Params: map[string]any{}, Body: body,
		IdempotencyKey: "conversation:org-1:write-403",
	}
	raw, _ := json.Marshal(map[string]any{
		"operation": "mail.send", "writeAttestation": signAPIWriteAttestation(t, privateKey, apiHumanIntentClaims(t, binding)),
		"idempotencyKey": binding.IdempotencyKey, "params": map[string]any{}, "body": body,
	})
	req := httptest.NewRequest("POST", "/api/v1/connections/conn-write-403/actions", strings.NewReader(string(raw)))
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("Authorization", "Bearer conversation-service-token")
	resp, err := app.Test(req)
	if err != nil {
		t.Fatalf("app.Test error: %v", err)
	}
	if resp.StatusCode != fiber.StatusConflict {
		t.Fatalf("status = %d, want 409", resp.StatusCode)
	}
	if code, _ := readReconnectError(t, resp); code != "reconnect_required" {
		t.Fatalf("error code = %q, want reconnect_required", code)
	}
	if calls != 1 {
		t.Fatalf("provider calls = %d, want 1", calls)
	}
}
