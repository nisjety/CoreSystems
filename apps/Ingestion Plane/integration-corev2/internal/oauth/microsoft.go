package oauth

import (
	"context"
	"encoding/json"
	"fmt"
	"net/http"
	"net/url"
	"strconv"
	"strings"
	"time"
)

type MicrosoftClientConfig struct {
	ClientID         string
	ClientSecret     string
	ClientAuthMode   string
	TokenOrigin      string
	AuthorizationURL string
	TokenURL         string
	GraphBaseURL     string
	HTTPClient       *http.Client
}

type TokenResult struct {
	AccessToken  string
	RefreshToken string
	TokenType    string
	Scope        []string
	ExpiresAt    time.Time
	Raw          map[string]any
}

type MicrosoftProfile struct {
	ID                string
	DisplayName       string
	Mail              string
	UserPrincipalName string
	TenantID          string
}

type MicrosoftClient struct {
	cfg        MicrosoftClientConfig
	httpClient *http.Client
}

func NewMicrosoftClient(cfg MicrosoftClientConfig) *MicrosoftClient {
	client := cfg.HTTPClient
	if client == nil {
		client = &http.Client{Timeout: 15 * time.Second}
	}
	if cfg.GraphBaseURL == "" {
		cfg.GraphBaseURL = "https://graph.microsoft.com"
	}
	if cfg.AuthorizationURL == "" {
		cfg.AuthorizationURL = "https://login.microsoftonline.com/common/oauth2/v2.0/authorize"
	}
	return &MicrosoftClient{cfg: cfg, httpClient: client}
}

func (c *MicrosoftClient) ExchangeCode(ctx context.Context, code, redirectURI, codeVerifier string, scopes []string) (TokenResult, error) {
	values := url.Values{}
	values.Set("client_id", c.cfg.ClientID)
	c.setClientSecret(values)
	values.Set("grant_type", "authorization_code")
	values.Set("code", code)
	values.Set("redirect_uri", redirectURI)
	values.Set("code_verifier", codeVerifier)
	if len(scopes) > 0 {
		values.Set("scope", strings.Join(scopes, " "))
	}
	return c.tokenRequest(ctx, values)
}

func (c *MicrosoftClient) Refresh(ctx context.Context, refreshToken string, scopes []string) (TokenResult, error) {
	values := url.Values{}
	values.Set("client_id", c.cfg.ClientID)
	c.setClientSecret(values)
	values.Set("grant_type", "refresh_token")
	values.Set("refresh_token", refreshToken)
	if len(scopes) > 0 {
		values.Set("scope", strings.Join(scopes, " "))
	}
	return c.tokenRequest(ctx, values)
}

func (c *MicrosoftClient) setClientSecret(values url.Values) {
	if microsoftUsesClientSecret(c.cfg.ClientAuthMode) && strings.TrimSpace(c.cfg.ClientSecret) != "" {
		values.Set("client_secret", c.cfg.ClientSecret)
	}
}

func microsoftUsesClientSecret(mode string) bool {
	switch strings.ToLower(strings.TrimSpace(mode)) {
	case "confidential", "secret", "web":
		return true
	default:
		return false
	}
}

func (c *MicrosoftClient) Profile(ctx context.Context, accessToken string) (MicrosoftProfile, error) {
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, strings.TrimRight(c.cfg.GraphBaseURL, "/")+"/v1.0/me?$select=id,displayName,mail,userPrincipalName", nil)
	if err != nil {
		return MicrosoftProfile{}, fmt.Errorf("build profile request: %w", err)
	}
	req.Header.Set("Authorization", "Bearer "+accessToken)
	req.Header.Set("Accept", "application/json")

	resp, err := c.httpClient.Do(req)
	if err != nil {
		return MicrosoftProfile{}, fmt.Errorf("call Microsoft profile: %w", err)
	}
	defer resp.Body.Close()
	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		return MicrosoftProfile{}, fmt.Errorf("Microsoft profile returned status %d", resp.StatusCode)
	}
	var body struct {
		ID                string `json:"id"`
		DisplayName       string `json:"displayName"`
		Mail              string `json:"mail"`
		UserPrincipalName string `json:"userPrincipalName"`
	}
	if err := json.NewDecoder(resp.Body).Decode(&body); err != nil {
		return MicrosoftProfile{}, fmt.Errorf("decode Microsoft profile: %w", err)
	}
	return MicrosoftProfile{
		ID:                body.ID,
		DisplayName:       body.DisplayName,
		Mail:              body.Mail,
		UserPrincipalName: body.UserPrincipalName,
	}, nil
}

func (c *MicrosoftClient) tokenRequest(ctx context.Context, values url.Values) (TokenResult, error) {
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, c.cfg.TokenURL, strings.NewReader(values.Encode()))
	if err != nil {
		return TokenResult{}, fmt.Errorf("build token request: %w", err)
	}
	req.Header.Set("Content-Type", "application/x-www-form-urlencoded")
	req.Header.Set("Accept", "application/json")
	c.setTokenOrigin(req, values)

	resp, err := c.httpClient.Do(req)
	if err != nil {
		return TokenResult{}, fmt.Errorf("call token endpoint: %w", err)
	}
	defer resp.Body.Close()

	var raw map[string]any
	if err := json.NewDecoder(resp.Body).Decode(&raw); err != nil {
		return TokenResult{}, fmt.Errorf("decode token response: %w", err)
	}
	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		return TokenResult{}, fmt.Errorf("token endpoint returned status %d: %s", resp.StatusCode, tokenError(raw))
	}
	accessToken, _ := raw["access_token"].(string)
	if accessToken == "" {
		return TokenResult{}, fmt.Errorf("token endpoint returned empty access_token")
	}
	refreshToken, _ := raw["refresh_token"].(string)
	scopeValue, _ := raw["scope"].(string)
	expiresIn := expiresInSeconds(raw["expires_in"])
	if expiresIn <= 0 {
		expiresIn = 3600
	}
	return TokenResult{
		AccessToken:  accessToken,
		RefreshToken: refreshToken,
		TokenType:    stringValue(raw["token_type"]),
		Scope:        strings.Fields(scopeValue),
		ExpiresAt:    time.Now().UTC().Add(time.Duration(expiresIn) * time.Second),
		Raw:          raw,
	}, nil
}

func (c *MicrosoftClient) setTokenOrigin(req *http.Request, values url.Values) {
	if microsoftUsesClientSecret(c.cfg.ClientAuthMode) {
		return
	}
	origin := strings.TrimRight(strings.TrimSpace(c.cfg.TokenOrigin), "/")
	if origin == "" {
		origin = requestOrigin(values.Get("redirect_uri"))
	}
	if origin != "" {
		req.Header.Set("Origin", origin)
	}
}

func requestOrigin(rawURL string) string {
	parsed, err := url.Parse(strings.TrimSpace(rawURL))
	if err != nil || parsed.Scheme == "" || parsed.Host == "" {
		return ""
	}
	return parsed.Scheme + "://" + parsed.Host
}

func tokenError(raw map[string]any) string {
	message := stringValue(raw["error_description"])
	if message == "" {
		message = stringValue(raw["error"])
	}
	if message == "" {
		message = "unknown token error"
	}
	return message
}

func expiresInSeconds(value any) int64 {
	switch v := value.(type) {
	case float64:
		return int64(v)
	case string:
		parsed, _ := strconv.ParseInt(v, 10, 64)
		return parsed
	default:
		return 0
	}
}

func stringValue(value any) string {
	if s, ok := value.(string); ok {
		return s
	}
	return ""
}
