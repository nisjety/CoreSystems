package oauth

import (
	"context"
	"errors"
	"slices"
	"testing"
	"time"

	secretcrypto "github.com/triodelab/integration-corev2/internal/crypto"
	"github.com/triodelab/integration-corev2/internal/store"
)

func newDelegatedTestService(t *testing.T, profile ProviderProfile) (*Service, *store.MemoryRepository) {
	t.Helper()
	cfg := testOAuthConfig()
	vault, err := secretcrypto.NewVault([]byte("12345678901234567890123456789012"))
	if err != nil {
		t.Fatalf("NewVault error: %v", err)
	}
	repo := store.NewMemoryRepository()
	service := NewService(cfg, repo, vault, NewMicrosoftClient(MicrosoftClientConfig{}))
	service.clients["microsoft"] = &callbackClient{profile: profile}
	return service, repo
}

// The sign-in scopes auth-core now requests: identity + the read side of the
// Microsoft catalog. Better Auth stores them comma-separated.
var signInScopes = []string{"email,openid,profile,User.Read,Files.Read.All,Sites.Read.All,Team.ReadBasic.All,Channel.ReadBasic.All,ChannelMessage.Read.All,Chat.Read,Mail.Read"}

func TestAdoptDelegatedTokenCreatesConnectionFromSignInScopes(t *testing.T) {
	service, repo := newDelegatedTestService(t, ProviderProfile{ID: "graph-oid-1", DisplayName: "Ima", Email: "ima@aquatiq.com", TenantID: "tenant-1"})
	expires := time.Now().Add(time.Hour).UTC().Truncate(time.Second)

	saved, created, err := service.AdoptDelegatedToken(context.Background(), DelegatedTokenInput{
		ProviderKey:       "microsoft",
		OrganizationID:    "org-1",
		UserID:            "user-1",
		UserEmail:         "ima@aquatiq.com",
		ProviderAccountID: "oidc-sub-1",
		AccessToken:       "graph-access-token",
		ExpiresAt:         expires,
		Scopes:            signInScopes,
		TokenRef:          "account-row-1",
	})
	if err != nil {
		t.Fatalf("AdoptDelegatedToken error: %v", err)
	}
	if !created {
		t.Fatal("created = false, want a new connection")
	}
	want := []string{"mail.read", "profile.read", "sharepoint.read", "teams.messages.read", "teams.read"}
	if !slices.Equal(saved.Capabilities, want) {
		t.Fatalf("capabilities = %v, want %v", saved.Capabilities, want)
	}
	if saved.Status != "active" || saved.ProviderAccountID != "graph-oid-1" || saved.TenantID != "tenant-1" {
		t.Fatalf("connection = %#v, want active with Graph identity", saved)
	}
	if saved.EncryptedRefreshToken != "" {
		t.Fatal("a Control Plane refresh token must never be adopted: it belongs to auth-core's Azure app")
	}
	if saved.ProviderContext[providerContextTokenRef] != "account-row-1" || saved.ProviderContext[providerContextTokenSource] != tokenSourceControlPlaneSignIn {
		t.Fatalf("providerContext = %v, want token ref + source recorded", saved.ProviderContext)
	}
	if saved.ProviderContext["mailbox_address"] != "ima@aquatiq.com" {
		t.Fatalf("mailbox_address = %q, want the Graph-confirmed email", saved.ProviderContext["mailbox_address"])
	}
	token, err := service.AccessTokenForConnection(context.Background(), saved.ID)
	if err != nil || token.AccessToken != "graph-access-token" || !token.ExpiresAt.Equal(expires) {
		t.Fatalf("AccessTokenForConnection = %#v, %v; want the delegated token", token, err)
	}
	stored, _ := repo.GetConnection(context.Background(), saved.ID)
	if stored.EncryptedAccessToken == "graph-access-token" {
		t.Fatal("access token was stored in plaintext")
	}
}

// The core guarantee: a sign-in can only widen a connection. An existing
// connection with mail.send (granted through the explicit connect flow) keeps
// it when a login token without Mail.Send arrives, and keeps its own refresh
// token.
func TestAdoptDelegatedTokenNeverNarrowsAndKeepsOwnRefreshToken(t *testing.T) {
	service, repo := newDelegatedTestService(t, ProviderProfile{ID: "graph-oid-1"})
	ctx := context.Background()
	existing := store.Connection{
		ID: "conn-existing", ProviderKey: "microsoft", ConnectorType: "microsoft-graph",
		OrganizationID: "org-1", WorkspaceID: "org-1", UserID: "user-1", Status: "needs_refresh",
		ProviderAccountID:     "graph-oid-1",
		Capabilities:          []string{"mail.read", "mail.send", "profile.read", "sharepoint.read"},
		Scopes:                []string{"Mail.Read", "Mail.Send", "Files.Read.All", "Sites.Read.All", "User.Read"},
		EncryptedRefreshToken: "our-own-encrypted-refresh-token",
		ProviderContext:       map[string]string{"mailbox_address": "ima@aquatiq.com", "custom": "kept"},
		LastSyncStatus:        "synced",
		CreatedAt:             time.Now().Add(-48 * time.Hour),
	}
	if _, err := repo.UpsertConnection(ctx, existing); err != nil {
		t.Fatalf("seed error: %v", err)
	}

	saved, created, err := service.AdoptDelegatedToken(ctx, DelegatedTokenInput{
		ProviderKey: "microsoft", OrganizationID: "org-1", UserID: "user-1",
		AccessToken: "fresh-login-token", ExpiresAt: time.Now().Add(time.Hour),
		Scopes:   []string{"openid profile email User.Read Mail.Read"}, // identity + mail read only
		TokenRef: "account-row-1",
	})
	if err != nil {
		t.Fatalf("AdoptDelegatedToken error: %v", err)
	}
	if created || saved.ID != "conn-existing" {
		t.Fatalf("created=%v id=%q, want the existing connection updated in place", created, saved.ID)
	}
	if !slices.Contains(saved.Capabilities, "mail.send") || !slices.Contains(saved.Capabilities, "sharepoint.read") {
		t.Fatalf("capabilities = %v, want mail.send and sharepoint.read preserved", saved.Capabilities)
	}
	if saved.EncryptedRefreshToken != "our-own-encrypted-refresh-token" {
		t.Fatalf("refresh token = %q, want our own kept", saved.EncryptedRefreshToken)
	}
	if saved.Status != "active" {
		t.Fatalf("status = %q, want the sign-in to revive a needs_refresh connection", saved.Status)
	}
	if saved.ProviderContext["custom"] != "kept" || saved.ProviderContext[providerContextTokenRef] != "account-row-1" {
		t.Fatalf("providerContext = %v, want existing keys kept and token ref added", saved.ProviderContext)
	}
	if saved.LastSyncStatus != "synced" || saved.CreatedAt.After(time.Now().Add(-47*time.Hour)) {
		t.Fatalf("history not preserved: lastSyncStatus=%q createdAt=%v", saved.LastSyncStatus, saved.CreatedAt)
	}
	connections, _ := repo.ListConnections(ctx, store.ConnectionFilter{OrganizationID: "org-1"})
	if len(connections) != 1 {
		t.Fatalf("connections = %d, want exactly one (no duplicate from the hand-off)", len(connections))
	}
}

// Better Auth's account_id is the OIDC subject while our connections carry
// the Graph object id; matching must go through the Graph profile, and fall
// back to the org's single connection when Graph is unreachable.
func TestAdoptDelegatedTokenMatchesExistingConnectionWithoutGraphProfile(t *testing.T) {
	service, repo := newDelegatedTestService(t, ProviderProfile{})
	ctx := context.Background()
	if _, err := repo.UpsertConnection(ctx, store.Connection{
		ID: "conn-org", ProviderKey: "microsoft", ConnectorType: "microsoft-graph", OrganizationID: "org-1",
		Status: "active", ProviderAccountID: "graph-oid-1", Capabilities: []string{"profile.read"}, CreatedAt: time.Now(),
	}); err != nil {
		t.Fatalf("seed error: %v", err)
	}
	saved, created, err := service.AdoptDelegatedToken(ctx, DelegatedTokenInput{
		ProviderKey: "microsoft", OrganizationID: "org-1", ProviderAccountID: "oidc-sub-differs",
		AccessToken: "token", Scopes: signInScopes,
	})
	if err != nil {
		t.Fatalf("AdoptDelegatedToken error: %v", err)
	}
	if created || saved.ID != "conn-org" || saved.ProviderAccountID != "graph-oid-1" {
		t.Fatalf("created=%v id=%q account=%q, want the org connection reused with its Graph id kept", created, saved.ID, saved.ProviderAccountID)
	}
	if saved.ProviderContext[providerContextSignInAccountID] != "oidc-sub-differs" {
		t.Fatalf("sign-in account id not recorded: %v", saved.ProviderContext)
	}
}

func TestAdoptDelegatedTokenRejectsIncompleteInput(t *testing.T) {
	service, _ := newDelegatedTestService(t, ProviderProfile{})
	if _, _, err := service.AdoptDelegatedToken(context.Background(), DelegatedTokenInput{ProviderKey: "microsoft", OrganizationID: "org-1"}); err == nil {
		t.Fatal("expected error for missing access token")
	}
	if _, _, err := service.AdoptDelegatedToken(context.Background(), DelegatedTokenInput{ProviderKey: "shipping", OrganizationID: "org-1", AccessToken: "t"}); err == nil {
		t.Fatal("expected error for a non-OAuth provider")
	}
}

type fakeControlPlaneTokens struct {
	token DelegatedToken
	err   error
	calls []string
}

func (f *fakeControlPlaneTokens) RefreshDelegatedToken(_ context.Context, tokenRef string) (DelegatedToken, error) {
	f.calls = append(f.calls, tokenRef)
	return f.token, f.err
}

type refreshFailingClient struct {
	callbackClient
	refreshErr error
}

func (c *refreshFailingClient) Refresh(context.Context, string, []string, map[string]string) (TokenResult, error) {
	return TokenResult{}, c.refreshErr
}

// A connection adopted from a sign-in has no refresh token of its own; when
// its access token expires the service asks auth-core to re-mint it.
func TestExpiredDelegatedConnectionRefreshesThroughControlPlane(t *testing.T) {
	service, repo := newDelegatedTestService(t, ProviderProfile{ID: "graph-oid-1"})
	ctx := context.Background()
	saved, _, err := service.AdoptDelegatedToken(ctx, DelegatedTokenInput{
		ProviderKey: "microsoft", OrganizationID: "org-1", UserID: "user-1",
		AccessToken: "expired-login-token", ExpiresAt: time.Now().Add(-time.Minute),
		Scopes: []string{"openid profile email User.Read"}, TokenRef: "account-row-1",
	})
	if err != nil {
		t.Fatalf("adopt error: %v", err)
	}
	if _, err := service.AccessTokenForConnection(ctx, saved.ID); err == nil {
		t.Fatal("expected failure with no refresh token and no Control Plane source")
	}

	cp := &fakeControlPlaneTokens{token: DelegatedToken{
		AccessToken: "re-minted-token", ExpiresAt: time.Now().Add(time.Hour),
		Scopes: []string{"openid profile email User.Read Mail.Read"},
	}}
	service.SetControlPlaneTokenSource(cp)
	result, err := service.AccessTokenForConnection(ctx, saved.ID)
	if err != nil {
		t.Fatalf("AccessTokenForConnection error: %v", err)
	}
	if result.AccessToken != "re-minted-token" {
		t.Fatalf("access token = %q, want the Control Plane re-mint", result.AccessToken)
	}
	if !slices.Equal(cp.calls, []string{"account-row-1"}) {
		t.Fatalf("control plane calls = %v, want the stored token ref", cp.calls)
	}
	// The re-mint carried a wider grant (Mail.Read): capabilities widen, never narrow.
	stored, _ := repo.GetConnection(ctx, saved.ID)
	if !slices.Contains(stored.Capabilities, "mail.read") || !slices.Contains(stored.Capabilities, "profile.read") {
		t.Fatalf("capabilities = %v, want mail.read added and profile.read kept", stored.Capabilities)
	}
	if stored.Status != "active" {
		t.Fatalf("status = %q, want active", stored.Status)
	}
}

// Credential-encryption keys can change during a local rollout. A connection
// linked to Better Auth must heal from its Control Plane token reference rather
// than asking the user to reconnect to Microsoft for a local key mismatch.
func TestUnreadableDelegatedCiphertextRefreshesThroughControlPlane(t *testing.T) {
	service, repo := newDelegatedTestService(t, ProviderProfile{})
	ctx := context.Background()
	oldVault, err := secretcrypto.NewVault([]byte("abcdefghijklmnopqrstuvwxyz123456"))
	if err != nil {
		t.Fatalf("old vault: %v", err)
	}
	unreadableAccess, err := oldVault.Encrypt("old-access-token", []byte("conn-key-rollout"))
	if err != nil {
		t.Fatalf("encrypt old access token: %v", err)
	}
	unreadableRefresh, err := oldVault.Encrypt("old-refresh-token", []byte("conn-key-rollout"))
	if err != nil {
		t.Fatalf("encrypt old refresh token: %v", err)
	}
	if _, err := repo.UpsertConnection(ctx, store.Connection{
		ID: "conn-key-rollout", ProviderKey: "microsoft", ConnectorType: "microsoft-graph", OrganizationID: "org-1",
		Status: "active", Capabilities: []string{"mail.read"}, Scopes: []string{"Mail.Read"},
		EncryptedAccessToken: unreadableAccess, EncryptedRefreshToken: unreadableRefresh,
		AccessTokenExpiresAt: time.Now().Add(time.Hour),
		ProviderContext:      map[string]string{providerContextTokenRef: "account-row-1"}, CreatedAt: time.Now(),
	}); err != nil {
		t.Fatalf("seed error: %v", err)
	}

	cp := &fakeControlPlaneTokens{token: DelegatedToken{AccessToken: "recovered-token", ExpiresAt: time.Now().Add(time.Hour)}}
	service.SetControlPlaneTokenSource(cp)
	result, err := service.AccessTokenForConnection(ctx, "conn-key-rollout")
	if err != nil || result.AccessToken != "recovered-token" {
		t.Fatalf("AccessTokenForConnection = %#v, %v; want Control Plane recovery", result, err)
	}
	stored, err := repo.GetConnection(ctx, "conn-key-rollout")
	if err != nil {
		t.Fatalf("stored connection: %v", err)
	}
	decrypted, err := service.vault.Decrypt(stored.EncryptedAccessToken, []byte(stored.ID))
	if err != nil || decrypted != "recovered-token" {
		t.Fatalf("re-encrypted access token = %q, %v; want recovered token under the active key", decrypted, err)
	}
	if !slices.Equal(cp.calls, []string{"account-row-1"}) {
		t.Fatalf("control plane calls = %v, want one re-mint", cp.calls)
	}
}

// When our own refresh token is dead (AADSTS70008 / invalid_grant) but the
// user still signs in, Control Plane's live credential recovers the connection
// instead of parking it in needs_refresh.
func TestDeadOwnRefreshTokenFallsBackToControlPlane(t *testing.T) {
	service, repo := newDelegatedTestService(t, ProviderProfile{ID: "graph-oid-1"})
	service.clients["microsoft"] = &refreshFailingClient{refreshErr: &TokenEndpointError{Code: "invalid_grant", Description: "AADSTS70008: refresh token expired"}}
	ctx := context.Background()
	ownRefresh, _ := service.vault.Encrypt("own-refresh", []byte("conn-own"))
	ownAccess, _ := service.vault.Encrypt("stale-access", []byte("conn-own"))
	if _, err := repo.UpsertConnection(ctx, store.Connection{
		ID: "conn-own", ProviderKey: "microsoft", ConnectorType: "microsoft-graph", OrganizationID: "org-1", UserID: "user-1",
		Status: "active", Capabilities: []string{"profile.read", "mail.read"}, Scopes: []string{"User.Read", "Mail.Read"},
		EncryptedAccessToken: ownAccess, EncryptedRefreshToken: ownRefresh, AccessTokenExpiresAt: time.Now().Add(-time.Minute),
		ProviderContext: map[string]string{providerContextTokenRef: "account-row-1"}, CreatedAt: time.Now(),
	}); err != nil {
		t.Fatalf("seed error: %v", err)
	}

	// Without a Control Plane source the dead refresh token parks the connection.
	if _, err := service.AccessTokenForConnection(ctx, "conn-own"); err == nil {
		t.Fatal("expected invalid_grant failure")
	}
	parked, _ := repo.GetConnection(ctx, "conn-own")
	if parked.Status != "needs_refresh" {
		t.Fatalf("status = %q, want needs_refresh after invalid_grant with no fallback", parked.Status)
	}

	// With one, the same situation recovers.
	parked.Status = "active"
	if _, err := repo.UpsertConnection(ctx, parked); err != nil {
		t.Fatalf("reset error: %v", err)
	}
	cp := &fakeControlPlaneTokens{token: DelegatedToken{AccessToken: "cp-token", ExpiresAt: time.Now().Add(time.Hour)}}
	service.SetControlPlaneTokenSource(cp)
	result, err := service.AccessTokenForConnection(ctx, "conn-own")
	if err != nil || result.AccessToken != "cp-token" {
		t.Fatalf("AccessTokenForConnection = %#v, %v; want Control Plane recovery", result, err)
	}
	recovered, _ := repo.GetConnection(ctx, "conn-own")
	if recovered.Status != "active" || recovered.EncryptedRefreshToken != ownRefresh {
		t.Fatalf("recovered = status %q refresh %q; want active with our refresh token untouched", recovered.Status, recovered.EncryptedRefreshToken)
	}
}

func TestControlPlaneFallbackDoesNotApplyWithoutTokenRefOrWhenUnavailable(t *testing.T) {
	service, repo := newDelegatedTestService(t, ProviderProfile{})
	ctx := context.Background()
	if _, err := repo.UpsertConnection(ctx, store.Connection{
		ID: "conn-noref", ProviderKey: "microsoft", ConnectorType: "microsoft-graph", OrganizationID: "org-1",
		Status: "active", AccessTokenExpiresAt: time.Now().Add(-time.Minute), CreatedAt: time.Now(),
	}); err != nil {
		t.Fatalf("seed error: %v", err)
	}
	cp := &fakeControlPlaneTokens{err: ErrControlPlaneTokenSourceUnavailable}
	service.SetControlPlaneTokenSource(cp)
	if _, err := service.AccessTokenForConnection(ctx, "conn-noref"); err == nil {
		t.Fatal("expected 'no refresh token' failure for a connection without a token ref")
	}
	if len(cp.calls) != 0 {
		t.Fatalf("control plane must not be called without a token ref; calls = %v", cp.calls)
	}

	if _, err := repo.UpsertConnection(ctx, store.Connection{
		ID: "conn-ref", ProviderKey: "microsoft", ConnectorType: "microsoft-graph", OrganizationID: "org-2",
		Status: "active", AccessTokenExpiresAt: time.Now().Add(-time.Minute), CreatedAt: time.Now(),
		ProviderContext: map[string]string{providerContextTokenRef: "row"},
	}); err != nil {
		t.Fatalf("seed error: %v", err)
	}
	_, err := service.AccessTokenForConnection(ctx, "conn-ref")
	if err == nil || errors.Is(err, ErrControlPlaneTokenSourceUnavailable) {
		t.Fatalf("err = %v, want the plain no-refresh-token error when the source is unavailable", err)
	}
}

func TestGrantedScopesForSignInNormalizesProviderForms(t *testing.T) {
	got := grantedScopesForSignIn([]string{"email,openid,profile,User.Read", "https://graph.microsoft.com/Mail.Read Files.Read.All"})
	for _, want := range []string{"openid", "profile", "email", "offline_access", "User.Read", "Mail.Read", "Files.Read.All"} {
		if !slices.Contains(got, want) {
			t.Fatalf("scopes %v lack %q", got, want)
		}
	}
	if slices.Contains(got, "https://graph.microsoft.com/Mail.Read") {
		t.Fatalf("Graph URL prefix must be stripped: %v", got)
	}
}
