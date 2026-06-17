package oauth

import (
	"context"
	"net/http"
	"net/url"
	"strings"

	"github.com/triodelab/integration-corev2/internal/config"
)

func NewProviderClients(cfg config.Config, microsoft *MicrosoftClient, httpClient *http.Client) map[string]ProviderOAuthClient {
	if httpClient == nil {
		httpClient = &http.Client{Timeout: defaultHTTPTimeout}
	}
	return map[string]ProviderOAuthClient{
		"microsoft": &MicrosoftProviderClient{base: microsoft},
		"slack": NewOAuth2Client(OAuth2ClientConfig{
			ProviderKey:      "slack",
			ClientID:         cfg.SlackClientID,
			ClientSecret:     cfg.SlackClientSecret,
			AuthorizationURL: cfg.SlackAuthorizationURL,
			TokenURL:         cfg.SlackTokenURL,
			APIBaseURL:       cfg.SlackAPIBaseURL,
			ScopeSeparator:   ",",
			UseBasicAuth:     true,
			HTTPClient:       httpClient,
		}),
		"google": NewOAuth2Client(OAuth2ClientConfig{
			ProviderKey:      "google",
			ClientID:         cfg.GoogleClientID,
			ClientSecret:     cfg.GoogleClientSecret,
			AuthorizationURL: cfg.GoogleAuthorizationURL,
			TokenURL:         cfg.GoogleTokenURL,
			APIBaseURL:       cfg.GoogleAPIBaseURL,
			ScopeSeparator:   " ",
			UsePKCE:          true,
			ExtraAuthParams: map[string]string{
				"access_type": "offline",
				"prompt":      "consent",
			},
			HTTPClient: httpClient,
		}),
		"notion": NewNotionOAuthClient(OAuth2ClientConfig{
			ProviderKey:      "notion",
			ClientID:         cfg.NotionClientID,
			ClientSecret:     cfg.NotionClientSecret,
			AuthorizationURL: cfg.NotionAuthorizationURL,
			TokenURL:         cfg.NotionTokenURL,
			APIBaseURL:       cfg.NotionAPIBaseURL,
			ScopeSeparator:   " ",
			HTTPClient:       httpClient,
		}),
		"github": NewOAuth2Client(OAuth2ClientConfig{
			ProviderKey:      "github",
			ClientID:         cfg.GitHubClientID,
			ClientSecret:     cfg.GitHubClientSecret,
			AuthorizationURL: cfg.GitHubAuthorizationURL,
			TokenURL:         cfg.GitHubTokenURL,
			APIBaseURL:       cfg.GitHubAPIBaseURL,
			ScopeSeparator:   " ",
			HTTPClient:       httpClient,
		}),
		"shopify": NewShopifyOAuthClient(cfg.ShopifyClientID, cfg.ShopifyClientSecret, httpClient),
		"stripe": NewOAuth2Client(OAuth2ClientConfig{
			ProviderKey:      "stripe",
			ClientID:         cfg.StripeClientID,
			ClientSecret:     cfg.StripeClientSecret,
			AuthorizationURL: cfg.StripeAuthorizationURL,
			TokenURL:         cfg.StripeTokenURL,
			APIBaseURL:       cfg.StripeAPIBaseURL,
			ScopeSeparator:   " ",
			HTTPClient:       httpClient,
		}),
		"linkedin": NewOAuth2Client(OAuth2ClientConfig{
			ProviderKey:      "linkedin",
			ClientID:         cfg.LinkedInClientID,
			ClientSecret:     cfg.LinkedInClientSecret,
			AuthorizationURL: cfg.LinkedInAuthorizationURL,
			TokenURL:         cfg.LinkedInTokenURL,
			APIBaseURL:       cfg.LinkedInAPIBaseURL,
			ScopeSeparator:   " ",
			HTTPClient:       httpClient,
		}),
		"x": NewOAuth2Client(OAuth2ClientConfig{
			ProviderKey:      "x",
			ClientID:         cfg.XClientID,
			ClientSecret:     cfg.XClientSecret,
			AuthorizationURL: cfg.XAuthorizationURL,
			TokenURL:         cfg.XTokenURL,
			APIBaseURL:       cfg.XAPIBaseURL,
			ScopeSeparator:   " ",
			UsePKCE:          true,
			UseBasicAuth:     true,
			HTTPClient:       httpClient,
		}),
		"instagram": NewOAuth2Client(OAuth2ClientConfig{
			ProviderKey:      "instagram",
			ClientID:         cfg.InstagramClientID,
			ClientSecret:     cfg.InstagramClientSecret,
			AuthorizationURL: cfg.InstagramAuthorizationURL,
			TokenURL:         cfg.InstagramTokenURL,
			APIBaseURL:       cfg.InstagramAPIBaseURL,
			ScopeSeparator:   ",",
			HTTPClient:       httpClient,
		}),
		"facebook": NewOAuth2Client(OAuth2ClientConfig{
			ProviderKey:      "facebook",
			ClientID:         cfg.FacebookClientID,
			ClientSecret:     cfg.FacebookClientSecret,
			AuthorizationURL: cfg.FacebookAuthorizationURL,
			TokenURL:         cfg.FacebookTokenURL,
			APIBaseURL:       cfg.FacebookAPIBaseURL,
			ScopeSeparator:   ",",
			HTTPClient:       httpClient,
		}),
		"snapchat": NewOAuth2Client(OAuth2ClientConfig{
			ProviderKey:      "snapchat",
			ClientID:         cfg.SnapchatClientID,
			ClientSecret:     cfg.SnapchatClientSecret,
			AuthorizationURL: cfg.SnapchatAuthorizationURL,
			TokenURL:         cfg.SnapchatTokenURL,
			APIBaseURL:       cfg.SnapchatAPIBaseURL,
			ScopeSeparator:   " ",
			HTTPClient:       httpClient,
		}),
	}
}

type MicrosoftProviderClient struct {
	base *MicrosoftClient
}

func (c *MicrosoftProviderClient) AuthorizationURL(state, redirectURI, codeVerifier string, scopes []string, _ map[string]string) (string, error) {
	values := url.Values{}
	values.Set("client_id", c.base.cfg.ClientID)
	values.Set("response_type", "code")
	values.Set("redirect_uri", redirectURI)
	values.Set("response_mode", "query")
	values.Set("scope", strings.Join(scopes, " "))
	values.Set("state", state)
	values.Set("code_challenge", CodeChallengeS256(codeVerifier))
	values.Set("code_challenge_method", "S256")
	values.Set("prompt", "select_account")
	return c.base.cfg.AuthorizationURL + "?" + values.Encode(), nil
}

func (c *MicrosoftProviderClient) ExchangeCode(ctx context.Context, code, redirectURI, codeVerifier string, scopes []string, _ map[string]string) (TokenResult, error) {
	return c.base.ExchangeCode(ctx, code, redirectURI, codeVerifier, scopes)
}

func (c *MicrosoftProviderClient) Refresh(ctx context.Context, refreshToken string, scopes []string, _ map[string]string) (TokenResult, error) {
	return c.base.Refresh(ctx, refreshToken, scopes)
}

func (c *MicrosoftProviderClient) Profile(ctx context.Context, accessToken string, _ map[string]string) (ProviderProfile, error) {
	profile, err := c.base.Profile(ctx, accessToken)
	if err != nil {
		return ProviderProfile{}, err
	}
	return ProviderProfile{
		ID:          profile.ID,
		DisplayName: profile.DisplayName,
		Email:       firstNonEmpty(profile.Mail, profile.UserPrincipalName),
		TenantID:    profile.TenantID,
	}, nil
}

func (c *MicrosoftProviderClient) Revoke(_ context.Context, _ string, _ map[string]string) error {
	return nil
}
