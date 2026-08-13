package oauth

import (
	"context"
	"net/http"
	"net/url"
	"strings"

	"github.com/triodelab/integration-corev2/internal/config"
	"github.com/triodelab/integration-corev2/internal/egress"
)

func NewProviderClients(cfg config.Config, microsoft *MicrosoftClient, httpClient *http.Client) map[string]ProviderOAuthClient {
	if httpClient == nil {
		// Several of the clients built below dial a host assembled from
		// caller-influenced data rather than a fixed config URL — most
		// notably Shopify, whose ExchangeCode/Profile calls target
		// "https://"+shop+"/..." where shop comes from the connection's
		// stored providerContext (see ShopifyShop in provider_context.go).
		// egress.SafeClient resolves, vets, and pins that host instead of
		// trusting net/http's independent second resolution; it is a
		// harmless upgrade for the remaining fixed-vendor-host clients too.
		httpClient = egress.SafeClient(egress.ClientConfig{RequestTimeout: defaultHTTPTimeout})
	}
	// Config.Load always resolves these fallbacks, but preserving them here
	// keeps explicitly constructed test and embedded configurations compatible.
	metaClientID := firstNonEmpty(cfg.MetaClientID, cfg.FacebookClientID)
	metaClientSecret := firstNonEmpty(cfg.MetaClientSecret, cfg.FacebookClientSecret)
	metaAuthorizationURL := firstNonEmpty(cfg.MetaAuthorizationURL, cfg.FacebookAuthorizationURL)
	metaTokenURL := firstNonEmpty(cfg.MetaTokenURL, cfg.FacebookTokenURL)
	metaAPIBaseURL := firstNonEmpty(cfg.MetaAPIBaseURL, cfg.FacebookAPIBaseURL)
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
				// Google documents a space-delimited combination of consent and
				// select_account. This keeps refresh-token consent while allowing
				// an operator to choose a different Google account on reconnect.
				"prompt": "select_account consent",
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
			ExtraAuthParams: map[string]string{
				// Notion's public OAuth contract requires an end-user grant.
				"owner": "user",
			},
			HTTPClient: httpClient,
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
		// Instagram API with Instagram Login has a different OAuth and Graph
		// surface from Facebook Login for Business. It must never share the
		// MetaOAuthClient's Facebook token-exchange or Graph endpoints.
		"instagram": NewInstagramBusinessLoginClient(OAuth2ClientConfig{
			ProviderKey:      "instagram",
			ClientID:         cfg.InstagramClientID,
			ClientSecret:     cfg.InstagramClientSecret,
			AuthorizationURL: cfg.InstagramAuthorizationURL,
			TokenURL:         cfg.InstagramTokenURL,
			APIBaseURL:       cfg.InstagramAPIBaseURL,
			ScopeSeparator:   ",",
			HTTPClient:       httpClient,
		}),
		"facebook": NewMetaOAuthClient(OAuth2ClientConfig{
			ProviderKey:      "facebook",
			ClientID:         cfg.FacebookClientID,
			ClientSecret:     cfg.FacebookClientSecret,
			AuthorizationURL: cfg.FacebookAuthorizationURL,
			TokenURL:         cfg.FacebookTokenURL,
			APIBaseURL:       cfg.FacebookAPIBaseURL,
			ScopeSeparator:   ",",
			HTTPClient:       httpClient,
		}, nil),
		"whatsapp": NewMetaOAuthClient(OAuth2ClientConfig{
			ProviderKey:      "whatsapp",
			ClientID:         cfg.FacebookClientID,
			ClientSecret:     cfg.FacebookClientSecret,
			AuthorizationURL: cfg.FacebookAuthorizationURL,
			TokenURL:         cfg.FacebookTokenURL,
			APIBaseURL:       cfg.FacebookAPIBaseURL,
			ScopeSeparator:   ",",
			HTTPClient:       httpClient,
		}, nil),
		"meta-ads": NewMetaOAuthClient(OAuth2ClientConfig{
			ProviderKey:      "meta-ads",
			ClientID:         cfg.FacebookClientID,
			ClientSecret:     cfg.FacebookClientSecret,
			AuthorizationURL: cfg.FacebookAuthorizationURL,
			TokenURL:         cfg.FacebookTokenURL,
			APIBaseURL:       cfg.FacebookAPIBaseURL,
			ScopeSeparator:   ",",
			HTTPClient:       httpClient,
		}, nil),
		"snapchat": NewOAuth2Client(OAuth2ClientConfig{
			ProviderKey:      "snapchat",
			ClientID:         cfg.SnapchatClientID,
			ClientSecret:     cfg.SnapchatClientSecret,
			AuthorizationURL: cfg.SnapchatAuthorizationURL,
			TokenURL:         cfg.SnapchatTokenURL,
			APIBaseURL:       cfg.SnapchatAPIBaseURL,
			ScopeSeparator:   " ",
			UsePKCE:          true,
			HTTPClient:       httpClient,
		}),
		// Unified Meta provider: ONE OAuth dialog covering Facebook Pages,
		// Instagram (Facebook-Login flavor), WhatsApp Business, and Marketing
		// API. Uses Facebook Login for Business (config_id) when a named
		// META_BUSINESS_LOGIN_*_CONFIG_ID is set for the requested purpose
		// (providerContext["business_login_config"], "default" otherwise);
		// falls back to classic comma-separated scopes if none are configured
		// at all (works in dev mode for app-role users on a Business-type app).
		"meta": NewMetaOAuthClient(OAuth2ClientConfig{
			ProviderKey:      "meta",
			ClientID:         metaClientID,
			ClientSecret:     metaClientSecret,
			AuthorizationURL: metaAuthorizationURL,
			TokenURL:         metaTokenURL,
			APIBaseURL:       metaAPIBaseURL,
			ScopeSeparator:   ",",
			HTTPClient:       httpClient,
		}, cfg.MetaBusinessLoginConfigIDs),
		// TikTok Login Kit v2: TikTok uses `client_key` (NOT client_id) in both
		// the authorize query and the token body — the generic client cannot
		// express that, hence the dedicated client type.
		"tiktok": NewTikTokOAuthClient(OAuth2ClientConfig{
			ProviderKey:      "tiktok",
			ClientID:         cfg.TikTokClientKey,
			ClientSecret:     cfg.TikTokClientSecret,
			AuthorizationURL: cfg.TikTokAuthorizationURL,
			TokenURL:         cfg.TikTokTokenURL,
			APIBaseURL:       cfg.TikTokAPIBaseURL,
			ScopeSeparator:   ",",
			HTTPClient:       httpClient,
		}),
		// Discord: standard authorization-code flow; token endpoint accepts the
		// secret in the form body (JSON bodies are rejected by Discord).
		"discord": NewOAuth2Client(OAuth2ClientConfig{
			ProviderKey:      "discord",
			ClientID:         cfg.DiscordClientID,
			ClientSecret:     cfg.DiscordClientSecret,
			AuthorizationURL: cfg.DiscordAuthorizationURL,
			TokenURL:         cfg.DiscordTokenURL,
			APIBaseURL:       cfg.DiscordAPIBaseURL,
			ScopeSeparator:   " ",
			// When the requested capabilities include messages.read the scope set
			// carries "bot" (guild bot install). Discord reads the granted bot
			// permissions from this bitfield: VIEW_CHANNEL (1024) +
			// READ_MESSAGE_HISTORY (65536) + SEND_MESSAGES (2048) = 68608 — the
			// minimum for inbox ingestion plus HITL-approved replies. Discord
			// ignores the parameter for non-bot scope sets, so it is safe to send
			// unconditionally.
			ExtraAuthParams: map[string]string{
				"permissions": "68608",
			},
			HTTPClient: httpClient,
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
	return ErrRevocationUnsupported
}
