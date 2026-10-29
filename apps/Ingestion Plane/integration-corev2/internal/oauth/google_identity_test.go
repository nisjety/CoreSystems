package oauth

import (
	"context"
	"testing"
	"time"

	secretcrypto "github.com/triodelab/integration-corev2/internal/crypto"
	"github.com/triodelab/integration-corev2/internal/store"
)

type unidentifiedGoogleClient struct{ callbackClient }

func (*unidentifiedGoogleClient) Profile(context.Context, string, map[string]string) (ProviderProfile, error) {
	return ProviderProfile{}, nil
}

func TestGoogleMissingProviderIdentityCannotReplaceExistingGrant(t *testing.T) {
	vault, err := secretcrypto.NewVault([]byte("12345678901234567890123456789012"))
	if err != nil {
		t.Fatal(err)
	}
	repo := store.NewMemoryRepository()
	original := store.Connection{ID: "conn-account-a", ProviderKey: "google", ConnectorType: "google", OrganizationID: "org-1", ProviderAccountID: "account-a", Status: "active", EncryptedAccessToken: "original-ciphertext"}
	if _, err := repo.UpsertConnection(t.Context(), original); err != nil {
		t.Fatal(err)
	}
	service := NewService(testOAuthConfig(), repo, vault, NewMicrosoftClient(MicrosoftClientConfig{}))
	service.clients["google"] = &unidentifiedGoogleClient{}
	_, err = service.persistConnection(t.Context(), store.ConnectSession{ID: "session-no-identity", ProviderKey: "google", ConnectorType: "google", OrganizationID: "org-1", UserID: "service:marketing-core"}, TokenResult{AccessToken: "replacement", ExpiresAt: time.Now().Add(time.Hour)})
	if err == nil {
		t.Fatal("Google connection accepted without provider-confirmed identity")
	}
	current, err := repo.GetConnection(t.Context(), original.ID)
	if err != nil || current.EncryptedAccessToken != original.EncryptedAccessToken || current.ProviderAccountID != original.ProviderAccountID {
		t.Fatal("failed identity lookup modified existing grant")
	}
	connections, err := repo.ListConnections(t.Context(), store.ConnectionFilter{OrganizationID: "org-1", ProviderKey: "google"})
	if err != nil || len(connections) != 1 {
		t.Fatal("failed identity lookup persisted a connection")
	}
}
