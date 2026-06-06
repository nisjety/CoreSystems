package oauth

import (
	"net/url"
	"strings"
	"testing"
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
			"prompt":      "consent",
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
	if values.Get("code_challenge") == "" {
		t.Fatalf("missing code_challenge")
	}
	if values.Get("scope") != "openid email" {
		t.Fatalf("scope = %q, want openid email", values.Get("scope"))
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

func TestShopifyAuthorizationURLRequiresShopContext(t *testing.T) {
	client := NewShopifyOAuthClient("client", "secret", nil)
	_, err := client.AuthorizationURL("state", "https://app.test/callback", "", []string{"read_products"}, nil)
	if err == nil {
		t.Fatalf("expected missing shop error")
	}

	rawURL, err := client.AuthorizationURL("state", "https://app.test/callback", "", []string{"read_products", "read_content"}, map[string]string{"shop": "velion.myshopify.com"})
	if err != nil {
		t.Fatalf("AuthorizationURL error: %v", err)
	}
	if !strings.HasPrefix(rawURL, "https://velion.myshopify.com/admin/oauth/authorize?") {
		t.Fatalf("url = %s, want shop scoped auth URL", rawURL)
	}
}
