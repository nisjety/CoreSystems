package controlplane

import (
	"context"
	"encoding/json"
	"errors"
	"net/http"
	"net/http/httptest"
	"testing"
	"time"

	"github.com/triodelab/integration-corev2/internal/config"
	"github.com/triodelab/integration-corev2/internal/oauth"
)

func TestAuthOAuthClientRefreshesWithScopedPrincipalHeaders(t *testing.T) {
	var seen struct {
		path       string
		credential string
		principal  string
		auth       string
		body       map[string]string
	}
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		seen.path = r.URL.Path
		seen.credential = r.Header.Get("x-service-credential-id")
		seen.principal = r.Header.Get("x-service-principal")
		seen.auth = r.Header.Get("x-service-auth")
		_ = json.NewDecoder(r.Body).Decode(&seen.body)
		_ = json.NewEncoder(w).Encode(map[string]any{
			"refreshed": true, "access_token": "fresh", "expires_at": "2026-09-06T15:00:00Z",
			"scope": "email openid profile User.Read Mail.Read",
		})
	}))
	defer server.Close()

	client := NewAuthOAuthClient(config.Config{
		AuthCoreURL: server.URL, AuthCoreOAuthServiceToken: "s3cret-s3cret-s3cret-s3cret-s3cret-s3cret",
		AuthCoreOAuthCredentialID: "integration-core-primary", AuthCoreOAuthPrincipal: "integration-core",
	}, server.Client())
	token, err := client.RefreshDelegatedToken(context.Background(), "account-row-1")
	if err != nil {
		t.Fatalf("RefreshDelegatedToken error: %v", err)
	}
	if seen.path != "/internal/oauth/refresh" || seen.credential != "integration-core-primary" || seen.principal != "integration-core" || seen.auth != "s3cret-s3cret-s3cret-s3cret-s3cret-s3cret" {
		t.Fatalf("request = %#v, want auth-core's scoped principal contract", seen)
	}
	if seen.body["tokenRef"] != "account-row-1" {
		t.Fatalf("body = %v, want tokenRef", seen.body)
	}
	if token.AccessToken != "fresh" || !token.ExpiresAt.Equal(time.Date(2026, 9, 6, 15, 0, 0, 0, time.UTC)) {
		t.Fatalf("token = %#v", token)
	}
	if len(token.Scopes) != 5 {
		t.Fatalf("scopes = %v, want the space-separated grant split", token.Scopes)
	}
}

func TestAuthOAuthClientMapsRefusalsAndPrincipalRejection(t *testing.T) {
	cases := []struct {
		name        string
		status      int
		body        string
		wantUnavail bool
		wantCode    string
	}{
		{name: "provider refused", status: 200, body: `{"refreshed":false,"code":"provider_rejected","error":"invalid_grant: AADSTS70008"}`, wantCode: "provider_rejected"},
		{name: "no refresh token", status: 200, body: `{"refreshed":false,"code":"no_refresh_token"}`, wantCode: "no_refresh_token"},
		{name: "principal rejected", status: 403, body: `{"message":"Service principal lacks OAuth token authority"}`, wantUnavail: true},
		{name: "unauthenticated", status: 401, body: `{}`, wantUnavail: true},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
				w.WriteHeader(tc.status)
				_, _ = w.Write([]byte(tc.body))
			}))
			defer server.Close()
			client := NewAuthOAuthClient(config.Config{AuthCoreURL: server.URL, AuthCoreOAuthServiceToken: "token-token-token-token-token-token"}, server.Client())
			_, err := client.RefreshDelegatedToken(context.Background(), "ref")
			if err == nil {
				t.Fatal("expected error")
			}
			if tc.wantUnavail != errors.Is(err, oauth.ErrControlPlaneTokenSourceUnavailable) {
				t.Fatalf("unavailable=%v for err %v, want %v", !tc.wantUnavail, err, tc.wantUnavail)
			}
			var refusal *oauth.ControlPlaneRefreshError
			if tc.wantCode != "" && (!errors.As(err, &refusal) || refusal.Code != tc.wantCode) {
				t.Fatalf("err = %v, want ControlPlaneRefreshError code %q", err, tc.wantCode)
			}
		})
	}
}

func TestAuthOAuthClientIsNilWithoutCredential(t *testing.T) {
	if client := NewAuthOAuthClient(config.Config{AuthCoreURL: "http://auth-core:3011"}, nil); client != nil {
		t.Fatal("expected nil client when AUTH_CORE_OAUTH_SERVICE_TOKEN is unset")
	}
	var nilClient *AuthOAuthClient
	if _, err := nilClient.RefreshDelegatedToken(context.Background(), "ref"); !errors.Is(err, oauth.ErrControlPlaneTokenSourceUnavailable) {
		t.Fatalf("nil client err = %v, want unavailable sentinel", err)
	}
}
