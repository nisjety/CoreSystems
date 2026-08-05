package oauth

import (
	"context"
	"encoding/json"
	"errors"
	"net/http"
	"net/http/httptest"
	"net/url"
	"strings"
	"testing"

	"github.com/triodelab/integration-corev2/internal/config"
)

func TestGoogleAuthorizationURLUsesOfflineAccessAndPKCE(t *testing.T) {
	client := NewOAuth2Client(OAuth2ClientConfig{
		ProviderKey:      "google",
		ClientID:         "client",
		AuthorizationURL: "https://accounts.google.com/o/oauth2/v2/auth",
		ScopeSeparator:   " ",
		UsePKCE:          true,
		ExtraAuthParams: map[string]string{
			"access_type": "offline",
			"prompt":      "select_account consent",
		},
	})

	rawURL, err := client.AuthorizationURL("state", "https://app.test/callback", "verifier", []string{"openid", "email"}, nil)
	if err != nil {
		t.Fatalf("AuthorizationURL error: %v", err)
	}
	parsed, err := url.Parse(rawURL)
	if err != nil {
		t.Fatalf("url parse error: %v", err)
	}
	values := parsed.Query()
	if values.Get("access_type") != "offline" {
		t.Fatalf("access_type = %q, want offline", values.Get("access_type"))
	}
	if values.Get("prompt") != "select_account consent" {
		t.Fatalf("prompt = %q, want select_account consent", values.Get("prompt"))
	}
	if values.Get("code_challenge") == "" {
		t.Fatalf("missing code_challenge")
	}
	if values.Get("scope") != "openid email" {
		t.Fatalf("scope = %q, want openid email", values.Get("scope"))
	}
}

func TestGoogleProviderClientRequestsAccountSelection(t *testing.T) {
	clients := NewProviderClients(config.Config{
		GoogleClientID:         "google-client",
		GoogleClientSecret:     "google-secret",
		GoogleAuthorizationURL: "https://accounts.google.com/o/oauth2/v2/auth",
		GoogleTokenURL:         "https://oauth2.googleapis.com/token",
		GoogleAPIBaseURL:       "https://www.googleapis.com",
	}, nil, nil)

	rawURL, err := clients["google"].AuthorizationURL(
		"state", "https://app.test/oauth/callback/google", "verifier", []string{"openid"}, nil,
	)
	if err != nil {
		t.Fatalf("AuthorizationURL error: %v", err)
	}
	parsed, err := url.Parse(rawURL)
	if err != nil {
		t.Fatalf("url parse error: %v", err)
	}
	if got := parsed.Query().Get("prompt"); got != "select_account consent" {
		t.Fatalf("provider prompt = %q, want select_account consent", got)
	}
}

func TestNotionProviderClientUsesUserOwnedAuthorization(t *testing.T) {
	clients := NewProviderClients(config.Config{
		NotionClientID:         "notion-client",
		NotionClientSecret:     "notion-secret",
		NotionAuthorizationURL: "https://api.notion.com/v1/oauth/authorize",
		NotionTokenURL:         "https://api.notion.com/v1/oauth/token",
		NotionAPIBaseURL:       "https://api.notion.com",
	}, nil, nil)

	rawURL, err := clients["notion"].AuthorizationURL(
		"state", "https://app.test/oauth/callback/notion", "", nil, nil,
	)
	if err != nil {
		t.Fatalf("AuthorizationURL error: %v", err)
	}
	parsed, err := url.Parse(rawURL)
	if err != nil {
		t.Fatalf("url parse error: %v", err)
	}
	if got := parsed.Query().Get("owner"); got != "user" {
		t.Fatalf("owner = %q, want user", got)
	}
}

func TestGoogleRefreshInvalidGrantIsClassifiedForReconnect(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		w.WriteHeader(http.StatusBadRequest)
		_ = json.NewEncoder(w).Encode(map[string]string{
			"error":             "invalid_grant",
			"error_description": "Token has been expired or revoked.",
		})
	}))
	defer server.Close()

	client := NewOAuth2Client(OAuth2ClientConfig{
		ProviderKey: "google",
		ClientID:    "client",
		TokenURL:    server.URL,
		HTTPClient:  server.Client(),
	})
	_, err := client.Refresh(t.Context(), "revoked-refresh-token", nil, nil)
	if err == nil {
		t.Fatal("expected revoked Google refresh token to fail")
	}
	if !IsAuthorizationRefreshRequired(err) {
		t.Fatalf("IsAuthorizationRefreshRequired(%v) = false, want true", err)
	}
	var tokenErr *TokenEndpointError
	if !errors.As(err, &tokenErr) {
		t.Fatalf("error = %T, want TokenEndpointError", err)
	}
	if tokenErr.ProviderKey != "google" || tokenErr.StatusCode != http.StatusBadRequest || tokenErr.Code != "invalid_grant" {
		t.Fatalf("token endpoint error = %#v, want Google invalid_grant 400", tokenErr)
	}
}

func TestSlackAuthorizationURLUsesCommaSeparatedScopes(t *testing.T) {
	client := NewOAuth2Client(OAuth2ClientConfig{
		ProviderKey:      "slack",
		ClientID:         "client",
		AuthorizationURL: "https://slack.com/oauth/v2/authorize",
		ScopeSeparator:   ",",
	})

	rawURL, err := client.AuthorizationURL("state", "https://app.test/callback", "", []string{"team:read", "channels:read"}, nil)
	if err != nil {
		t.Fatalf("AuthorizationURL error: %v", err)
	}
	if !strings.Contains(rawURL, "team%3Aread%2Cchannels%3Aread") {
		t.Fatalf("url = %s, want comma-separated encoded Slack scopes", rawURL)
	}
}

func TestSlackProviderClientSendsClientSecretInTokenBody(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.Method == http.MethodGet && r.URL.Path == "/me/permissions" {
			w.Header().Set("Content-Type", "application/json")
			_ = json.NewEncoder(w).Encode(map[string]any{"data": []map[string]string{{"permission": "pages_show_list", "status": "granted"}}})
			return
		}
		if err := r.ParseForm(); err != nil {
			t.Fatalf("ParseForm error: %v", err)
		}
		if got := r.Header.Get("Authorization"); got != "" {
			t.Fatalf("Authorization = %q, want no Basic auth for Slack token exchange", got)
		}
		if got := r.Form.Get("client_id"); got != "slack-client" {
			t.Fatalf("client_id = %q, want slack-client", got)
		}
		if got := r.Form.Get("client_secret"); got != "slack-secret" {
			t.Fatalf("client_secret = %q, want slack-secret", got)
		}
		if got := r.Form.Get("code"); got != "temporary-code" {
			t.Fatalf("code = %q, want temporary-code", got)
		}
		w.Header().Set("Content-Type", "application/json")
		_ = json.NewEncoder(w).Encode(map[string]any{
			"ok":           true,
			"access_token": "xoxb-token",
			"token_type":   "bot",
			"scope":        "team:read",
		})
	}))
	defer server.Close()

	clients := NewProviderClients(config.Config{
		SlackClientID:         "slack-client",
		SlackClientSecret:     "slack-secret",
		SlackAuthorizationURL: "https://slack.test/oauth/v2/authorize",
		SlackTokenURL:         server.URL,
		SlackAPIBaseURL:       "https://slack.test/api",
	}, nil, server.Client())

	token, err := clients["slack"].ExchangeCode(context.Background(), "temporary-code", "https://app.test/oauth/callback/slack", "", nil, nil)
	if err != nil {
		t.Fatalf("ExchangeCode error: %v", err)
	}
	if token.AccessToken != "xoxb-token" {
		t.Fatalf("AccessToken = %q, want xoxb-token", token.AccessToken)
	}
}

func TestSnapchatProviderClientUsesPKCE(t *testing.T) {
	requests := []url.Values{}
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if err := r.ParseForm(); err != nil {
			t.Fatalf("ParseForm error: %v", err)
		}
		requests = append(requests, cloneValues(r.Form))
		w.Header().Set("Content-Type", "application/json")
		_ = json.NewEncoder(w).Encode(map[string]any{
			"access_token":  "snap-token",
			"refresh_token": "snap-refresh",
			"expires_in":    3600,
			"token_type":    "Bearer",
		})
	}))
	defer server.Close()

	clients := NewProviderClients(config.Config{
		SnapchatClientID:         "snap-client",
		SnapchatClientSecret:     "snap-secret",
		SnapchatAuthorizationURL: "https://accounts.snapchat.com/login/oauth2/authorize",
		SnapchatTokenURL:         server.URL,
		SnapchatAPIBaseURL:       "https://adsapi.snapchat.com/v1",
	}, nil, server.Client())

	rawURL, err := clients["snapchat"].AuthorizationURL("state", "https://connect.example.com/oauth/callback/snapchat", "abc", []string{"snapchat-profile-api"}, nil)
	if err != nil {
		t.Fatalf("AuthorizationURL error: %v", err)
	}
	parsed, err := url.Parse(rawURL)
	if err != nil {
		t.Fatalf("url parse error: %v", err)
	}
	values := parsed.Query()
	if values.Get("code_challenge") != "ungWv48Bz-pBQUDeXa4iI7ADYaOWF3qctBD_YfIAFa0" {
		t.Fatalf("code_challenge = %q, want base64url SHA-256 challenge", values.Get("code_challenge"))
	}
	if values.Get("code_challenge_method") != "S256" {
		t.Fatalf("code_challenge_method = %q, want S256", values.Get("code_challenge_method"))
	}

	token, err := clients["snapchat"].ExchangeCode(context.Background(), "code", "https://connect.example.com/oauth/callback/snapchat", "abc", nil, nil)
	if err != nil {
		t.Fatalf("ExchangeCode error: %v", err)
	}
	if token.AccessToken != "snap-token" {
		t.Fatalf("AccessToken = %q, want snap-token", token.AccessToken)
	}
	if len(requests) != 1 {
		t.Fatalf("token endpoint calls = %d, want 1", len(requests))
	}
	if requests[0].Get("code_verifier") != "abc" {
		t.Fatalf("code_verifier = %q, want abc", requests[0].Get("code_verifier"))
	}
	if requests[0].Get("client_secret") != "snap-secret" {
		t.Fatalf("client_secret = %q, want snap-secret", requests[0].Get("client_secret"))
	}
}

func TestShopifyAuthorizationURLRequiresShopContext(t *testing.T) {
	client := NewShopifyOAuthClient("client", "secret", nil)
	_, err := client.AuthorizationURL("state", "https://app.test/callback", "", []string{"read_products"}, nil)
	if err == nil {
		t.Fatalf("expected missing shop error")
	}

	rawURL, err := client.AuthorizationURL("state", "https://app.test/callback", "", []string{"read_products", "read_content"}, map[string]string{"shop": "verevon.myshopify.com"})
	if err != nil {
		t.Fatalf("AuthorizationURL error: %v", err)
	}
	if !strings.HasPrefix(rawURL, "https://verevon.myshopify.com/admin/oauth/authorize?") {
		t.Fatalf("url = %s, want shop scoped auth URL", rawURL)
	}
}

func TestProvidersWithoutRemoteRevocationReturnTypedUnsupportedSentinel(t *testing.T) {
	clients := map[string]ProviderOAuthClient{
		"generic": NewOAuth2Client(OAuth2ClientConfig{ProviderKey: "unsupported"}),
		"notion":  NewNotionOAuthClient(OAuth2ClientConfig{ProviderKey: "notion"}),
		"shopify": NewShopifyOAuthClient("client", "secret", nil),
	}
	for name, client := range clients {
		t.Run(name, func(t *testing.T) {
			if err := client.Revoke(t.Context(), "token", nil); !errors.Is(err, ErrRevocationUnsupported) {
				t.Fatalf("Revoke error = %v, want ErrRevocationUnsupported", err)
			}
		})
	}
}

func TestMetaFamilyClientsUseTheirCorrectLoginModels(t *testing.T) {
	clients := NewProviderClients(config.Config{
		FacebookClientID:          "facebook-client",
		FacebookClientSecret:      "facebook-secret",
		FacebookAuthorizationURL:  "https://facebook.test/dialog/oauth",
		FacebookTokenURL:          "https://facebook.test/oauth/access_token",
		FacebookAPIBaseURL:        "https://graph.test",
		MetaClientID:              "meta-client",
		MetaClientSecret:          "meta-secret",
		MetaAuthorizationURL:      "https://meta.test/dialog/oauth",
		MetaTokenURL:              "https://meta.test/oauth/access_token",
		MetaAPIBaseURL:            "https://meta-graph.test",
		InstagramClientID:         "instagram-client",
		InstagramClientSecret:     "instagram-secret",
		InstagramAuthorizationURL: "https://instagram.test/dialog/oauth",
		InstagramTokenURL:         "https://instagram.test/oauth/access_token",
		InstagramAPIBaseURL:       "https://graph-instagram.test",
	}, nil, nil)

	for _, provider := range []string{"meta", "facebook", "whatsapp", "meta-ads"} {
		if _, ok := clients[provider].(*MetaOAuthClient); !ok {
			t.Fatalf("%s client = %T, want *MetaOAuthClient", provider, clients[provider])
		}
	}
	if _, ok := clients["instagram"].(*InstagramBusinessLoginClient); !ok {
		t.Fatalf("instagram client = %T, want *InstagramBusinessLoginClient", clients["instagram"])
	}
	metaClient := clients["meta"].(*MetaOAuthClient)
	if metaClient.base.cfg.ClientID != "meta-client" || metaClient.base.cfg.ClientSecret != "meta-secret" {
		t.Fatalf("unified Meta client credentials = %q/%q, want meta-client/meta-secret", metaClient.base.cfg.ClientID, metaClient.base.cfg.ClientSecret)
	}
	if metaClient.base.cfg.AuthorizationURL != "https://meta.test/dialog/oauth" || metaClient.base.cfg.TokenURL != "https://meta.test/oauth/access_token" || metaClient.base.cfg.APIBaseURL != "https://meta-graph.test" {
		t.Fatalf("unified Meta endpoints = %q/%q/%q, want dedicated Meta endpoints", metaClient.base.cfg.AuthorizationURL, metaClient.base.cfg.TokenURL, metaClient.base.cfg.APIBaseURL)
	}
}

func TestInstagramBusinessLoginUsesInstagramOAuthAndGraphEndpoints(t *testing.T) {
	var refreshRequested bool
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		switch r.URL.Path {
		case "/oauth/access_token":
			if r.Method != http.MethodPost {
				t.Fatalf("token method = %s, want POST", r.Method)
			}
			if err := r.ParseForm(); err != nil {
				t.Fatalf("parse token form: %v", err)
			}
			if r.Form.Get("client_id") != "instagram-client" || r.Form.Get("client_secret") != "instagram-secret" {
				t.Fatalf("token credentials = %q/%q", r.Form.Get("client_id"), r.Form.Get("client_secret"))
			}
			if r.Form.Get("redirect_uri") != "https://verevon.test/oauth/callback/instagram" {
				t.Fatalf("redirect_uri = %q", r.Form.Get("redirect_uri"))
			}
			_, _ = w.Write([]byte(`{"access_token":"ig-short","expires_in":3600}`))
		case "/v25.0/refresh_access_token":
			refreshRequested = true
			if r.URL.Query().Get("grant_type") != "ig_refresh_token" || r.URL.Query().Get("access_token") != "ig-short" {
				t.Fatalf("refresh query = %q", r.URL.RawQuery)
			}
			_, _ = w.Write([]byte(`{"access_token":"ig-long","expires_in":5184000}`))
		case "/v25.0/me":
			if r.Header.Get("Authorization") != "Bearer ig-long" {
				t.Fatalf("profile authorization = %q", r.Header.Get("Authorization"))
			}
			if r.URL.Query().Get("fields") != "id,username" {
				t.Fatalf("profile fields = %q", r.URL.Query().Get("fields"))
			}
			_, _ = w.Write([]byte(`{"id":"ig-user-1","username":"verevon_support"}`))
		default:
			http.NotFound(w, r)
		}
	}))
	defer server.Close()

	client := NewInstagramBusinessLoginClient(OAuth2ClientConfig{
		ProviderKey:      "instagram",
		ClientID:         "instagram-client",
		ClientSecret:     "instagram-secret",
		AuthorizationURL: "https://www.instagram.com/oauth/authorize",
		TokenURL:         server.URL + "/oauth/access_token",
		APIBaseURL:       server.URL + "/v25.0",
		ScopeSeparator:   ",",
		HTTPClient:       server.Client(),
	})

	authorizeURL, err := client.AuthorizationURL("state-1", "https://verevon.test/oauth/callback/instagram", "", []string{"instagram_business_basic", "instagram_business_manage_messages"}, nil)
	if err != nil {
		t.Fatalf("AuthorizationURL: %v", err)
	}
	parsed, err := url.Parse(authorizeURL)
	if err != nil {
		t.Fatalf("parse authorize URL: %v", err)
	}
	if parsed.Host != "www.instagram.com" || parsed.Path != "/oauth/authorize" {
		t.Fatalf("authorize endpoint = %s", parsed.String())
	}
	if parsed.Query().Get("scope") != "instagram_business_basic,instagram_business_manage_messages" {
		t.Fatalf("authorize scope = %q", parsed.Query().Get("scope"))
	}

	token, err := client.ExchangeCode(t.Context(), "code-1", "https://verevon.test/oauth/callback/instagram", "", nil, nil)
	if err != nil {
		t.Fatalf("ExchangeCode: %v", err)
	}
	if token.AccessToken != "ig-short" || token.RefreshToken != "ig-short" {
		t.Fatalf("token = %#v", token)
	}
	refreshed, err := client.Refresh(t.Context(), token.RefreshToken, nil, nil)
	if err != nil {
		t.Fatalf("Refresh: %v", err)
	}
	if !refreshRequested || refreshed.AccessToken != "ig-long" || refreshed.RefreshToken != "ig-long" {
		t.Fatalf("refreshed = %#v, requested = %v", refreshed, refreshRequested)
	}
	profile, err := client.Profile(t.Context(), refreshed.AccessToken, nil)
	if err != nil {
		t.Fatalf("Profile: %v", err)
	}
	if profile.ID != "ig-user-1" || profile.DisplayName != "verevon_support" {
		t.Fatalf("profile = %#v", profile)
	}
}

func TestMetaAuthorizationURLSelectsNamedBusinessLoginConfig(t *testing.T) {
	client := NewMetaOAuthClient(OAuth2ClientConfig{
		ProviderKey:      "meta",
		ClientID:         "client-id",
		AuthorizationURL: "https://facebook.test/dialog/oauth",
	}, map[string]string{
		"default":     "instagram-registrering-id",
		"conversions": "conversions-partner-id",
	})

	tests := []struct {
		name            string
		providerContext map[string]string
		wantConfigID    string
	}{
		{"classic requested scopes when unset", nil, ""},
		{"explicit default", map[string]string{"business_login_config": "default"}, "instagram-registrering-id"},
		{"conversions", map[string]string{"business_login_config": "conversions"}, "conversions-partner-id"},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			rawURL, err := client.AuthorizationURL("state", "https://app.test/callback", "", []string{"pages_show_list"}, tt.providerContext)
			if err != nil {
				t.Fatalf("AuthorizationURL error: %v", err)
			}
			parsed, err := url.Parse(rawURL)
			if err != nil {
				t.Fatalf("url parse error: %v", err)
			}
			if got := parsed.Query().Get("config_id"); got != tt.wantConfigID {
				t.Fatalf("config_id = %q, want %q", got, tt.wantConfigID)
			}
			if tt.wantConfigID == "" {
				if parsed.Query().Get("scope") != "pages_show_list" {
					t.Fatalf("scope = %q, want classic requested scope", parsed.Query().Get("scope"))
				}
			} else if parsed.Query().Get("override_default_response_type") != "true" {
				t.Fatal("expected override_default_response_type=true when a config_id is used")
			}
		})
	}
}

func TestMetaAuthorizationURLFallsBackToScopeWhenConfigUnknown(t *testing.T) {
	client := NewMetaOAuthClient(OAuth2ClientConfig{
		ProviderKey:      "meta",
		ClientID:         "client-id",
		AuthorizationURL: "https://facebook.test/dialog/oauth",
	}, nil)

	rawURL, err := client.AuthorizationURL("state", "https://app.test/callback", "", []string{"pages_show_list"}, nil)
	if err != nil {
		t.Fatalf("AuthorizationURL error: %v", err)
	}
	parsed, err := url.Parse(rawURL)
	if err != nil {
		t.Fatalf("url parse error: %v", err)
	}
	if parsed.Query().Get("config_id") != "" {
		t.Fatalf("config_id = %q, want empty when no Business Login configs are set", parsed.Query().Get("config_id"))
	}
	if parsed.Query().Get("scope") != "pages_show_list" {
		t.Fatalf("scope = %q, want pages_show_list", parsed.Query().Get("scope"))
	}
}

func TestMetaExchangeCodePersistsLongLivedTokenAsRefreshHandle(t *testing.T) {
	requests := []url.Values{}
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.Method == http.MethodGet && r.URL.Path == "/me/permissions" {
			w.Header().Set("Content-Type", "application/json")
			_ = json.NewEncoder(w).Encode(map[string]any{"data": []map[string]string{{"permission": "pages_show_list", "status": "granted"}}})
			return
		}
		if err := r.ParseForm(); err != nil {
			t.Fatalf("ParseForm error: %v", err)
		}
		requests = append(requests, cloneValues(r.Form))
		w.Header().Set("Content-Type", "application/json")
		switch r.Form.Get("grant_type") {
		case "authorization_code":
			_ = json.NewEncoder(w).Encode(map[string]any{
				"access_token": "short-lived-token",
				"token_type":   "bearer",
				"expires_in":   3600,
			})
		case "fb_exchange_token":
			if r.Form.Get("fb_exchange_token") != "short-lived-token" {
				t.Fatalf("fb_exchange_token = %q, want short-lived-token", r.Form.Get("fb_exchange_token"))
			}
			_ = json.NewEncoder(w).Encode(map[string]any{
				"access_token": "long-lived-token",
				"token_type":   "bearer",
				"expires_in":   5184000,
			})
		default:
			t.Fatalf("unexpected grant_type %q", r.Form.Get("grant_type"))
		}
	}))
	defer server.Close()

	client := NewMetaOAuthClient(OAuth2ClientConfig{
		ProviderKey:      "meta",
		ClientID:         "client-id",
		ClientSecret:     "client-secret",
		AuthorizationURL: "https://facebook.test/dialog/oauth",
		TokenURL:         server.URL,
		APIBaseURL:       server.URL,
		HTTPClient:       server.Client(),
	}, nil)

	token, err := client.ExchangeCode(context.Background(), "code", "https://app.test/oauth/callback/meta", "", nil, nil)
	if err != nil {
		t.Fatalf("ExchangeCode error: %v", err)
	}
	if token.AccessToken != "long-lived-token" {
		t.Fatalf("AccessToken = %q, want long-lived-token", token.AccessToken)
	}
	if token.RefreshToken != "long-lived-token" {
		t.Fatalf("RefreshToken = %q, want long-lived-token refresh handle", token.RefreshToken)
	}
	if len(requests) != 2 {
		t.Fatalf("token endpoint calls = %d, want 2", len(requests))
	}
}

func TestMetaBusinessLoginExchangeCodeKeepsReturnedToken(t *testing.T) {
	requests := []url.Values{}
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.Method == http.MethodGet && r.URL.Path == "/me/permissions" {
			w.Header().Set("Content-Type", "application/json")
			_ = json.NewEncoder(w).Encode(map[string]any{"data": []map[string]string{{"permission": "pages_show_list", "status": "granted"}}})
			return
		}
		if err := r.ParseForm(); err != nil {
			t.Fatalf("ParseForm error: %v", err)
		}
		requests = append(requests, cloneValues(r.Form))
		w.Header().Set("Content-Type", "application/json")
		if r.Form.Get("grant_type") != "authorization_code" {
			t.Fatalf("grant_type = %q, want authorization_code", r.Form.Get("grant_type"))
		}
		_ = json.NewEncoder(w).Encode(map[string]any{
			"access_token": "business-system-user-token",
			"token_type":   "bearer",
		})
	}))
	defer server.Close()

	client := NewMetaOAuthClient(OAuth2ClientConfig{
		ProviderKey:      "meta",
		ClientID:         "client-id",
		ClientSecret:     "client-secret",
		AuthorizationURL: "https://facebook.test/dialog/oauth",
		TokenURL:         server.URL,
		APIBaseURL:       server.URL,
		HTTPClient:       server.Client(),
	}, map[string]string{"default": "business-login-config-id"})

	token, err := client.ExchangeCode(context.Background(), "code", "https://app.test/oauth/callback/meta", "", nil, map[string]string{"business_login_config": "default"})
	if err != nil {
		t.Fatalf("ExchangeCode error: %v", err)
	}
	if token.AccessToken != "business-system-user-token" {
		t.Fatalf("AccessToken = %q, want business-system-user-token", token.AccessToken)
	}
	if token.RefreshToken != "business-system-user-token" {
		t.Fatalf("RefreshToken = %q, want access token persisted as refresh handle", token.RefreshToken)
	}
	if len(requests) != 1 {
		t.Fatalf("token endpoint calls = %d, want 1", len(requests))
	}
}

func TestMetaExchangeCodeReadsGrantedPermissions(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		switch r.URL.Path {
		case "/oauth/access_token":
			_ = json.NewEncoder(w).Encode(map[string]any{
				"access_token": "meta-token",
				"token_type":   "bearer",
			})
		case "/me/permissions":
			if r.Header.Get("Authorization") != "Bearer meta-token" {
				t.Fatalf("Authorization = %q", r.Header.Get("Authorization"))
			}
			_ = json.NewEncoder(w).Encode(map[string]any{"data": []map[string]string{
				{"permission": "public_profile", "status": "granted"},
				{"permission": "pages_show_list", "status": "granted"},
				{"permission": "instagram_manage_messages", "status": "declined"},
			}})
		default:
			http.NotFound(w, r)
		}
	}))
	defer server.Close()

	client := NewMetaOAuthClient(OAuth2ClientConfig{
		ProviderKey:      "meta",
		ClientID:         "client-id",
		ClientSecret:     "client-secret",
		AuthorizationURL: server.URL + "/dialog/oauth",
		TokenURL:         server.URL + "/oauth/access_token",
		APIBaseURL:       server.URL,
		HTTPClient:       server.Client(),
	}, map[string]string{"default": "business-login-config-id"})

	token, err := client.ExchangeCode(context.Background(), "code", "https://app.test/oauth/callback/meta", "", nil, map[string]string{"business_login_config": "default"})
	if err != nil {
		t.Fatalf("ExchangeCode error: %v", err)
	}
	if len(token.Scope) != 2 || token.Scope[0] != "pages_show_list" || token.Scope[1] != "public_profile" {
		t.Fatalf("granted scopes = %v, want only granted provider permissions", token.Scope)
	}
	if !token.ScopesVerified {
		t.Fatal("ScopesVerified = false, want authoritative provider grant snapshot")
	}
}

func TestMetaGrantedPermissionsRejectsOffOriginPagination(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		_ = json.NewEncoder(w).Encode(map[string]any{
			"data":   []any{},
			"paging": map[string]string{"next": "https://attacker.invalid/steal"},
		})
	}))
	defer server.Close()
	client := NewMetaOAuthClient(OAuth2ClientConfig{APIBaseURL: server.URL, HTTPClient: server.Client()}, nil)
	if _, err := client.grantedPermissions(t.Context(), "secret-token"); err == nil {
		t.Fatal("grantedPermissions error = nil, want off-origin pagination rejection")
	}
}

func TestMetaGrantedPermissionsRejectsOffOriginRedirect(t *testing.T) {
	targetCalls := 0
	target := httptest.NewServer(http.HandlerFunc(func(http.ResponseWriter, *http.Request) { targetCalls++ }))
	defer target.Close()
	source := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		w.Header().Set("Location", target.URL)
		w.WriteHeader(http.StatusFound)
	}))
	defer source.Close()
	client := NewMetaOAuthClient(OAuth2ClientConfig{APIBaseURL: source.URL, HTTPClient: source.Client()}, nil)
	if _, err := client.grantedPermissions(t.Context(), "secret-token"); err == nil {
		t.Fatal("grantedPermissions error = nil, want off-origin redirect rejection")
	}
	if targetCalls != 0 {
		t.Fatalf("off-origin redirect target calls = %d, want 0", targetCalls)
	}
}

func cloneValues(values url.Values) url.Values {
	cloned := make(url.Values, len(values))
	for key, vals := range values {
		copied := make([]string, len(vals))
		copy(copied, vals)
		cloned[key] = copied
	}
	return cloned
}

func TestTikTokAuthorizationURLUsesClientKeyAndCommaScopes(t *testing.T) {
	client := NewTikTokOAuthClient(OAuth2ClientConfig{
		ProviderKey:      "tiktok",
		ClientID:         "client-key",
		ClientSecret:     "secret",
		AuthorizationURL: "https://www.tiktok.com/v2/auth/authorize/",
		ScopeSeparator:   ",",
	})

	rawURL, err := client.AuthorizationURL("state", "https://app.test/callback", "abc", []string{"user.info.basic", "video.upload"}, nil)
	if err != nil {
		t.Fatalf("AuthorizationURL error: %v", err)
	}
	parsed, err := url.Parse(rawURL)
	if err != nil {
		t.Fatalf("url parse error: %v", err)
	}
	values := parsed.Query()
	if values.Get("client_key") != "client-key" {
		t.Fatalf("client_key = %q, want client-key", values.Get("client_key"))
	}
	if values.Get("client_id") != "" {
		t.Fatalf("client_id = %q, want empty for TikTok Login Kit v2", values.Get("client_id"))
	}
	if values.Get("scope") != "user.info.basic,video.upload" {
		t.Fatalf("scope = %q, want comma-separated TikTok scopes", values.Get("scope"))
	}
	if values.Get("code_challenge") != "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad" {
		t.Fatalf("code_challenge = %q, want TikTok hex SHA-256 challenge", values.Get("code_challenge"))
	}
	if values.Get("code_challenge_method") != "S256" {
		t.Fatalf("code_challenge_method = %q, want S256", values.Get("code_challenge_method"))
	}
}

func TestTikTokAuthorizationURLRequiresCodeVerifier(t *testing.T) {
	client := NewTikTokOAuthClient(OAuth2ClientConfig{
		ProviderKey:      "tiktok",
		ClientID:         "client-key",
		ClientSecret:     "secret",
		AuthorizationURL: "https://www.tiktok.com/v2/auth/authorize/",
		ScopeSeparator:   ",",
	})

	if _, err := client.AuthorizationURL("state", "https://app.test/callback", "", []string{"user.info.basic"}, nil); err == nil {
		t.Fatal("AuthorizationURL error = nil, want code verifier error")
	}
}

func TestTikTokExchangeCodeIncludesCodeVerifier(t *testing.T) {
	requests := []url.Values{}
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if err := r.ParseForm(); err != nil {
			t.Fatalf("ParseForm error: %v", err)
		}
		requests = append(requests, cloneValues(r.Form))
		w.Header().Set("Content-Type", "application/json")
		_ = json.NewEncoder(w).Encode(map[string]any{
			"access_token":  "token",
			"refresh_token": "refresh",
			"expires_in":    3600,
			"token_type":    "Bearer",
		})
	}))
	defer server.Close()

	client := NewTikTokOAuthClient(OAuth2ClientConfig{
		ProviderKey:  "tiktok",
		ClientID:     "client-key",
		ClientSecret: "secret",
		TokenURL:     server.URL,
	})

	token, err := client.ExchangeCode(context.Background(), "code", "https://app.test/callback", "abc", nil, nil)
	if err != nil {
		t.Fatalf("ExchangeCode error: %v", err)
	}
	if token.AccessToken != "token" {
		t.Fatalf("AccessToken = %q, want token", token.AccessToken)
	}
	if len(requests) != 1 {
		t.Fatalf("token endpoint calls = %d, want 1", len(requests))
	}
	form := requests[0]
	if form.Get("client_key") != "client-key" {
		t.Fatalf("client_key = %q, want client-key", form.Get("client_key"))
	}
	if form.Get("client_id") != "" {
		t.Fatalf("client_id = %q, want empty for TikTok Login Kit v2", form.Get("client_id"))
	}
	if form.Get("code_verifier") != "abc" {
		t.Fatalf("code_verifier = %q, want abc", form.Get("code_verifier"))
	}
	if form.Get("client_secret") != "secret" {
		t.Fatalf("client_secret = %q, want secret", form.Get("client_secret"))
	}
}

func TestGitHubProfileFallsBackToPrimaryVerifiedEmail(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		switch r.URL.Path {
		case "/user":
			_ = json.NewEncoder(w).Encode(map[string]any{"id": 123, "login": "ima", "name": "Ima", "email": nil})
		case "/user/emails":
			_ = json.NewEncoder(w).Encode([]map[string]any{
				{"email": "secondary@example.com", "primary": false, "verified": true},
				{"email": "ima@example.com", "primary": true, "verified": true},
			})
		default:
			http.NotFound(w, r)
		}
	}))
	defer server.Close()

	client := NewOAuth2Client(OAuth2ClientConfig{
		ProviderKey: "github",
		APIBaseURL:  server.URL,
		HTTPClient:  server.Client(),
	})
	profile, err := client.Profile(context.Background(), "token", nil)
	if err != nil {
		t.Fatalf("Profile error: %v", err)
	}
	if profile.Email != "ima@example.com" {
		t.Fatalf("Email = %q, want primary verified email", profile.Email)
	}
}

func TestSnapchatProfileParsesOrganizationOnSuccess(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/me/organizations" {
			t.Fatalf("path = %q, want /me/organizations", r.URL.Path)
		}
		w.Header().Set("Content-Type", "application/json")
		_ = json.NewEncoder(w).Encode(map[string]any{
			"organizations": []map[string]any{
				{"organization": map[string]any{"id": "org-1", "name": "Aquatiq AS"}},
			},
		})
	}))
	defer server.Close()

	client := NewOAuth2Client(OAuth2ClientConfig{
		ProviderKey: "snapchat",
		APIBaseURL:  server.URL,
	})

	profile, err := client.Profile(context.Background(), "marketing-scoped-token", nil)
	if err != nil {
		t.Fatalf("Profile error: %v", err)
	}
	if profile.ID != "org-1" || profile.DisplayName != "Aquatiq AS" {
		t.Fatalf("profile = %+v, want org-1/Aquatiq AS", profile)
	}
}

// A connection made with only the "onboarding" bundle (scope
// snapchat-profile-api) has no snapchat-marketing-api access, so
// /me/organizations always fails for it -- an expected outcome, not a real
// error. snapchatProfile must degrade to a named placeholder rather than an
// empty ProviderProfile{}, which would otherwise surface as the generic
// "snapchat connection" fallback name in persistConnection.
func TestSnapchatProfileDegradesGracefullyWithoutMarketingScope(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.WriteHeader(http.StatusForbidden)
		_ = json.NewEncoder(w).Encode(map[string]any{"error": "insufficient_scope"})
	}))
	defer server.Close()

	client := NewOAuth2Client(OAuth2ClientConfig{
		ProviderKey: "snapchat",
		APIBaseURL:  server.URL,
	})

	profile, err := client.Profile(context.Background(), "profile-only-token", nil)
	if err == nil {
		t.Fatal("expected an error from the 403 organizations response")
	}
	if profile.ID == "" || profile.DisplayName == "" {
		t.Fatalf("profile = %+v, want a non-empty placeholder identity even on failure", profile)
	}
}
