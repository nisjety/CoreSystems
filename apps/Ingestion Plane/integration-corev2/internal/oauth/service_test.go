package oauth

import (
	"context"
	"errors"
	"net/url"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"github.com/triodelab/integration-corev2/internal/config"
	secretcrypto "github.com/triodelab/integration-corev2/internal/crypto"
	"github.com/triodelab/integration-corev2/internal/events"
	"github.com/triodelab/integration-corev2/internal/providers"
	"github.com/triodelab/integration-corev2/internal/store"
)

func TestReconnectProviderContextReplacesDerivedContextButPreservesGuildBinding(t *testing.T) {
	existing := map[string]string{
		"guild_id":            "guild-1",
		"webhook_account_ids": "old-page,old-waba",
		"stale":               "remove-me",
	}
	session := map[string]string{
		"webhook_account_ids": "new-page,new-waba",
		"fresh":               "keep-me",
		"guild_id":            "unverified-replacement",
	}

	got := reconnectProviderContext(existing, session)
	if got["guild_id"] != "guild-1" {
		t.Fatalf("guild_id = %q, want sticky verified binding", got["guild_id"])
	}
	if got["webhook_account_ids"] != "new-page,new-waba" || got["fresh"] != "keep-me" {
		t.Fatalf("session context was not applied: %+v", got)
	}
	if _, retained := got["stale"]; retained {
		t.Fatalf("stale provider-derived context survived reconnect: %+v", got)
	}
}

func TestCreateSessionSupportsOAuthProviderCatalog(t *testing.T) {
	cfg := testOAuthConfig()
	vault, err := secretcrypto.NewVault([]byte("12345678901234567890123456789012"))
	if err != nil {
		t.Fatalf("NewVault error: %v", err)
	}
	repo := store.NewMemoryRepository()
	service := NewService(cfg, repo, vault, NewMicrosoftClient(MicrosoftClientConfig{
		ClientID:         cfg.MicrosoftClientID,
		ClientSecret:     cfg.MicrosoftClientSecret,
		AuthorizationURL: cfg.MicrosoftAuthorizationURL,
		TokenURL:         cfg.MicrosoftTokenURL,
		GraphBaseURL:     cfg.MicrosoftGraphBaseURL,
	}))
	for _, provider := range providers.OAuthCatalog() {
		service.clients[provider.Key] = &callbackClient{authBaseURL: "https://" + provider.Key + ".auth.test/oauth"}
	}

	for _, provider := range providers.OAuthCatalog() {
		t.Run(provider.Key, func(t *testing.T) {
			providerContext := map[string]string(nil)
			if provider.Key == "shopify" {
				providerContext = map[string]string{"shop": "verevon.myshopify.com"}
			}
			result, err := service.CreateSession(context.Background(), CreateSessionInput{
				ProviderKey:     provider.Key,
				OrganizationID:  "org-1",
				WorkspaceID:     "workspace-1",
				UserID:          "user-1",
				Bundles:         []string{"onboarding"},
				ReturnURL:       "https://app.test/onboarding",
				ProviderContext: providerContext,
			})
			if err != nil {
				t.Fatalf("CreateSession error: %v", err)
			}
			if result.AuthMode != "direct-oauth" || result.ConnectURL == "" || result.Provider.Key != provider.Key {
				t.Fatalf("result = %#v, want direct OAuth session for %s", result, provider.Key)
			}
			if provider.Key != "notion" && len(result.Scopes) == 0 {
				t.Fatalf("Scopes = empty for %s", provider.Key)
			}
		})
	}
}

func TestCreateSessionSelectsConversionsBusinessLoginConfigForMeta(t *testing.T) {
	cfg := testOAuthConfig()
	vault, err := secretcrypto.NewVault([]byte("12345678901234567890123456789012"))
	if err != nil {
		t.Fatalf("NewVault error: %v", err)
	}
	repo := store.NewMemoryRepository()
	service := NewService(cfg, repo, vault, NewMicrosoftClient(MicrosoftClientConfig{}))
	client := &callbackClient{}
	service.clients["meta"] = client

	if _, err := service.CreateSession(context.Background(), CreateSessionInput{
		ProviderKey:    "meta",
		OrganizationID: "org-1",
		WorkspaceID:    "workspace-1",
		UserID:         "user-1",
		Bundles:        []string{"conversions"},
		ReturnURL:      "https://app.test/onboarding",
	}); err != nil {
		t.Fatalf("CreateSession error: %v", err)
	}
	if got := client.lastProviderContext["business_login_config"]; got != "conversions" {
		t.Fatalf("business_login_config = %q, want conversions for the conversions bundle", got)
	}

	// A general bundle must NOT pick up the conversions-specific config --
	// it should use the default (or no) Business Login configuration.
	if _, err := service.CreateSession(context.Background(), CreateSessionInput{
		ProviderKey:    "meta",
		OrganizationID: "org-1",
		WorkspaceID:    "workspace-1",
		UserID:         "user-1",
		Bundles:        []string{"onboarding"},
		ReturnURL:      "https://app.test/onboarding",
	}); err != nil {
		t.Fatalf("CreateSession error: %v", err)
	}
	if _, ok := client.lastProviderContext["business_login_config"]; ok {
		t.Fatalf("business_login_config = %q, want unset for the onboarding bundle", client.lastProviderContext["business_login_config"])
	}
}

func TestCreateSessionSelectsStudioAdsBusinessLoginConfigForMeta(t *testing.T) {
	cfg := testOAuthConfig()
	vault, err := secretcrypto.NewVault([]byte("12345678901234567890123456789012"))
	if err != nil {
		t.Fatalf("NewVault error: %v", err)
	}
	repo := store.NewMemoryRepository()
	service := NewService(cfg, repo, vault, NewMicrosoftClient(MicrosoftClientConfig{}))
	client := &callbackClient{}
	service.clients["meta"] = client

	if _, err := service.CreateSession(context.Background(), CreateSessionInput{
		ProviderKey:    "meta",
		OrganizationID: "org-1",
		WorkspaceID:    "workspace-1",
		UserID:         "user-1",
		Bundles:        []string{"ads"},
		ReturnURL:      "https://app.test/onboarding",
	}); err != nil {
		t.Fatalf("CreateSession error: %v", err)
	}
	if got := client.lastProviderContext["business_login_config"]; got != "studio_ads" {
		t.Fatalf("business_login_config = %q, want studio_ads for the ads bundle", got)
	}
}

func TestCreateSessionSelectsSupportMessagingBusinessLoginConfigForMeta(t *testing.T) {
	cfg := testOAuthConfig()
	vault, err := secretcrypto.NewVault([]byte("12345678901234567890123456789012"))
	if err != nil {
		t.Fatalf("NewVault error: %v", err)
	}
	repo := store.NewMemoryRepository()
	service := NewService(cfg, repo, vault, NewMicrosoftClient(MicrosoftClientConfig{}))
	client := &callbackClient{}
	service.clients["meta"] = client

	if _, err := service.CreateSession(context.Background(), CreateSessionInput{
		ProviderKey:    "meta",
		OrganizationID: "org-1",
		WorkspaceID:    "workspace-1",
		UserID:         "user-1",
		Bundles:        []string{"messenger"},
		ReturnURL:      "https://app.test/onboarding",
	}); err != nil {
		t.Fatalf("CreateSession error: %v", err)
	}
	if got := client.lastProviderContext["business_login_config"]; got != "support_messaging" {
		t.Fatalf("business_login_config = %q, want support_messaging for the messenger bundle", got)
	}
}

func TestCreateSessionUsesSnapchatRedirectBaseURL(t *testing.T) {
	cfg := testOAuthConfig()
	cfg.PublicBaseURL = "http://localhost:3026"
	cfg.SnapchatRedirectBaseURL = "https://connect.example.com"
	vault, err := secretcrypto.NewVault([]byte("12345678901234567890123456789012"))
	if err != nil {
		t.Fatalf("NewVault error: %v", err)
	}
	repo := store.NewMemoryRepository()
	service := NewService(cfg, repo, vault, NewMicrosoftClient(MicrosoftClientConfig{}))
	client := &callbackClient{}
	service.clients["snapchat"] = client

	if _, err := service.CreateSession(context.Background(), CreateSessionInput{
		ProviderKey:    "snapchat",
		OrganizationID: "org-1",
		WorkspaceID:    "workspace-1",
		UserID:         "user-1",
		Bundles:        []string{"onboarding"},
		ReturnURL:      "https://app.test/onboarding",
	}); err != nil {
		t.Fatalf("CreateSession error: %v", err)
	}
	if client.lastRedirectURI != "https://connect.example.com/oauth/callback/snapchat" {
		t.Fatalf("Snapchat redirect URI = %q, want provider-specific HTTPS callback", client.lastRedirectURI)
	}
}

func TestCallbackURLUsesNormalizedProviderAndPublicBase(t *testing.T) {
	service := &Service{cfg: config.Config{PublicBaseURL: "https://tunnel.example.test///"}}

	tests := []struct {
		name     string
		provider string
		want     string
	}{
		{name: "slack", provider: "slack", want: "https://tunnel.example.test/oauth/callback/slack"},
		{name: "discord", provider: "discord", want: "https://tunnel.example.test/oauth/callback/discord"},
		{name: "notion", provider: "notion", want: "https://tunnel.example.test/oauth/callback/notion"},
		{name: "linkedin alias", provider: "linkedin-organization", want: "https://tunnel.example.test/oauth/callback/linkedin"},
		{name: "x alias", provider: "twitter", want: "https://tunnel.example.test/oauth/callback/x"},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			if got := service.callbackURL(tt.provider); got != tt.want {
				t.Fatalf("callbackURL(%q) = %q, want %q", tt.provider, got, tt.want)
			}
		})
	}
}

func TestAccessTokenForConnectionReturnsExactConnectionToken(t *testing.T) {
	vault, err := secretcrypto.NewVault([]byte("12345678901234567890123456789012"))
	if err != nil {
		t.Fatalf("NewVault error: %v", err)
	}
	repo := store.NewMemoryRepository()
	encrypted, err := vault.Encrypt("access-token", []byte("conn-1"))
	if err != nil {
		t.Fatalf("Encrypt error: %v", err)
	}
	_, err = repo.UpsertConnection(context.Background(), store.Connection{
		ID:                   "conn-1",
		ProviderKey:          "github",
		ConnectorType:        "github",
		OrganizationID:       "org-1",
		UserID:               "user-1",
		Status:               "active",
		EncryptedAccessToken: encrypted,
		AccessTokenExpiresAt: time.Now().Add(time.Hour),
		Scopes:               []string{"read:user"},
		Capabilities:         []string{"profile.read"},
	})
	if err != nil {
		t.Fatalf("UpsertConnection error: %v", err)
	}
	service := NewService(config.Config{}, repo, vault, NewMicrosoftClient(MicrosoftClientConfig{}))

	token, err := service.AccessTokenForConnection(context.Background(), "conn-1")
	if err != nil {
		t.Fatalf("AccessTokenForConnection error: %v", err)
	}
	if token.AccessToken != "access-token" {
		t.Fatalf("AccessToken = %q, want access-token", token.AccessToken)
	}
	if token.ConnectionID != "conn-1" {
		t.Fatalf("ConnectionID = %q, want conn-1", token.ConnectionID)
	}
}

func TestAccessTokenForConnectionRejectsDisconnectingSagaState(t *testing.T) {
	vault, err := secretcrypto.NewVault([]byte("12345678901234567890123456789012"))
	if err != nil {
		t.Fatalf("NewVault error: %v", err)
	}
	repo := store.NewMemoryRepository()
	encrypted, err := vault.Encrypt("access-token", []byte("conn-disconnecting"))
	if err != nil {
		t.Fatalf("Encrypt error: %v", err)
	}
	_, err = repo.UpsertConnection(t.Context(), store.Connection{
		ID: "conn-disconnecting", ProviderKey: "github", ConnectorType: "github",
		OrganizationID: "org-1", UserID: "user-1", Status: "disconnecting",
		EncryptedAccessToken: encrypted, AccessTokenExpiresAt: time.Now().Add(time.Hour),
	})
	if err != nil {
		t.Fatalf("UpsertConnection error: %v", err)
	}
	service := NewService(config.Config{}, repo, vault, NewMicrosoftClient(MicrosoftClientConfig{}))

	if _, err := service.AccessTokenForConnection(t.Context(), "conn-disconnecting"); !errors.Is(err, store.ErrNotFound) {
		t.Fatalf("AccessTokenForConnection error = %v, want ErrNotFound", err)
	}
}

func TestAccessTokenForConnectionMarksRevokedGrantNeedsRefresh(t *testing.T) {
	vault, err := secretcrypto.NewVault([]byte("12345678901234567890123456789012"))
	if err != nil {
		t.Fatalf("NewVault error: %v", err)
	}
	repo := store.NewMemoryRepository()
	accessToken, err := vault.Encrypt("expired-access-token", []byte("conn-revoked-google"))
	if err != nil {
		t.Fatalf("Encrypt access token error: %v", err)
	}
	refreshToken, err := vault.Encrypt("revoked-refresh-token", []byte("conn-revoked-google"))
	if err != nil {
		t.Fatalf("Encrypt refresh token error: %v", err)
	}
	_, err = repo.UpsertConnection(t.Context(), store.Connection{
		ID:                    "conn-revoked-google",
		ProviderKey:           "google",
		ConnectorType:         "google-workspace",
		OrganizationID:        "org-1",
		UserID:                "user-1",
		Status:                "active",
		EncryptedAccessToken:  accessToken,
		EncryptedRefreshToken: refreshToken,
		AccessTokenExpiresAt:  time.Now().Add(-time.Hour),
	})
	if err != nil {
		t.Fatalf("UpsertConnection error: %v", err)
	}
	service := NewService(config.Config{TokenRefreshSkew: time.Minute}, repo, vault, NewMicrosoftClient(MicrosoftClientConfig{}))
	client := &refreshErrorClient{err: &TokenEndpointError{
		ProviderKey: "google",
		StatusCode:  400,
		Code:        "invalid_grant",
		Description: "Token has been expired or revoked.",
	}}
	service.clients["google"] = client

	_, err = service.AccessTokenForConnection(t.Context(), "conn-revoked-google")
	if !IsAuthorizationRefreshRequired(err) {
		t.Fatalf("AccessTokenForConnection error = %v, want classified refresh authorization error", err)
	}
	saved, err := repo.GetConnection(t.Context(), "conn-revoked-google")
	if err != nil {
		t.Fatalf("GetConnection error: %v", err)
	}
	if saved.Status != "needs_refresh" {
		t.Fatalf("connection status = %q, want needs_refresh", saved.Status)
	}
	if _, err := service.AccessTokenForConnection(t.Context(), "conn-revoked-google"); !errors.Is(err, store.ErrNotFound) {
		t.Fatalf("second AccessTokenForConnection error = %v, want ErrNotFound", err)
	}
	if client.calls.Load() != 1 {
		t.Fatalf("refresh calls = %d, want one", client.calls.Load())
	}
}

func TestCompleteCallbackReconnectReusesConnectionAndUpgradesScopes(t *testing.T) {
	cfg := testOAuthConfig()
	vault, err := secretcrypto.NewVault([]byte("12345678901234567890123456789012"))
	if err != nil {
		t.Fatalf("NewVault error: %v", err)
	}
	repo := store.NewMemoryRepository()
	oldAccess, err := vault.Encrypt("old-access-token", []byte("conn-existing"))
	if err != nil {
		t.Fatalf("Encrypt access token error: %v", err)
	}
	oldRefresh, err := vault.Encrypt("old-refresh-token", []byte("conn-existing"))
	if err != nil {
		t.Fatalf("Encrypt refresh token error: %v", err)
	}
	createdAt := time.Now().Add(-time.Hour).UTC()
	_, err = repo.UpsertConnection(context.Background(), store.Connection{
		ID:                    "conn-existing",
		ProviderKey:           "github",
		ConnectorType:         "github",
		OrganizationID:        "org-1",
		WorkspaceID:           "workspace-1",
		UserID:                "user-1",
		ProviderAccountID:     "gh-user",
		Status:                "active",
		EncryptedAccessToken:  oldAccess,
		EncryptedRefreshToken: oldRefresh,
		AccessTokenExpiresAt:  time.Now().Add(time.Hour),
		Capabilities:          []string{"profile.read"},
		Scopes:                []string{"read:user", "user:email"},
		CreatedAt:             createdAt,
	})
	if err != nil {
		t.Fatalf("UpsertConnection error: %v", err)
	}

	client := &callbackClient{
		authBaseURL: "https://github.auth.test/oauth",
		token: TokenResult{
			AccessToken: "new-access-token",
			ExpiresAt:   time.Now().Add(2 * time.Hour),
		},
		profile: ProviderProfile{ID: "gh-user", DisplayName: "Ima"},
	}
	service := NewService(cfg, repo, vault, NewMicrosoftClient(MicrosoftClientConfig{}))
	service.clients["github"] = client
	result, err := service.CreateSession(context.Background(), CreateSessionInput{
		ProviderKey:    "github",
		OrganizationID: "org-1",
		WorkspaceID:    "workspace-1",
		UserID:         "user-1",
		Capabilities:   []string{"profile.read", "org.read", "repo.public.read"},
	})
	if err != nil {
		t.Fatalf("CreateSession error: %v", err)
	}
	state := stateFromAuthURL(t, result.AuthorizationURL)
	callback, err := service.CompleteCallback(context.Background(), "github", state, "code", "", "")
	if err != nil {
		t.Fatalf("CompleteCallback error: %v", err)
	}
	if callback.ConnectionID != "conn-existing" {
		t.Fatalf("ConnectionID = %q, want existing connection reuse", callback.ConnectionID)
	}
	connection, err := repo.GetConnection(context.Background(), "conn-existing")
	if err != nil {
		t.Fatalf("GetConnection error: %v", err)
	}
	if !connection.CreatedAt.Equal(createdAt) {
		t.Fatalf("CreatedAt = %s, want preserved %s", connection.CreatedAt, createdAt)
	}
	if connection.EncryptedRefreshToken != oldRefresh {
		t.Fatal("reconnect without new refresh token did not preserve existing refresh token")
	}
	if !hasString(connection.Capabilities, "repo.public.read") || !hasString(connection.Scopes, "public_repo") {
		t.Fatalf("connection capabilities/scopes = %#v/%#v, want upgraded GitHub access", connection.Capabilities, connection.Scopes)
	}
	token, err := service.AccessTokenForConnection(context.Background(), "conn-existing")
	if err != nil {
		t.Fatalf("AccessTokenForConnection error: %v", err)
	}
	if token.AccessToken != "new-access-token" {
		t.Fatalf("AccessToken = %q, want new access token", token.AccessToken)
	}
}

func TestPersistConnectionKeepsSeparateProviderAccounts(t *testing.T) {
	cfg := testOAuthConfig()
	vault, err := secretcrypto.NewVault([]byte("12345678901234567890123456789012"))
	if err != nil {
		t.Fatalf("NewVault error: %v", err)
	}
	repo := store.NewMemoryRepository()
	service := NewService(cfg, repo, vault, NewMicrosoftClient(MicrosoftClientConfig{}))
	client := &callbackClient{profile: ProviderProfile{ID: "account-b", DisplayName: "Second account"}}
	service.clients["google"] = client

	if _, err := repo.UpsertConnection(t.Context(), store.Connection{
		ID:                "conn-account-a",
		ProviderKey:       "google",
		ConnectorType:     "google",
		OrganizationID:    "org-1",
		WorkspaceID:       "workspace-1",
		UserID:            "user-1",
		ProviderAccountID: "account-a",
		Status:            "active",
	}); err != nil {
		t.Fatalf("seed account A: %v", err)
	}

	session := store.ConnectSession{
		ID: "session-account-b", ProviderKey: "google", ConnectorType: "google",
		OrganizationID: "org-1", WorkspaceID: "workspace-1", UserID: "user-1",
		Capabilities: []string{"mail.read"}, Scopes: []string{"openid"},
	}
	savedB, err := service.persistConnection(t.Context(), session, TokenResult{AccessToken: "token-b", ExpiresAt: time.Now().Add(time.Hour)})
	if err != nil {
		t.Fatalf("persist account B: %v", err)
	}
	if savedB.ID == "conn-account-a" {
		t.Fatalf("different provider account reused account A connection: %q", savedB.ID)
	}

	client.profile = ProviderProfile{ID: "account-a", DisplayName: "First account"}
	session.ID = "session-account-a-reconnect"
	savedA, err := service.persistConnection(t.Context(), session, TokenResult{AccessToken: "token-a-new", ExpiresAt: time.Now().Add(time.Hour)})
	if err != nil {
		t.Fatalf("persist account A reconnect: %v", err)
	}
	if savedA.ID != "conn-account-a" {
		t.Fatalf("same provider account created a new connection: %q", savedA.ID)
	}

	connections, err := repo.ListConnections(t.Context(), store.ConnectionFilter{OrganizationID: "org-1", ProviderKey: "google"})
	if err != nil {
		t.Fatalf("ListConnections: %v", err)
	}
	if len(connections) != 2 {
		t.Fatalf("connection count = %d, want 2: %#v", len(connections), connections)
	}
}

func TestProviderAccountIdentityUsesNotionWorkspaceID(t *testing.T) {
	profile := ProviderProfile{ID: "notion-bot-id"}
	if got := providerAccountIdentity("notion", profile, TokenResult{Raw: map[string]any{"workspace_id": "workspace-a"}}); got != "workspace-a" {
		t.Fatalf("Notion account identity = %q, want workspace-a", got)
	}
	if got := providerAccountIdentity("notion", profile, TokenResult{}); got != "notion-bot-id" {
		t.Fatalf("Notion fallback account identity = %q, want notion-bot-id", got)
	}
}

func TestPersistMetaConnectionUsesGrantedScopesAndRemovesUngrantableCapabilities(t *testing.T) {
	cfg := testOAuthConfig()
	vault, err := secretcrypto.NewVault([]byte("12345678901234567890123456789012"))
	if err != nil {
		t.Fatalf("NewVault error: %v", err)
	}
	repo := store.NewMemoryRepository()
	service := NewService(cfg, repo, vault, NewMicrosoftClient(MicrosoftClientConfig{}))
	meta, ok := providers.FindOAuth("meta")
	if !ok {
		t.Fatal("catalog missing Meta")
	}
	requestedCapabilities := providers.ResolveCapabilities(meta, nil, []string{"inbox"})
	session := store.ConnectSession{
		ID: "session-meta-grants", ProviderKey: "meta", ConnectorType: "meta",
		OrganizationID: "org-1", UserID: "user-1",
		Capabilities: requestedCapabilities,
		Scopes:       providers.ResolveScopes(meta, requestedCapabilities),
	}

	connection, err := service.persistConnection(t.Context(), session, TokenResult{
		AccessToken:    "meta-access-token",
		Scope:          []string{"public_profile"},
		ScopesVerified: true,
		ExpiresAt:      time.Now().Add(time.Hour),
	})
	if err != nil {
		t.Fatalf("persistConnection error: %v", err)
	}
	if len(connection.Capabilities) != 0 {
		t.Fatalf("capabilities = %v, want none without the required Meta grants", connection.Capabilities)
	}
	if len(connection.Scopes) != 1 || connection.Scopes[0] != "public_profile" {
		t.Fatalf("scopes = %v, want only provider-verified public_profile", connection.Scopes)
	}
	if connection.Status != "needs_refresh" {
		t.Fatalf("status = %q, want needs_refresh for incomplete Meta authorization", connection.Status)
	}
}

func TestMetaCallbackDefersAuthoritativeCompletionUntilProvisioningFinalizes(t *testing.T) {
	cfg := testOAuthConfig()
	vault, err := secretcrypto.NewVault([]byte("12345678901234567890123456789012"))
	if err != nil {
		t.Fatalf("NewVault: %v", err)
	}
	repo := store.NewMemoryRepository()
	service := NewService(cfg, repo, vault, NewMicrosoftClient(MicrosoftClientConfig{}))
	publisher := &capturePublisher{}
	service.SetEventPublisher(publisher)
	provider, _ := providers.FindOAuth("meta")
	requested := providers.ResolveCapabilities(provider, nil, []string{"inbox"})
	granted := providers.ResolveScopes(provider, requested)
	service.clients["meta"] = &callbackClient{token: TokenResult{
		AccessToken: "meta-token", Scope: granted, ScopesVerified: true, ExpiresAt: time.Now().Add(time.Hour),
	}}
	created, err := service.CreateSession(t.Context(), CreateSessionInput{
		ProviderKey: "meta", OrganizationID: "org-1", UserID: "user-1", Capabilities: requested,
	})
	if err != nil {
		t.Fatalf("CreateSession: %v", err)
	}
	result, err := service.CompleteCallback(t.Context(), "meta", stateFromAuthURL(t, created.AuthorizationURL), "code", "", "")
	if err != nil || !result.Success {
		t.Fatalf("CompleteCallback result=%+v err=%v", result, err)
	}
	session, _ := repo.GetConnectSessionByID(t.Context(), result.SessionID)
	if session.ConsumedAt != nil || len(publisher.events) != 0 {
		t.Fatalf("Meta completion was emitted before provisioning: session=%+v events=%+v", session, publisher.events)
	}
	connection, _ := repo.GetConnection(t.Context(), result.ConnectionID)
	if err := service.FinalizeConnectedCallback(t.Context(), result, connection); err != nil {
		t.Fatalf("FinalizeConnectedCallback: %v", err)
	}
	session, _ = repo.GetConnectSessionByID(t.Context(), result.SessionID)
	if session.ConsumedAt == nil || session.ErrorCode != "" || len(publisher.events) != 1 || publisher.events[0].Type != "integration.connected" {
		t.Fatalf("finalization was not authoritative: session=%+v events=%+v", session, publisher.events)
	}
}

func TestPersistLegacyInstagramConnectionTreatsVerifiedEmptyGrantAsAuthoritative(t *testing.T) {
	cfg := testOAuthConfig()
	vault, err := secretcrypto.NewVault([]byte("12345678901234567890123456789012"))
	if err != nil {
		t.Fatalf("NewVault error: %v", err)
	}
	repo := store.NewMemoryRepository()
	service := NewService(cfg, repo, vault, NewMicrosoftClient(MicrosoftClientConfig{}))
	provider, ok := providers.FindOAuth("instagram")
	if !ok {
		t.Fatal("catalog missing Instagram")
	}
	requested := providers.ResolveCapabilities(provider, nil, []string{"inbox"})
	session := store.ConnectSession{
		ID: "session-instagram-empty", ProviderKey: "instagram", ConnectorType: "instagram",
		OrganizationID: "org-1", UserID: "user-1", Capabilities: requested,
		Scopes: providers.ResolveScopes(provider, requested),
	}

	connection, err := service.persistConnection(t.Context(), session, TokenResult{
		AccessToken: "meta-access-token", ScopesVerified: true, ExpiresAt: time.Now().Add(time.Hour),
	})
	if err != nil {
		t.Fatalf("persistConnection error: %v", err)
	}
	if len(connection.Scopes) != 0 || len(connection.Capabilities) != 0 || connection.Status != "needs_refresh" {
		t.Fatalf("connection = %+v, want authoritative empty grants and needs_refresh", connection)
	}
}

func TestPersistConnectionRollsBackWhenCreationAuditIntentFails(t *testing.T) {
	vault, err := secretcrypto.NewVault([]byte("12345678901234567890123456789012"))
	if err != nil {
		t.Fatalf("NewVault error: %v", err)
	}
	repo := store.NewMemoryRepository()
	service := NewService(config.Config{}, failingAuditRepository{Repository: repo}, vault, NewMicrosoftClient(MicrosoftClientConfig{}))
	session := store.ConnectSession{
		ID: "session-audit-rollback", ProviderKey: "test-provider", ConnectorType: "test-provider",
		OrganizationID: "org-1", UserID: "user-1", UserEmail: "user@example.test",
		Capabilities: []string{"profile.read"}, Scopes: []string{"profile"},
	}
	_, err = service.persistConnection(t.Context(), session, TokenResult{
		AccessToken: "provider-access-token", RefreshToken: "provider-refresh-token", ExpiresAt: time.Now().Add(time.Hour),
	})
	if err == nil {
		t.Fatal("persistConnection error = nil, want audit persistence failure")
	}
	connections, err := repo.ListConnections(t.Context(), store.ConnectionFilter{OrganizationID: "org-1"})
	if err != nil {
		t.Fatalf("ListConnections error: %v", err)
	}
	if len(connections) != 0 {
		t.Fatalf("connection committed without creation audit: %#v", connections)
	}
}

func TestConcurrentAccessTokenRefreshUsesSingleProviderRefresh(t *testing.T) {
	vault, err := secretcrypto.NewVault([]byte("12345678901234567890123456789012"))
	if err != nil {
		t.Fatalf("NewVault error: %v", err)
	}
	repo := store.NewMemoryRepository()
	accessToken, err := vault.Encrypt("old-access-token", []byte("conn-1"))
	if err != nil {
		t.Fatalf("Encrypt access token error: %v", err)
	}
	refreshToken, err := vault.Encrypt("refresh-token", []byte("conn-1"))
	if err != nil {
		t.Fatalf("Encrypt refresh token error: %v", err)
	}
	_, err = repo.UpsertConnection(context.Background(), store.Connection{
		ID:                    "conn-1",
		ProviderKey:           "github",
		ConnectorType:         "github",
		OrganizationID:        "org-1",
		UserID:                "user-1",
		Status:                "active",
		EncryptedAccessToken:  accessToken,
		EncryptedRefreshToken: refreshToken,
		AccessTokenExpiresAt:  time.Now().Add(-time.Minute),
		Scopes:                []string{"read:user"},
		Capabilities:          []string{"profile.read"},
	})
	if err != nil {
		t.Fatalf("UpsertConnection error: %v", err)
	}
	client := &countingRefreshClient{
		wait:      25 * time.Millisecond,
		expiresAt: time.Now().Add(time.Hour),
	}
	service := NewService(config.Config{TokenRefreshSkew: 2 * time.Minute}, repo, vault, NewMicrosoftClient(MicrosoftClientConfig{}))
	service.clients["github"] = client

	var wg sync.WaitGroup
	errs := make(chan error, 8)
	for i := 0; i < 8; i++ {
		wg.Add(1)
		go func() {
			defer wg.Done()
			token, err := service.AccessTokenForConnection(context.Background(), "conn-1")
			if err != nil {
				errs <- err
				return
			}
			if token.AccessToken != "new-access-token" {
				errs <- &unexpectedTokenError{got: token.AccessToken}
			}
		}()
	}
	wg.Wait()
	close(errs)
	for err := range errs {
		if err != nil {
			t.Fatalf("concurrent refresh error: %v", err)
		}
	}
	if got := client.calls.Load(); got != 1 {
		t.Fatalf("refresh calls = %d, want 1", got)
	}
}

func TestDistributedRefreshLockRereadsFreshConnectionBeforeProviderRefresh(t *testing.T) {
	vault, err := secretcrypto.NewVault([]byte("12345678901234567890123456789012"))
	if err != nil {
		t.Fatalf("NewVault error: %v", err)
	}
	repo := &refreshLockRepo{MemoryRepository: store.NewMemoryRepository()}
	oldAccess, err := vault.Encrypt("old-access-token", []byte("conn-1"))
	if err != nil {
		t.Fatalf("Encrypt old access token error: %v", err)
	}
	refreshToken, err := vault.Encrypt("refresh-token", []byte("conn-1"))
	if err != nil {
		t.Fatalf("Encrypt refresh token error: %v", err)
	}
	staleConnection := store.Connection{
		ID:                    "conn-1",
		ProviderKey:           "github",
		ConnectorType:         "github",
		OrganizationID:        "org-1",
		UserID:                "user-1",
		Status:                "active",
		EncryptedAccessToken:  oldAccess,
		EncryptedRefreshToken: refreshToken,
		AccessTokenExpiresAt:  time.Now().Add(-time.Minute),
		Scopes:                []string{"read:user"},
		Capabilities:          []string{"profile.read"},
	}
	freshAccess, err := vault.Encrypt("fresh-access-token", []byte("conn-1"))
	if err != nil {
		t.Fatalf("Encrypt fresh access token error: %v", err)
	}
	freshConnection := staleConnection
	freshConnection.EncryptedAccessToken = freshAccess
	freshConnection.AccessTokenExpiresAt = time.Now().Add(time.Hour)
	if _, err := repo.UpsertConnection(context.Background(), freshConnection); err != nil {
		t.Fatalf("UpsertConnection error: %v", err)
	}
	client := &countingRefreshClient{expiresAt: time.Now().Add(time.Hour)}
	service := NewService(config.Config{TokenRefreshSkew: 2 * time.Minute}, repo, vault, NewMicrosoftClient(MicrosoftClientConfig{}))
	service.clients["github"] = client

	token, err := service.accessTokenForConnection(context.Background(), staleConnection)
	if err != nil {
		t.Fatalf("accessTokenForConnection error: %v", err)
	}
	if token.AccessToken != "fresh-access-token" {
		t.Fatalf("AccessToken = %q, want fresh-access-token", token.AccessToken)
	}
	if got := client.calls.Load(); got != 0 {
		t.Fatalf("refresh calls = %d, want 0", got)
	}
	if got := repo.locks.Load(); got != 1 {
		t.Fatalf("refresh locks = %d, want 1", got)
	}
}

func TestDisconnectConnectionPublishesLifecycleEvent(t *testing.T) {
	vault, err := secretcrypto.NewVault([]byte("12345678901234567890123456789012"))
	if err != nil {
		t.Fatalf("NewVault error: %v", err)
	}
	repo := store.NewMemoryRepository()
	encrypted, err := vault.Encrypt("access-token", []byte("conn-1"))
	if err != nil {
		t.Fatalf("Encrypt error: %v", err)
	}
	_, err = repo.UpsertConnection(context.Background(), store.Connection{
		ID:                   "conn-1",
		ProviderKey:          "microsoft",
		ConnectorType:        "microsoft-graph",
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
	publisher := &capturePublisher{}
	service := NewService(config.Config{}, repo, vault, NewMicrosoftClient(MicrosoftClientConfig{}))
	service.SetEventPublisher(publisher)

	if _, err := service.DisconnectConnection(context.Background(), "conn-1", "test"); err != nil {
		t.Fatalf("DisconnectConnection error: %v", err)
	}
	if len(publisher.events) != 1 {
		t.Fatalf("published events = %d, want 1", len(publisher.events))
	}
	if publisher.events[0].Type != "integration.disconnected" {
		t.Fatalf("event type = %q, want integration.disconnected", publisher.events[0].Type)
	}
}

type failingAuditTransaction struct {
	store.AuditTransaction
}

func (failingAuditTransaction) InsertAuditEvent(context.Context, store.AuditEvent) error {
	return errors.New("audit insert unavailable")
}

type failingAuditRepository struct {
	store.Repository
}

func (r failingAuditRepository) WithAuditTransaction(ctx context.Context, fn func(store.AuditTransaction) error) error {
	return r.Repository.WithAuditTransaction(ctx, func(tx store.AuditTransaction) error {
		return fn(failingAuditTransaction{AuditTransaction: tx})
	})
}

type failCompletedDeleteOnceRepository struct {
	store.Repository
	failed atomic.Bool
}

func (r *failCompletedDeleteOnceRepository) WithAuditTransaction(ctx context.Context, fn func(store.AuditTransaction) error) error {
	return r.Repository.WithAuditTransaction(ctx, func(tx store.AuditTransaction) error {
		return fn(failCompletedDeleteOnceTransaction{AuditTransaction: tx, parent: r})
	})
}

type failCompletedDeleteOnceTransaction struct {
	store.AuditTransaction
	parent *failCompletedDeleteOnceRepository
}

func (tx failCompletedDeleteOnceTransaction) InsertAuditEvent(ctx context.Context, event store.AuditEvent) error {
	if event.EventType == "connection.deleted" && tx.parent.failed.CompareAndSwap(false, true) {
		return errors.New("simulated crash before delete finalization")
	}
	return tx.AuditTransaction.InsertAuditEvent(ctx, event)
}

func TestDisconnectConnectionStopsBeforeProviderWhenDeleteIntentCannotPersist(t *testing.T) {
	vault, err := secretcrypto.NewVault([]byte("12345678901234567890123456789012"))
	if err != nil {
		t.Fatalf("NewVault error: %v", err)
	}
	repo := store.NewMemoryRepository()
	encrypted, err := vault.Encrypt("access-token", []byte("conn-atomic-delete"))
	if err != nil {
		t.Fatalf("Encrypt error: %v", err)
	}
	_, err = repo.UpsertConnection(t.Context(), store.Connection{
		ID:                   "conn-atomic-delete",
		ProviderKey:          "github",
		ConnectorType:        "github",
		OrganizationID:       "org-1",
		UserID:               "user-1",
		Status:               "active",
		EncryptedAccessToken: encrypted,
	})
	if err != nil {
		t.Fatalf("UpsertConnection error: %v", err)
	}
	client := &revocationCountingClient{}
	service := NewService(config.Config{}, failingAuditRepository{Repository: repo}, vault, NewMicrosoftClient(MicrosoftClientConfig{}))
	service.clients["github"] = client

	if _, err := service.DisconnectConnection(t.Context(), "conn-atomic-delete", "test"); err == nil {
		t.Fatal("DisconnectConnection error = nil, want audit persistence failure")
	}
	if got := client.revokeCalls.Load(); got != 0 {
		t.Fatalf("provider revoke calls = %d, want 0 before durable delete intent", got)
	}
	connection, err := repo.GetConnection(t.Context(), "conn-atomic-delete")
	if err != nil {
		t.Fatalf("GetConnection error: %v", err)
	}
	if connection.Status != "active" || connection.DeletedAt != nil {
		t.Fatalf("connection state = %q/%v, want active rollback", connection.Status, connection.DeletedAt)
	}
}

func TestDisconnectConnectionResumesAfterProviderSuccessBeforeDeleteFinalization(t *testing.T) {
	vault, err := secretcrypto.NewVault([]byte("12345678901234567890123456789012"))
	if err != nil {
		t.Fatalf("NewVault error: %v", err)
	}
	repo := store.NewMemoryRepository()
	encrypted, err := vault.Encrypt("access-token", []byte("conn-resumable-delete"))
	if err != nil {
		t.Fatalf("Encrypt error: %v", err)
	}
	_, err = repo.UpsertConnection(t.Context(), store.Connection{
		ID:                   "conn-resumable-delete",
		ProviderKey:          "github",
		ConnectorType:        "github",
		OrganizationID:       "org-1",
		UserID:               "user-1",
		Status:               "active",
		EncryptedAccessToken: encrypted,
	})
	if err != nil {
		t.Fatalf("UpsertConnection error: %v", err)
	}
	client := &revocationCountingClient{}
	guardedRepo := &failCompletedDeleteOnceRepository{Repository: repo}
	service := NewService(config.Config{}, guardedRepo, vault, NewMicrosoftClient(MicrosoftClientConfig{}))
	service.clients["github"] = client

	if _, err := service.DisconnectConnection(t.Context(), "conn-resumable-delete", "test"); err == nil {
		t.Fatal("first DisconnectConnection error = nil, want simulated finalization failure")
	}
	interrupted, err := repo.GetConnection(t.Context(), "conn-resumable-delete")
	if err != nil {
		t.Fatalf("GetConnection interrupted state: %v", err)
	}
	if interrupted.Status != "disconnecting" || interrupted.DeletedAt != nil || interrupted.EncryptedAccessToken == "" {
		t.Fatalf("interrupted state = %q/%v token_empty=%v, want resumable disconnecting state", interrupted.Status, interrupted.DeletedAt, interrupted.EncryptedAccessToken == "")
	}

	deleted, err := service.DisconnectConnection(t.Context(), "conn-resumable-delete", "test")
	if err != nil {
		t.Fatalf("retry DisconnectConnection error: %v", err)
	}
	if deleted.Status != "deleted" || deleted.DeletedAt == nil || deleted.EncryptedAccessToken != "" {
		t.Fatalf("retry state = %q/%v token_empty=%v, want deleted", deleted.Status, deleted.DeletedAt, deleted.EncryptedAccessToken == "")
	}
	if got := client.revokeCalls.Load(); got != 2 {
		t.Fatalf("provider revoke calls = %d, want 2 idempotent attempts across resume", got)
	}
}

func TestDisconnectConnectionRetainsCredentialsAndRetriesRevocationFailure(t *testing.T) {
	vault, err := secretcrypto.NewVault([]byte("12345678901234567890123456789012"))
	if err != nil {
		t.Fatalf("NewVault error: %v", err)
	}
	repo := store.NewMemoryRepository()
	encrypted, err := vault.Encrypt("access-token", []byte("conn-revocation-retry"))
	if err != nil {
		t.Fatalf("Encrypt error: %v", err)
	}
	_, err = repo.UpsertConnection(t.Context(), store.Connection{
		ID: "conn-revocation-retry", ProviderKey: "github", ConnectorType: "github",
		OrganizationID: "org-1", UserID: "user-1", Status: "active",
		EncryptedAccessToken: encrypted, AccessTokenExpiresAt: time.Now().Add(time.Hour),
	})
	if err != nil {
		t.Fatalf("UpsertConnection error: %v", err)
	}
	client := &revocationSequenceClient{errors: []error{errors.New("provider unavailable"), nil}}
	service := NewService(config.Config{}, repo, vault, NewMicrosoftClient(MicrosoftClientConfig{}))
	service.clients["github"] = client

	if _, err := service.DisconnectConnection(t.Context(), "conn-revocation-retry", "test"); err == nil {
		t.Fatal("first DisconnectConnection error = nil, want retryable revocation failure")
	}
	failed, err := repo.GetConnection(t.Context(), "conn-revocation-retry")
	if err != nil {
		t.Fatalf("GetConnection failed state: %v", err)
	}
	if failed.Status != "revocation_failed" || failed.DeletedAt != nil || failed.EncryptedAccessToken == "" {
		t.Fatalf("failed state = %q/%v token_empty=%v, want non-leasable retained credentials", failed.Status, failed.DeletedAt, failed.EncryptedAccessToken == "")
	}
	if _, err := service.AccessTokenForConnection(t.Context(), failed.ID); !errors.Is(err, store.ErrNotFound) {
		t.Fatalf("AccessTokenForConnection error = %v, want non-leasable ErrNotFound", err)
	}

	deleted, err := service.DisconnectConnection(t.Context(), failed.ID, "test")
	if err != nil {
		t.Fatalf("retry DisconnectConnection error: %v", err)
	}
	if deleted.Status != "deleted" || deleted.DeletedAt == nil || deleted.EncryptedAccessToken != "" {
		t.Fatalf("retry state = %q/%v token_empty=%v, want deleted after verified revocation", deleted.Status, deleted.DeletedAt, deleted.EncryptedAccessToken == "")
	}
	if client.calls.Load() != 2 {
		t.Fatalf("revoke calls = %d, want 2", client.calls.Load())
	}
}

func TestDisconnectConnectionAppliesExplicitUnsupportedProviderPolicy(t *testing.T) {
	vault, err := secretcrypto.NewVault([]byte("12345678901234567890123456789012"))
	if err != nil {
		t.Fatalf("NewVault error: %v", err)
	}
	repo := store.NewMemoryRepository()
	encrypted, err := vault.Encrypt("access-token", []byte("conn-unsupported-revocation"))
	if err != nil {
		t.Fatalf("Encrypt error: %v", err)
	}
	_, err = repo.UpsertConnection(t.Context(), store.Connection{
		ID: "conn-unsupported-revocation", ProviderKey: "notion", ConnectorType: "notion",
		OrganizationID: "org-1", UserID: "user-1", Status: "active",
		EncryptedAccessToken: encrypted,
	})
	if err != nil {
		t.Fatalf("UpsertConnection error: %v", err)
	}
	service := NewService(config.Config{}, repo, vault, NewMicrosoftClient(MicrosoftClientConfig{}))
	service.clients["notion"] = &revocationSequenceClient{errors: []error{ErrRevocationUnsupported}}

	deleted, err := service.DisconnectConnection(t.Context(), "conn-unsupported-revocation", "test")
	if err != nil {
		t.Fatalf("DisconnectConnection error: %v", err)
	}
	if deleted.Status != "deleted" || deleted.DeletedAt == nil || deleted.EncryptedAccessToken != "" {
		t.Fatalf("unsupported policy state = %q/%v token_empty=%v, want explicit local deletion", deleted.Status, deleted.DeletedAt, deleted.EncryptedAccessToken == "")
	}
	event, found, err := repo.ClaimAuditEvent(t.Context())
	for err == nil && found && event.EventType != "connection.deleted" {
		event, found, err = repo.ClaimAuditEvent(t.Context())
	}
	if err != nil || !found {
		t.Fatalf("claim connection.deleted audit = (%v, %v)", found, err)
	}
	if event.Metadata["revoke_status"] != "unsupported_provider_policy" {
		t.Fatalf("revoke_status = %#v, want explicit unsupported policy", event.Metadata["revoke_status"])
	}
}

type countingRefreshClient struct {
	calls     atomic.Int32
	wait      time.Duration
	expiresAt time.Time
}

type refreshErrorClient struct {
	calls atomic.Int32
	err   error
}

func (c *refreshErrorClient) AuthorizationURL(string, string, string, []string, map[string]string) (string, error) {
	return "", nil
}

func (c *refreshErrorClient) ExchangeCode(context.Context, string, string, string, []string, map[string]string) (TokenResult, error) {
	return TokenResult{}, nil
}

func (c *refreshErrorClient) Refresh(context.Context, string, []string, map[string]string) (TokenResult, error) {
	c.calls.Add(1)
	return TokenResult{}, c.err
}

func (c *refreshErrorClient) Profile(context.Context, string, map[string]string) (ProviderProfile, error) {
	return ProviderProfile{}, nil
}

func (c *refreshErrorClient) Revoke(context.Context, string, map[string]string) error {
	return nil
}

type revocationCountingClient struct {
	callbackClient
	revokeCalls atomic.Int32
}

type revocationSequenceClient struct {
	callbackClient
	calls  atomic.Int32
	errors []error
}

func (c *revocationSequenceClient) Revoke(context.Context, string, map[string]string) error {
	index := int(c.calls.Add(1)) - 1
	if index >= len(c.errors) {
		return nil
	}
	return c.errors[index]
}

func (c *revocationCountingClient) Revoke(context.Context, string, map[string]string) error {
	c.revokeCalls.Add(1)
	return nil
}

type refreshLockRepo struct {
	*store.MemoryRepository
	locks atomic.Int32
}

func (r *refreshLockRepo) WithConnectionRefreshLock(ctx context.Context, _ string, fn func(context.Context) error) error {
	r.locks.Add(1)
	return fn(ctx)
}

func (c *countingRefreshClient) AuthorizationURL(string, string, string, []string, map[string]string) (string, error) {
	return "", nil
}

func (c *countingRefreshClient) ExchangeCode(context.Context, string, string, string, []string, map[string]string) (TokenResult, error) {
	return TokenResult{}, nil
}

func (c *countingRefreshClient) Refresh(context.Context, string, []string, map[string]string) (TokenResult, error) {
	c.calls.Add(1)
	time.Sleep(c.wait)
	return TokenResult{
		AccessToken:  "new-access-token",
		RefreshToken: "new-refresh-token",
		ExpiresAt:    c.expiresAt,
	}, nil
}

func (c *countingRefreshClient) Profile(context.Context, string, map[string]string) (ProviderProfile, error) {
	return ProviderProfile{}, nil
}

func (c *countingRefreshClient) Revoke(context.Context, string, map[string]string) error {
	return nil
}

type unexpectedTokenError struct {
	got string
}

func (e *unexpectedTokenError) Error() string {
	return "AccessToken = " + e.got + ", want new-access-token"
}

type capturePublisher struct {
	events []events.Event
}

func (p *capturePublisher) Publish(_ context.Context, event events.Event) error {
	p.events = append(p.events, event)
	return nil
}

type callbackClient struct {
	authBaseURL         string
	token               TokenResult
	profile             ProviderProfile
	lastRedirectURI     string
	lastProviderContext map[string]string
}

func (c *callbackClient) AuthorizationURL(state, redirectURI string, _ string, _ []string, providerContext map[string]string) (string, error) {
	c.lastRedirectURI = redirectURI
	c.lastProviderContext = providerContext
	base := c.authBaseURL
	if base == "" {
		base = "https://auth.test/oauth"
	}
	values := url.Values{"state": {state}}
	return base + "?" + values.Encode(), nil
}

func (c *callbackClient) ExchangeCode(context.Context, string, string, string, []string, map[string]string) (TokenResult, error) {
	if c.token.AccessToken != "" {
		return c.token, nil
	}
	return TokenResult{AccessToken: "access-token", RefreshToken: "refresh-token", ExpiresAt: time.Now().Add(time.Hour)}, nil
}

func (c *callbackClient) Refresh(context.Context, string, []string, map[string]string) (TokenResult, error) {
	return TokenResult{AccessToken: "refreshed-token", RefreshToken: "refreshed-refresh-token", ExpiresAt: time.Now().Add(time.Hour)}, nil
}

func (c *callbackClient) Profile(context.Context, string, map[string]string) (ProviderProfile, error) {
	return c.profile, nil
}

func (c *callbackClient) Revoke(context.Context, string, map[string]string) error {
	return nil
}

func testOAuthConfig() config.Config {
	return config.Config{
		PublicBaseURL:             "https://integration.test",
		SessionTTL:                10 * time.Minute,
		TokenRefreshSkew:          2 * time.Minute,
		MicrosoftClientID:         "microsoft-client",
		MicrosoftClientSecret:     "microsoft-secret",
		MicrosoftAuthorizationURL: "https://login.test/oauth",
		MicrosoftTokenURL:         "https://login.test/token",
		MicrosoftGraphBaseURL:     "https://graph.test",
		SlackClientID:             "slack-client",
		SlackClientSecret:         "slack-secret",
		SlackAuthorizationURL:     "https://slack.test/oauth",
		SlackTokenURL:             "https://slack.test/token",
		GoogleClientID:            "google-client",
		GoogleClientSecret:        "google-secret",
		GoogleAuthorizationURL:    "https://google.test/oauth",
		GoogleTokenURL:            "https://google.test/token",
		NotionClientID:            "notion-client",
		NotionClientSecret:        "notion-secret",
		NotionAuthorizationURL:    "https://notion.test/oauth",
		NotionTokenURL:            "https://notion.test/token",
		GitHubClientID:            "github-client",
		GitHubClientSecret:        "github-secret",
		GitHubAuthorizationURL:    "https://github.test/oauth",
		GitHubTokenURL:            "https://github.test/token",
		ShopifyClientID:           "shopify-client",
		ShopifyClientSecret:       "shopify-secret",
		StripeClientID:            "stripe-client",
		StripeClientSecret:        "stripe-secret",
		StripeAuthorizationURL:    "https://stripe.test/oauth",
		StripeTokenURL:            "https://stripe.test/token",
		StripeAPIBaseURL:          "https://stripe.test",
		LinkedInClientID:          "linkedin-client",
		LinkedInClientSecret:      "linkedin-secret",
		LinkedInAuthorizationURL:  "https://linkedin.test/oauth",
		LinkedInTokenURL:          "https://linkedin.test/token",
		LinkedInAPIBaseURL:        "https://linkedin-api.test",
		XClientID:                 "x-client",
		XClientSecret:             "x-secret",
		XAuthorizationURL:         "https://x.test/oauth",
		XTokenURL:                 "https://x.test/token",
		XAPIBaseURL:               "https://x-api.test",
		InstagramClientID:         "instagram-client",
		InstagramClientSecret:     "instagram-secret",
		InstagramAuthorizationURL: "https://instagram.test/oauth",
		InstagramTokenURL:         "https://instagram.test/token",
		InstagramAPIBaseURL:       "https://instagram-api.test",
		FacebookClientID:          "facebook-client",
		FacebookClientSecret:      "facebook-secret",
		FacebookAuthorizationURL:  "https://facebook.test/oauth",
		FacebookTokenURL:          "https://facebook.test/token",
		FacebookAPIBaseURL:        "https://facebook-api.test",
		SnapchatClientID:          "snapchat-client",
		SnapchatClientSecret:      "snapchat-secret",
		SnapchatAuthorizationURL:  "https://snapchat.test/oauth",
		SnapchatTokenURL:          "https://snapchat.test/token",
		SnapchatAPIBaseURL:        "https://snapchat-api.test",
		TikTokClientKey:           "tiktok-client-key",
		TikTokClientSecret:        "tiktok-secret",
		TikTokAuthorizationURL:    "https://tiktok.test/oauth",
		TikTokTokenURL:            "https://tiktok.test/token",
		TikTokAPIBaseURL:          "https://tiktok-api.test",
		DiscordClientID:           "discord-client",
		DiscordClientSecret:       "discord-secret",
		DiscordAuthorizationURL:   "https://discord.test/oauth",
		DiscordTokenURL:           "https://discord.test/token",
		DiscordAPIBaseURL:         "https://discord-api.test",
		OktaDomain:                "https://okta.test",
		OktaClientID:              "okta-client",
		OktaClientSecret:          "okta-secret",
		OktaAPIToken:              "okta-token",
		OktaAPIBaseURL:            "https://okta.test",
		SCIMBearerToken:           "scim-token",
		InternalAPIKey:            "internal-key",
		AuthCoreInternalAPIKey:    "internal-key",
		AuthCoreURL:               "https://auth-core.test",
		OrgCoreURL:                "https://org-core.test",
		BillingCoreURL:            "https://billing-core.test",
		AuditCoreURL:              "https://audit-core.test",
		DataPlaneDocumentsURL:     "https://documents.test",
		DataPlaneGraphIndexURL:    "https://graph.test",
		FinspoCoreURL:             "https://finspo.test",
		GoogleAPIBaseURL:          "https://google-api.test",
		SlackAPIBaseURL:           "https://slack-api.test",
		NotionAPIBaseURL:          "https://notion-api.test",
		GitHubAPIBaseURL:          "https://github-api.test",
		ShopifyAPIBaseURL:         "https://shopify-api.test",
		MicrosoftTenantID:         "common",
		SCIMBearerTokens:          map[string]string{"org-1": "scim-token"},
		TokenLeaseConsumers:       []string{"finspo-core"},
		RateLimitMax:              120,
		RateLimitWindow:           time.Minute,
	}
}

func stateFromAuthURL(t *testing.T, rawURL string) string {
	t.Helper()
	parsed, err := url.Parse(rawURL)
	if err != nil {
		t.Fatalf("Parse AuthorizationURL error: %v", err)
	}
	state := parsed.Query().Get("state")
	if state == "" {
		t.Fatalf("AuthorizationURL %q did not contain state", rawURL)
	}
	return state
}

func hasString(values []string, want string) bool {
	for _, value := range values {
		if value == want {
			return true
		}
	}
	return false
}
