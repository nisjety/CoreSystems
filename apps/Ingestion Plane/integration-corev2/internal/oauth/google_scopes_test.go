package oauth

import (
	"context"
	"net/http"
	"net/http/httptest"
	"slices"
	"testing"
	"time"

	secretcrypto "github.com/triodelab/integration-corev2/internal/crypto"
	"github.com/triodelab/integration-corev2/internal/store"
)

func TestGoogleTokenDistinguishesOmittedFromEmptyScope(t *testing.T) {
	for _, tc := range []struct {
		body     string
		verified bool
	}{{`{"access_token":"synthetic","scope":""}`, true}, {`{"access_token":"synthetic","scope":"openid"}`, true}, {`{"access_token":"synthetic"}`, false}} {
		server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) { _, _ = w.Write([]byte(tc.body)) }))
		client := NewOAuth2Client(OAuth2ClientConfig{ProviderKey: "google", TokenURL: server.URL, HTTPClient: server.Client()})
		got, err := client.ExchangeCode(context.Background(), "synthetic", "https://app.test/callback", "verifier", nil, nil)
		server.Close()
		if err != nil || got.ScopesVerified != tc.verified {
			t.Fatalf("token scope verification=%v err=%v", got.ScopesVerified, err)
		}
	}
}

func TestGoogleReconnectPreservesOnlyProviderConfirmedExistingCapabilities(t *testing.T) {
	for _, tc := range []struct {
		name          string
		scope         []string
		wantDrive     bool
		wantAnalytics bool
	}{
		{"existinggrantconfirmed", []string{"openid", "https://www.googleapis.com/auth/userinfo.email", "https://www.googleapis.com/auth/userinfo.profile", "https://www.googleapis.com/auth/webmasters.readonly", "https://www.googleapis.com/auth/drive.readonly"}, true, false},
		{"existingandnewconfirmed", []string{"openid", "email", "profile", "https://www.googleapis.com/auth/webmasters.readonly", "https://www.googleapis.com/auth/drive.readonly", "https://www.googleapis.com/auth/analytics.readonly"}, true, true},
		{"existinggrantnotconfirmed", []string{"openid", "email", "profile", "https://www.googleapis.com/auth/webmasters.readonly"}, false, false},
		{"explicitlyempty", []string{}, false, false},
	} {
		t.Run(tc.name, func(t *testing.T) {
			vault, err := secretcrypto.NewVault([]byte("12345678901234567890123456789012"))
			if err != nil {
				t.Fatal(err)
			}
			repo := store.NewMemoryRepository()
			_, err = repo.UpsertConnection(t.Context(), store.Connection{ID: "existing", ProviderKey: "google", ConnectorType: "google", OrganizationID: "org-1", ProviderAccountID: "account-a", Status: "active", Capabilities: []string{"profile.read", "drive.read"}})
			if err != nil {
				t.Fatal(err)
			}
			service := NewService(testOAuthConfig(), repo, vault, NewMicrosoftClient(MicrosoftClientConfig{}))
			service.clients["google"] = &callbackClient{profile: ProviderProfile{ID: "account-a"}}
			got, err := service.persistConnection(t.Context(), store.ConnectSession{ID: "newsession", ProviderKey: "google", ConnectorType: "google", OrganizationID: "org-1", UserID: "user-1", Capabilities: []string{"profile.read", "search_console.read", "analytics.read"}, Scopes: []string{"openid", "email", "profile", "https://www.googleapis.com/auth/webmasters.readonly", "https://www.googleapis.com/auth/analytics.readonly"}}, TokenResult{AccessToken: "synthetic", ExpiresAt: time.Now().Add(time.Hour), Scope: tc.scope, ScopesVerified: true})
			if err != nil {
				t.Fatal(err)
			}
			if slices.Contains(got.Capabilities, "analytics.read") != tc.wantAnalytics || slices.Contains(got.Scopes, "https://www.googleapis.com/auth/analytics.readonly") != tc.wantAnalytics {
				t.Fatal("declined Analytics scope marked granted")
			}
			if slices.Contains(got.Capabilities, "drive.read") != tc.wantDrive {
				t.Fatalf("existing capability=%#v", got.Capabilities)
			}
			if len(tc.scope) > 0 && !slices.Contains(got.Capabilities, "profile.read") {
				t.Fatal("Google equivalent userinfo scopes lost profile capability")
			}
			if got.ID != "existing" || got.Status != "active" {
				t.Fatalf("reconnect binding=%+v", got)
			}
		})
	}
}

func TestGoogleReconnectWithoutGrantSnapshotCannotDowngradeExistingConnection(t *testing.T) {
	vault, err := secretcrypto.NewVault([]byte("12345678901234567890123456789012"))
	if err != nil {
		t.Fatal(err)
	}
	repo := store.NewMemoryRepository()
	original := store.Connection{ID: "existing", ProviderKey: "google", ConnectorType: "google", OrganizationID: "org-1", ProviderAccountID: "account-a", Status: "active", Capabilities: []string{"profile.read", "drive.read"}, EncryptedAccessToken: "original-encrypted-grant"}
	if _, err := repo.UpsertConnection(t.Context(), original); err != nil {
		t.Fatal(err)
	}
	service := NewService(testOAuthConfig(), repo, vault, NewMicrosoftClient(MicrosoftClientConfig{}))
	service.clients["google"] = &callbackClient{profile: ProviderProfile{ID: "account-a"}}
	_, err = service.persistConnection(t.Context(), store.ConnectSession{ID: "newsession", ProviderKey: "google", ConnectorType: "google", OrganizationID: "org-1", UserID: "user-1", Capabilities: []string{"profile.read", "search_console.read", "analytics.read"}}, TokenResult{AccessToken: "synthetic", ExpiresAt: time.Now().Add(time.Hour)})
	if err == nil {
		t.Fatal("accepted reconnect without proof of existing grants")
	}
	current, err := repo.GetConnection(t.Context(), original.ID)
	if err != nil || current.EncryptedAccessToken != original.EncryptedAccessToken || !slices.Equal(current.Capabilities, original.Capabilities) {
		t.Fatal("unconfirmed reconnect changed existing grant")
	}
}
