package oauth

import (
	"context"
	"fmt"
	"net/http"
	"net/url"
	"strings"
)

// InstagramBusinessLoginClient implements Instagram API with Instagram Login.
// It is intentionally separate from MetaOAuthClient: that client speaks
// Facebook Login for Business (graph.facebook.com and fb_exchange_token),
// while Instagram Login uses Instagram OAuth and graph.instagram.com tokens.
type InstagramBusinessLoginClient struct {
	base *OAuth2Client
}

func NewInstagramBusinessLoginClient(cfg OAuth2ClientConfig) *InstagramBusinessLoginClient {
	return &InstagramBusinessLoginClient{base: NewOAuth2Client(cfg)}
}

func (c *InstagramBusinessLoginClient) AuthorizationURL(state, redirectURI, codeVerifier string, scopes []string, providerContext map[string]string) (string, error) {
	return c.base.AuthorizationURL(state, redirectURI, codeVerifier, scopes, providerContext)
}

func (c *InstagramBusinessLoginClient) ExchangeCode(ctx context.Context, code, redirectURI, codeVerifier string, scopes []string, providerContext map[string]string) (TokenResult, error) {
	token, err := c.base.ExchangeCode(ctx, code, redirectURI, codeVerifier, scopes, providerContext)
	if err != nil {
		return TokenResult{}, err
	}
	// Instagram returns an access token but no refresh-token field. The access
	// token itself is the renewal handle for refresh_access_token.
	if token.RefreshToken == "" {
		token.RefreshToken = token.AccessToken
	}
	return token, nil
}

func (c *InstagramBusinessLoginClient) Refresh(ctx context.Context, refreshToken string, _ []string, _ map[string]string) (TokenResult, error) {
	refreshToken = strings.TrimSpace(refreshToken)
	if refreshToken == "" {
		return TokenResult{}, fmt.Errorf("instagram refresh token is required")
	}
	endpoint, err := url.Parse(strings.TrimRight(c.base.cfg.APIBaseURL, "/") + "/refresh_access_token")
	if err != nil {
		return TokenResult{}, fmt.Errorf("build Instagram refresh URL: %w", err)
	}
	query := endpoint.Query()
	query.Set("grant_type", "ig_refresh_token")
	query.Set("access_token", refreshToken)
	endpoint.RawQuery = query.Encode()
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, endpoint.String(), nil)
	if err != nil {
		return TokenResult{}, fmt.Errorf("build Instagram refresh request: %w", err)
	}
	req.Header.Set("Accept", "application/json")
	token, err := doTokenRequest(c.base.httpClient, req, "instagram")
	if err != nil {
		return TokenResult{}, err
	}
	if token.RefreshToken == "" {
		token.RefreshToken = token.AccessToken
	}
	return token, nil
}

func (c *InstagramBusinessLoginClient) Profile(ctx context.Context, accessToken string, _ map[string]string) (ProviderProfile, error) {
	body, err := c.base.getJSON(ctx, accessToken, strings.TrimRight(c.base.cfg.APIBaseURL, "/")+"/me?fields=id,username", nil)
	if err != nil {
		return ProviderProfile{}, err
	}
	id := stringValue(body["id"])
	return ProviderProfile{
		ID:          id,
		DisplayName: firstNonEmpty(stringValue(body["username"]), id),
	}, nil
}

func (c *InstagramBusinessLoginClient) Revoke(_ context.Context, _ string, _ map[string]string) error {
	return ErrRevocationUnsupported
}
