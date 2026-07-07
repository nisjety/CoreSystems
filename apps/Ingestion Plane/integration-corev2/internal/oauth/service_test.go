package oauth

import (
	"context"
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
				providerContext = map[string]string{"shop": "velion.myshopify.com"}
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

type countingRefreshClient struct {
	calls     atomic.Int32
	wait      time.Duration
	expiresAt time.Time
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
