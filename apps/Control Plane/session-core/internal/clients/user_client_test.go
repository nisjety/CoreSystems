package clients

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"testing"

	"github.com/stretchr/testify/require"
)

func TestUserClientGetProfileDecodesUserCoreEnvelope(t *testing.T) {
	t.Parallel()

	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		require.Equal(t, "/api/v1/users/me", r.URL.Path)
		require.Equal(t, "secret", r.Header.Get("X-Service-Token"))
		require.Equal(t, "session-core", r.Header.Get("X-Service-Id"))
		require.Empty(t, r.Header.Get("X-Internal-Api-Key"))
		require.Equal(t, "auth-user-1", r.Header.Get("X-User-Id"))
		require.NotEmpty(t, r.Header.Get("X-Delegation-Timestamp"))
		require.Equal(t, "47DEQpj8HBSa-_TImW-5JCeuQeRkm5NMpJWZG3hSuFU", r.Header.Get("X-Delegation-Body-SHA256"))
		require.NotEmpty(t, r.Header.Get("X-Delegation-Signature"))

		require.NoError(t, json.NewEncoder(w).Encode(map[string]any{
			"user": map[string]any{
				"id":                  "auth-user-1",
				"email":               "ima@example.com",
				"name":                "Ima Dacosta",
				"avatar":              "https://cdn.example.com/avatar.png",
				"onboarding_complete": true,
			},
		}))
	}))
	defer server.Close()

	client := NewUserClient(server.URL, "secret")
	profile, err := client.GetProfile(context.Background(), "auth-user-1")

	require.NoError(t, err)
	require.Equal(t, &UserProfile{
		ID:                 "auth-user-1",
		Email:              "ima@example.com",
		Name:               "Ima Dacosta",
		Image:              "https://cdn.example.com/avatar.png",
		OnboardingComplete: true,
	}, profile)
}

func TestUserClientGetProfileDecodesFlatLegacyShape(t *testing.T) {
	t.Parallel()

	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		require.NoError(t, json.NewEncoder(w).Encode(map[string]any{
			"id":                 "auth-user-2",
			"email":              "team@example.com",
			"display_name":       "Team Member",
			"image":              "https://cdn.example.com/member.png",
			"onboardingComplete": true,
		}))
	}))
	defer server.Close()

	client := NewUserClient(server.URL, "secret")
	profile, err := client.GetProfile(context.Background(), "auth-user-2")

	require.NoError(t, err)
	require.Equal(t, &UserProfile{
		ID:                 "auth-user-2",
		Email:              "team@example.com",
		Name:               "Team Member",
		Image:              "https://cdn.example.com/member.png",
		OnboardingComplete: true,
	}, profile)
}
