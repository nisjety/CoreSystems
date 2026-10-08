package oauth

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"net/url"
	"slices"
	"testing"
	"time"

	secretcrypto "github.com/triodelab/integration-corev2/internal/crypto"
	"github.com/triodelab/integration-corev2/internal/providers"
	"github.com/triodelab/integration-corev2/internal/store"
)

// G1: OAuth scope upgrades must be additive and truthful. A reconnect that
// asks for a new bundle must never silently remove a capability the user had
// already granted, and a capability may only be recorded when the provider's
// own token response shows its scopes were granted.
//
// The catalog has no calendar write capability yet, so sharepoint.write
// (Files.ReadWrite.All, Sites.ReadWrite.All) and gmail.send stand in for "a
// previously granted write capability".

func additiveTestService(t *testing.T) (*Service, *store.MemoryRepository) {
	t.Helper()
	vault, err := secretcrypto.NewVault([]byte("12345678901234567890123456789012"))
	if err != nil {
		t.Fatalf("NewVault error: %v", err)
	}
	repo := store.NewMemoryRepository()
	return NewService(testOAuthConfig(), repo, vault, NewMicrosoftClient(MicrosoftClientConfig{})), repo
}

type seededConnection struct {
	id           string
	providerKey  string
	connector    string
	userID       string
	accountID    string
	status       string
	capabilities []string
}

func seedPersonalConnection(t *testing.T, repo *store.MemoryRepository, seed seededConnection) store.Connection {
	t.Helper()
	provider, ok := providers.FindOAuth(seed.providerKey)
	if !ok {
		t.Fatalf("catalog is missing provider %q", seed.providerKey)
	}
	status := seed.status
	if status == "" {
		status = "active"
	}
	saved, err := repo.UpsertConnection(t.Context(), store.Connection{
		ID:                   seed.id,
		ProviderKey:          seed.providerKey,
		ConnectorType:        seed.connector,
		OrganizationID:       "org-1",
		WorkspaceID:          "workspace-1",
		UserID:               seed.userID,
		ProviderAccountID:    seed.accountID,
		Status:               status,
		Capabilities:         append([]string{}, seed.capabilities...),
		Scopes:               providers.ResolveScopes(provider, seed.capabilities),
		AccessTokenExpiresAt: time.Now().Add(time.Hour),
		CreatedAt:            time.Now().Add(-time.Hour).UTC(),
	})
	if err != nil {
		t.Fatalf("seed connection %s: %v", seed.id, err)
	}
	return saved
}

func additiveSessionInput(providerKey string, bundles ...string) CreateSessionInput {
	return CreateSessionInput{
		ProviderKey:    providerKey,
		OrganizationID: "org-1",
		WorkspaceID:    "workspace-1",
		UserID:         "user-1",
		UserEmail:      "user-1@example.test",
		Bundles:        bundles,
		ReturnURL:      "https://app.test/settings",
	}
}

func requireContainsAll(t *testing.T, label string, got []string, want ...string) {
	t.Helper()
	for _, value := range want {
		if !slices.Contains(got, value) {
			t.Fatalf("%s = %v, missing %q", label, got, value)
		}
	}
}

func requireContainsNone(t *testing.T, label string, got []string, unwanted ...string) {
	t.Helper()
	for _, value := range unwanted {
		if slices.Contains(got, value) {
			t.Fatalf("%s = %v, must not contain %q", label, got, value)
		}
	}
}

func TestCreateSessionRequestsUnionOfGrantedCapabilitiesForPersonalProviders(t *testing.T) {
	cases := []struct {
		name        string
		providerKey string
		connector   string
		existing    []string
		bundles     []string
		wantCaps    []string
		wantScopes  []string
		wantNotCaps []string
	}{
		{
			name:        "microsoft inbox upgrade keeps a granted write",
			providerKey: "microsoft",
			connector:   "microsoft-graph",
			existing:    []string{"profile.read", "sharepoint.read", "sharepoint.write"},
			bundles:     []string{"onboarding", "inbox"},
			wantCaps:    []string{"sharepoint.write", "sharepoint.read", "mail.read", "mail.send", "teams.messages.read"},
			wantScopes:  []string{"Files.ReadWrite.All", "Sites.ReadWrite.All", "Mail.Read", "Mail.Send"},
			wantNotCaps: []string{"calendar.read"},
		},
		{
			name:        "google knowledge upgrade keeps granted send and calendar",
			providerKey: "google",
			connector:   "google-workspace",
			existing:    []string{"profile.read", "gmail.send", "calendar.read"},
			bundles:     []string{"knowledge"},
			wantCaps:    []string{"gmail.send", "calendar.read", "drive.read"},
			wantScopes: []string{
				"https://www.googleapis.com/auth/gmail.send",
				"https://www.googleapis.com/auth/calendar.readonly",
				"https://www.googleapis.com/auth/drive.readonly",
			},
			wantNotCaps: []string{"ads.manage", "gmail.read"},
		},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			service, repo := additiveTestService(t)
			service.clients[tc.providerKey] = &callbackClient{}
			existing := seedPersonalConnection(t, repo, seededConnection{
				id: "conn-existing", providerKey: tc.providerKey, connector: tc.connector,
				userID: "user-1", accountID: "acct-1", capabilities: tc.existing,
			})

			result, err := service.CreateSession(t.Context(), additiveSessionInput(tc.providerKey, tc.bundles...))
			if err != nil {
				t.Fatalf("CreateSession error: %v", err)
			}
			requireContainsAll(t, "result capabilities", result.Capabilities, tc.wantCaps...)
			requireContainsAll(t, "result scopes", result.Scopes, tc.wantScopes...)
			requireContainsNone(t, "result capabilities", result.Capabilities, tc.wantNotCaps...)

			session, err := repo.GetConnectSessionByID(t.Context(), result.SessionToken)
			if err != nil {
				t.Fatalf("GetConnectSessionByID error: %v", err)
			}
			requireContainsAll(t, "stored session capabilities", session.Capabilities, tc.wantCaps...)
			requireContainsAll(t, "stored session scopes", session.Scopes, tc.wantScopes...)

			// Asking for a bigger grant must not itself touch the live connection.
			after, err := repo.GetConnection(t.Context(), existing.ID)
			if err != nil {
				t.Fatalf("GetConnection error: %v", err)
			}
			if !slices.Equal(after.Capabilities, existing.Capabilities) {
				t.Fatalf("CreateSession changed the live connection: %v -> %v", existing.Capabilities, after.Capabilities)
			}
		})
	}
}

func TestCreateSessionKeepsNeedsRefreshConnectionGrantsAndIgnoresDeletedOnes(t *testing.T) {
	service, repo := additiveTestService(t)
	service.clients["microsoft"] = &callbackClient{}
	seedPersonalConnection(t, repo, seededConnection{
		id: "conn-needs-refresh", providerKey: "microsoft", connector: "microsoft-graph",
		userID: "user-1", accountID: "acct-1", status: "needs_refresh",
		capabilities: []string{"profile.read", "sharepoint.write"},
	})
	deleted := seedPersonalConnection(t, repo, seededConnection{
		id: "conn-deleted", providerKey: "microsoft", connector: "microsoft-graph",
		userID: "user-1", accountID: "acct-old", capabilities: []string{"profile.read", "mail.send"},
	})
	if _, err := repo.MarkConnectionDeleted(t.Context(), deleted.ID); err != nil {
		t.Fatalf("MarkConnectionDeleted error: %v", err)
	}

	result, err := service.CreateSession(t.Context(), additiveSessionInput("microsoft", "onboarding"))
	if err != nil {
		t.Fatalf("CreateSession error: %v", err)
	}
	requireContainsAll(t, "capabilities", result.Capabilities, "sharepoint.write")
	requireContainsNone(t, "capabilities", result.Capabilities, "mail.send")
}

func TestCreateSessionDoesNotBorrowGrantsFromOtherUsersOrOrgWideProviders(t *testing.T) {
	service, repo := additiveTestService(t)
	service.clients["microsoft"] = &callbackClient{}
	service.clients["github"] = &callbackClient{}
	seedPersonalConnection(t, repo, seededConnection{
		id: "conn-other-user", providerKey: "microsoft", connector: "microsoft-graph",
		userID: "user-2", accountID: "acct-2", capabilities: []string{"profile.read", "sharepoint.write"},
	})
	seedPersonalConnection(t, repo, seededConnection{
		id: "conn-github", providerKey: "github", connector: "github",
		userID: "user-1", accountID: "gh-1", capabilities: []string{"profile.read", "repo.private.read"},
	})

	microsoft, err := service.CreateSession(t.Context(), additiveSessionInput("microsoft", "onboarding"))
	if err != nil {
		t.Fatalf("CreateSession microsoft error: %v", err)
	}
	requireContainsNone(t, "another user's grants", microsoft.Capabilities, "sharepoint.write")
	requireContainsNone(t, "another user's scopes", microsoft.Scopes, "Files.ReadWrite.All")

	github, err := service.CreateSession(t.Context(), additiveSessionInput("github", "onboarding"))
	if err != nil {
		t.Fatalf("CreateSession github error: %v", err)
	}
	requireContainsNone(t, "org-wide provider grants", github.Capabilities, "repo.private.read")
}

func TestCreateSessionDoesNotGuessWhichAccountWhenUserHasSeveralConnections(t *testing.T) {
	service, repo := additiveTestService(t)
	service.clients["google"] = &callbackClient{}
	seedPersonalConnection(t, repo, seededConnection{
		id: "conn-account-a", providerKey: "google", connector: "google-workspace",
		userID: "user-1", accountID: "acct-a", capabilities: []string{"profile.read", "gmail.send"},
	})
	seedPersonalConnection(t, repo, seededConnection{
		id: "conn-account-b", providerKey: "google", connector: "google-workspace",
		userID: "user-1", accountID: "acct-b", capabilities: []string{"profile.read", "calendar.read"},
	})

	result, err := service.CreateSession(t.Context(), additiveSessionInput("google", "knowledge"))
	if err != nil {
		t.Fatalf("CreateSession error: %v", err)
	}
	// Which account the user will pick is unknown until the callback, so no
	// account's grants may be requested on the other's behalf.
	requireContainsNone(t, "capabilities", result.Capabilities, "gmail.send", "calendar.read")
	requireContainsAll(t, "capabilities", result.Capabilities, "drive.read")
}

func TestMicrosoftUpgradeReconnectKeepsEarlierGrantsEndToEnd(t *testing.T) {
	service, repo := additiveTestService(t)
	client := &callbackClient{profile: ProviderProfile{ID: "acct-1", DisplayName: "Ima"}}
	service.clients["microsoft"] = client
	seedPersonalConnection(t, repo, seededConnection{
		id: "conn-ms", providerKey: "microsoft", connector: "microsoft-graph",
		userID: "user-1", accountID: "acct-1",
		capabilities: []string{"profile.read", "sharepoint.read", "sharepoint.write"},
	})

	created, err := service.CreateSession(t.Context(), additiveSessionInput("microsoft", "onboarding", "inbox"))
	if err != nil {
		t.Fatalf("CreateSession error: %v", err)
	}
	// Microsoft returns every scope that was asked for.
	client.token = TokenResult{
		AccessToken: "upgraded-access", RefreshToken: "upgraded-refresh",
		Scope: created.Scopes, ScopesVerified: true, ExpiresAt: time.Now().Add(time.Hour),
	}
	callback, err := service.CompleteCallback(t.Context(), "microsoft", stateFromAuthURL(t, created.AuthorizationURL), "code", "", "")
	if err != nil {
		t.Fatalf("CompleteCallback error: %v", err)
	}
	if callback.ConnectionID != "conn-ms" {
		t.Fatalf("ConnectionID = %q, want the existing connection reused", callback.ConnectionID)
	}
	connection, err := repo.GetConnection(t.Context(), "conn-ms")
	if err != nil {
		t.Fatalf("GetConnection error: %v", err)
	}
	requireContainsAll(t, "capabilities", connection.Capabilities, "sharepoint.write", "sharepoint.read", "mail.read", "mail.send", "profile.read")
	requireContainsAll(t, "scopes", connection.Scopes, "Files.ReadWrite.All", "Sites.ReadWrite.All", "Mail.Send")
	if connection.Status != "active" {
		t.Fatalf("status = %q, want active after a full grant", connection.Status)
	}
}

func TestPersistConnectionCarriesGrantsTheSessionDidNotRequestForPersonalProviders(t *testing.T) {
	service, repo := additiveTestService(t)
	service.clients["microsoft"] = &callbackClient{profile: ProviderProfile{ID: "acct-1"}}
	seedPersonalConnection(t, repo, seededConnection{
		id: "conn-ms", providerKey: "microsoft", connector: "microsoft-graph",
		userID: "user-1", accountID: "acct-1",
		capabilities: []string{"profile.read", "sharepoint.write", "mail.send"},
	})
	provider, _ := providers.FindOAuth("microsoft")
	requested := providers.ResolveCapabilities(provider, nil, []string{"onboarding"})
	session := store.ConnectSession{
		ID: "session-reconnect", ProviderKey: "microsoft", ConnectorType: "microsoft-graph",
		OrganizationID: "org-1", WorkspaceID: "workspace-1", UserID: "user-1",
		Capabilities: requested, Scopes: providers.ResolveScopes(provider, requested),
	}

	connection, err := service.persistConnection(t.Context(), session, TokenResult{
		AccessToken: "t", ExpiresAt: time.Now().Add(time.Hour),
	})
	if err != nil {
		t.Fatalf("persistConnection error: %v", err)
	}
	requireContainsAll(t, "capabilities", connection.Capabilities, "profile.read", "sharepoint.read", "sharepoint.write", "mail.send")
	requireContainsAll(t, "scopes", connection.Scopes, "Files.ReadWrite.All", "Mail.Send", "Files.Read.All")
}

func TestPersistConnectionDoesNotCarryAnotherUsersGrants(t *testing.T) {
	service, repo := additiveTestService(t)
	// A profile lookup that yields no account id falls back to the org's
	// connection for the connector, which may belong to someone else.
	service.clients["microsoft"] = &callbackClient{}
	seedPersonalConnection(t, repo, seededConnection{
		id: "conn-someone-else", providerKey: "microsoft", connector: "microsoft-graph",
		userID: "user-2", accountID: "", capabilities: []string{"profile.read", "sharepoint.write"},
	})
	provider, _ := providers.FindOAuth("microsoft")
	requested := providers.ResolveCapabilities(provider, nil, []string{"onboarding"})
	session := store.ConnectSession{
		ID: "session-user-1", ProviderKey: "microsoft", ConnectorType: "microsoft-graph",
		OrganizationID: "org-1", WorkspaceID: "workspace-1", UserID: "user-1",
		Capabilities: requested, Scopes: providers.ResolveScopes(provider, requested),
	}

	connection, err := service.persistConnection(t.Context(), session, TokenResult{
		AccessToken: "t", ExpiresAt: time.Now().Add(time.Hour),
	})
	if err != nil {
		t.Fatalf("persistConnection error: %v", err)
	}
	requireContainsNone(t, "capabilities", connection.Capabilities, "sharepoint.write")
	requireContainsNone(t, "scopes", connection.Scopes, "Files.ReadWrite.All")
}

func TestPersistMicrosoftConnectionRecordsOnlyCapabilitiesTheTokenGrants(t *testing.T) {
	service, _ := additiveTestService(t)
	service.clients["microsoft"] = &callbackClient{}
	provider, _ := providers.FindOAuth("microsoft")
	requested := providers.ResolveCapabilities(provider, nil, []string{"inbox"})
	session := store.ConnectSession{
		ID: "session-narrow", ProviderKey: "microsoft", ConnectorType: "microsoft-graph",
		OrganizationID: "org-1", WorkspaceID: "workspace-1", UserID: "user-1",
		Capabilities: requested, Scopes: providers.ResolveScopes(provider, requested),
	}

	connection, err := service.persistConnection(t.Context(), session, TokenResult{
		AccessToken:    "t",
		Scope:          []string{"User.Read", "Mail.Read", "profile", "openid", "email"},
		ScopesVerified: true,
		ExpiresAt:      time.Now().Add(time.Hour),
	})
	if err != nil {
		t.Fatalf("persistConnection error: %v", err)
	}
	requireContainsAll(t, "capabilities", connection.Capabilities, "profile.read", "mail.read")
	requireContainsNone(t, "capabilities", connection.Capabilities, "mail.send", "teams.read", "teams.messages.read")
	requireContainsNone(t, "scopes", connection.Scopes, "Mail.Send", "Chat.Read")
	if connection.Status != "needs_refresh" {
		t.Fatalf("status = %q, want needs_refresh because fewer capabilities were granted than requested", connection.Status)
	}
}

func TestPersistMicrosoftConnectionAcceptsGraphPrefixedAndMixedCaseScopes(t *testing.T) {
	service, _ := additiveTestService(t)
	service.clients["microsoft"] = &callbackClient{}
	provider, _ := providers.FindOAuth("microsoft")
	requested := providers.ResolveCapabilities(provider, nil, []string{"inbox"})
	session := store.ConnectSession{
		ID: "session-prefixed", ProviderKey: "microsoft", ConnectorType: "microsoft-graph",
		OrganizationID: "org-1", WorkspaceID: "workspace-1", UserID: "user-1",
		Capabilities: requested, Scopes: providers.ResolveScopes(provider, requested),
	}
	granted := []string{
		"https://graph.microsoft.com/User.Read", "https://graph.microsoft.com/Mail.Read",
		"https://graph.microsoft.com/mail.send", "https://graph.microsoft.com/Team.ReadBasic.All",
		"https://graph.microsoft.com/Channel.ReadBasic.All", "https://graph.microsoft.com/ChannelMessage.Read.All",
		"https://graph.microsoft.com/Chat.Read",
	}

	connection, err := service.persistConnection(t.Context(), session, TokenResult{
		AccessToken: "t", Scope: granted, ScopesVerified: true, ExpiresAt: time.Now().Add(time.Hour),
	})
	if err != nil {
		t.Fatalf("persistConnection error: %v", err)
	}
	requireContainsAll(t, "capabilities", connection.Capabilities, requested...)
	if connection.Status != "active" {
		t.Fatalf("status = %q, want active when every requested scope was granted", connection.Status)
	}
	requireContainsNone(t, "scopes", connection.Scopes, "https://graph.microsoft.com/Mail.Read")
	requireContainsAll(t, "scopes", connection.Scopes, "Mail.Read")
}

func TestPersistMicrosoftConnectionTrustsSessionWhenTokenCarriesNoScope(t *testing.T) {
	service, _ := additiveTestService(t)
	service.clients["microsoft"] = &callbackClient{}
	provider, _ := providers.FindOAuth("microsoft")
	requested := providers.ResolveCapabilities(provider, nil, []string{"inbox"})
	session := store.ConnectSession{
		ID: "session-unverified", ProviderKey: "microsoft", ConnectorType: "microsoft-graph",
		OrganizationID: "org-1", WorkspaceID: "workspace-1", UserID: "user-1",
		Capabilities: requested, Scopes: providers.ResolveScopes(provider, requested),
	}

	connection, err := service.persistConnection(t.Context(), session, TokenResult{
		AccessToken: "t", ExpiresAt: time.Now().Add(time.Hour),
	})
	if err != nil {
		t.Fatalf("persistConnection error: %v", err)
	}
	requireContainsAll(t, "capabilities", connection.Capabilities, requested...)
	if connection.Status != "active" {
		t.Fatalf("status = %q, want active when the token response carries no scope to verify", connection.Status)
	}
}

func TestPersistMicrosoftConnectionDropsRequestedGrantTheTokenNoLongerHolds(t *testing.T) {
	service, repo := additiveTestService(t)
	service.clients["microsoft"] = &callbackClient{profile: ProviderProfile{ID: "acct-1"}}
	seedPersonalConnection(t, repo, seededConnection{
		id: "conn-ms", providerKey: "microsoft", connector: "microsoft-graph",
		userID: "user-1", accountID: "acct-1",
		capabilities: []string{"profile.read", "mail.send", "sharepoint.write"},
	})
	provider, _ := providers.FindOAuth("microsoft")
	// The additive session asked for everything the connection had plus mail.read.
	requested := []string{"profile.read", "mail.send", "mail.read", "sharepoint.write", "sharepoint.read"}
	session := store.ConnectSession{
		ID: "session-lost-grant", ProviderKey: "microsoft", ConnectorType: "microsoft-graph",
		OrganizationID: "org-1", WorkspaceID: "workspace-1", UserID: "user-1",
		Capabilities: requested, Scopes: providers.ResolveScopes(provider, requested),
	}

	connection, err := service.persistConnection(t.Context(), session, TokenResult{
		AccessToken: "t",
		// Mail.Send is missing although it was requested: the grant is gone.
		Scope:          []string{"User.Read", "Mail.Read", "Files.ReadWrite.All", "Sites.ReadWrite.All", "Files.Read.All", "Sites.Read.All"},
		ScopesVerified: true, ExpiresAt: time.Now().Add(time.Hour),
	})
	if err != nil {
		t.Fatalf("persistConnection error: %v", err)
	}
	requireContainsNone(t, "capabilities", connection.Capabilities, "mail.send")
	requireContainsAll(t, "capabilities", connection.Capabilities, "mail.read", "sharepoint.write")
	if connection.Status != "needs_refresh" {
		t.Fatalf("status = %q, want needs_refresh when a requested grant is missing", connection.Status)
	}
}

func TestCreateSessionReplaceCapabilitiesRequestsOnlyWhatWasAsked(t *testing.T) {
	service, repo := additiveTestService(t)
	client := &callbackClient{}
	service.clients["microsoft"] = client
	seedPersonalConnection(t, repo, seededConnection{
		id: "conn-existing", providerKey: "microsoft", connector: "microsoft-graph",
		userID: "user-1", accountID: "acct-1",
		capabilities: []string{"profile.read", "sharepoint.read", "sharepoint.write"},
	})
	input := additiveSessionInput("microsoft", "onboarding")
	input.ReplaceCapabilities = true

	result, err := service.CreateSession(t.Context(), input)
	if err != nil {
		t.Fatalf("CreateSession error: %v", err)
	}
	requireContainsNone(t, "capabilities", result.Capabilities, "sharepoint.write")
	requireContainsNone(t, "scopes", result.Scopes, "Files.ReadWrite.All")
	if _, leaked := client.lastProviderContext[replaceCapabilitiesContextKey]; leaked {
		t.Fatalf("replace marker reached the provider client: %v", client.lastProviderContext)
	}
	session, err := repo.GetConnectSessionByID(t.Context(), result.SessionToken)
	if err != nil {
		t.Fatalf("GetConnectSessionByID error: %v", err)
	}
	if session.ProviderContext[replaceCapabilitiesContextKey] == "" {
		t.Fatalf("session lost the replace marker, so the callback could not honor it: %v", session.ProviderContext)
	}
}

func TestReplaceSessionOverwritesStoredCapabilitiesAndStripsMarker(t *testing.T) {
	service, repo := additiveTestService(t)
	client := &callbackClient{profile: ProviderProfile{ID: "acct-1"}}
	service.clients["microsoft"] = client
	seedPersonalConnection(t, repo, seededConnection{
		id: "conn-ms", providerKey: "microsoft", connector: "microsoft-graph",
		userID: "user-1", accountID: "acct-1",
		capabilities: []string{"profile.read", "sharepoint.read", "sharepoint.write"},
	})
	input := additiveSessionInput("microsoft", "onboarding")
	input.ReplaceCapabilities = true
	created, err := service.CreateSession(t.Context(), input)
	if err != nil {
		t.Fatalf("CreateSession error: %v", err)
	}
	client.token = TokenResult{
		AccessToken: "t", Scope: created.Scopes, ScopesVerified: true, ExpiresAt: time.Now().Add(time.Hour),
	}

	callback, err := service.CompleteCallback(t.Context(), "microsoft", stateFromAuthURL(t, created.AuthorizationURL), "code", "", "")
	if err != nil {
		t.Fatalf("CompleteCallback error: %v", err)
	}
	connection, err := repo.GetConnection(t.Context(), callback.ConnectionID)
	if err != nil {
		t.Fatalf("GetConnection error: %v", err)
	}
	requireContainsNone(t, "capabilities", connection.Capabilities, "sharepoint.write")
	requireContainsNone(t, "scopes", connection.Scopes, "Files.ReadWrite.All")
	if _, stored := connection.ProviderContext[replaceCapabilitiesContextKey]; stored {
		t.Fatalf("replace marker leaked into the stored connection: %v", connection.ProviderContext)
	}
}

func TestCallerCannotSmuggleTheReplaceMarkerThroughProviderContext(t *testing.T) {
	service, repo := additiveTestService(t)
	service.clients["microsoft"] = &callbackClient{}
	seedPersonalConnection(t, repo, seededConnection{
		id: "conn-existing", providerKey: "microsoft", connector: "microsoft-graph",
		userID: "user-1", accountID: "acct-1", capabilities: []string{"profile.read", "sharepoint.write"},
	})
	input := additiveSessionInput("microsoft", "onboarding")
	input.ProviderContext = map[string]string{replaceCapabilitiesContextKey: "true"}

	result, err := service.CreateSession(t.Context(), input)
	if err != nil {
		t.Fatalf("CreateSession error: %v", err)
	}
	requireContainsAll(t, "capabilities", result.Capabilities, "sharepoint.write")
	session, _ := repo.GetConnectSessionByID(t.Context(), result.SessionToken)
	if session.ProviderContext[replaceCapabilitiesContextKey] != "" {
		t.Fatalf("a caller-supplied marker survived into the session: %v", session.ProviderContext)
	}
}

func TestReplaceCapabilitiesIsIgnoredForOrgWideProviders(t *testing.T) {
	service, repo := additiveTestService(t)
	service.clients["github"] = &callbackClient{}
	input := additiveSessionInput("github", "onboarding")
	input.ReplaceCapabilities = true

	result, err := service.CreateSession(t.Context(), input)
	if err != nil {
		t.Fatalf("CreateSession error: %v", err)
	}
	session, _ := repo.GetConnectSessionByID(t.Context(), result.SessionToken)
	if _, marked := session.ProviderContext[replaceCapabilitiesContextKey]; marked {
		t.Fatalf("org-wide provider session carries a personal-connection marker: %v", session.ProviderContext)
	}
}

func TestMicrosoftTokenResponseScopeIsVerifiedOnlyWhenPresent(t *testing.T) {
	respond := func(body map[string]any) *MicrosoftClient {
		server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
			w.Header().Set("Content-Type", "application/json")
			_ = json.NewEncoder(w).Encode(body)
		}))
		t.Cleanup(server.Close)
		return NewMicrosoftClient(MicrosoftClientConfig{ClientID: "client", TokenURL: server.URL})
	}

	withScope, err := respond(map[string]any{
		"access_token": "a", "expires_in": 3600, "scope": "User.Read Mail.Read profile openid email",
	}).ExchangeCode(context.Background(), "code", "https://app.test/cb", "verifier", nil)
	if err != nil {
		t.Fatalf("ExchangeCode error: %v", err)
	}
	if !withScope.ScopesVerified || !slices.Contains(withScope.Scope, "Mail.Read") {
		t.Fatalf("token with scope: verified=%v scope=%v, want an authoritative snapshot", withScope.ScopesVerified, withScope.Scope)
	}

	refreshed, err := respond(map[string]any{
		"access_token": "a", "expires_in": 3600, "scope": "User.Read",
	}).Refresh(context.Background(), "refresh", nil)
	if err != nil {
		t.Fatalf("Refresh error: %v", err)
	}
	if !refreshed.ScopesVerified {
		t.Fatal("refresh response with scope was not marked verified")
	}

	withoutScope, err := respond(map[string]any{"access_token": "a", "expires_in": 3600}).
		ExchangeCode(context.Background(), "code", "https://app.test/cb", "verifier", nil)
	if err != nil {
		t.Fatalf("ExchangeCode without scope error: %v", err)
	}
	if withoutScope.ScopesVerified {
		t.Fatal("a token response without scope must not claim an authoritative grant snapshot")
	}
}

func TestGoogleAuthorizationRequestIncludesPreviouslyGrantedScopes(t *testing.T) {
	cfg := testOAuthConfig()
	clients := NewProviderClients(cfg, NewMicrosoftClient(MicrosoftClientConfig{}), nil)
	rawURL, err := clients["google"].AuthorizationURL("state", "https://app.test/oauth/callback/google", "verifier", []string{"openid", "email"}, nil)
	if err != nil {
		t.Fatalf("AuthorizationURL error: %v", err)
	}
	parsed, err := url.Parse(rawURL)
	if err != nil {
		t.Fatalf("url parse error: %v", err)
	}
	values := parsed.Query()
	if values.Get("include_granted_scopes") != "true" {
		t.Fatalf("include_granted_scopes = %q, want true so a later bundle's token carries earlier grants", values.Get("include_granted_scopes"))
	}
	if values.Get("access_type") != "offline" || values.Get("prompt") != "select_account consent" {
		t.Fatalf("existing Google params changed: access_type=%q prompt=%q", values.Get("access_type"), values.Get("prompt"))
	}
}
