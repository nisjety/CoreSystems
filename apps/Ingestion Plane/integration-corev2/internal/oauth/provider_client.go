package oauth

import (
	"context"
	"encoding/base64"
	"encoding/json"
	"fmt"
	"net/http"
	"net/url"
	"strings"
	"time"
)

type ProviderProfile struct {
	ID            string
	DisplayName   string
	Email         string
	TenantID      string
	WorkspaceID   string
	WorkspaceName string
}

type ProviderOAuthClient interface {
	AuthorizationURL(state, redirectURI, codeVerifier string, scopes []string, providerContext map[string]string) (string, error)
	ExchangeCode(ctx context.Context, code, redirectURI, codeVerifier string, scopes []string, providerContext map[string]string) (TokenResult, error)
	Refresh(ctx context.Context, refreshToken string, scopes []string, providerContext map[string]string) (TokenResult, error)
	Profile(ctx context.Context, accessToken string, providerContext map[string]string) (ProviderProfile, error)
	Revoke(ctx context.Context, accessToken string, providerContext map[string]string) error
}

const defaultHTTPTimeout = 15 * time.Second

type OAuth2ClientConfig struct {
	ProviderKey      string
	ClientID         string
	ClientSecret     string
	AuthorizationURL string
	TokenURL         string
	APIBaseURL       string
	ScopeSeparator   string
	UsePKCE          bool
	UseBasicAuth     bool
	ExtraAuthParams  map[string]string
	HTTPClient       *http.Client
}

type OAuth2Client struct {
	cfg        OAuth2ClientConfig
	httpClient *http.Client
}

func NewOAuth2Client(cfg OAuth2ClientConfig) *OAuth2Client {
	client := cfg.HTTPClient
	if client == nil {
		client = &http.Client{Timeout: defaultHTTPTimeout}
	}
	if cfg.ScopeSeparator == "" {
		cfg.ScopeSeparator = " "
	}
	return &OAuth2Client{cfg: cfg, httpClient: client}
}

func (c *OAuth2Client) AuthorizationURL(state, redirectURI, codeVerifier string, scopes []string, _ map[string]string) (string, error) {
	values := url.Values{}
	values.Set("client_id", c.cfg.ClientID)
	values.Set("redirect_uri", redirectURI)
	values.Set("response_type", "code")
	values.Set("state", state)
	if len(scopes) > 0 {
		values.Set("scope", strings.Join(scopes, c.cfg.ScopeSeparator))
	}
	if c.cfg.UsePKCE {
		values.Set("code_challenge", CodeChallengeS256(codeVerifier))
		values.Set("code_challenge_method", "S256")
	}
	for key, value := range c.cfg.ExtraAuthParams {
		values.Set(key, value)
	}
	return c.cfg.AuthorizationURL + "?" + values.Encode(), nil
}

func (c *OAuth2Client) ExchangeCode(ctx context.Context, code, redirectURI, codeVerifier string, _ []string, _ map[string]string) (TokenResult, error) {
	values := url.Values{}
	values.Set("client_id", c.cfg.ClientID)
	if !c.cfg.UseBasicAuth {
		values.Set("client_secret", c.cfg.ClientSecret)
	}
	values.Set("grant_type", "authorization_code")
	values.Set("code", code)
	values.Set("redirect_uri", redirectURI)
	if c.cfg.UsePKCE {
		values.Set("code_verifier", codeVerifier)
	}
	return c.tokenRequest(ctx, values)
}

func (c *OAuth2Client) Refresh(ctx context.Context, refreshToken string, _ []string, _ map[string]string) (TokenResult, error) {
	values := url.Values{}
	values.Set("client_id", c.cfg.ClientID)
	if !c.cfg.UseBasicAuth {
		values.Set("client_secret", c.cfg.ClientSecret)
	}
	values.Set("grant_type", "refresh_token")
	values.Set("refresh_token", refreshToken)
	return c.tokenRequest(ctx, values)
}

func (c *OAuth2Client) Revoke(ctx context.Context, accessToken string, _ map[string]string) error {
	switch c.cfg.ProviderKey {
	case "google":
		values := url.Values{}
		values.Set("token", accessToken)
		req, err := http.NewRequestWithContext(ctx, http.MethodPost, "https://oauth2.googleapis.com/revoke", strings.NewReader(values.Encode()))
		if err != nil {
			return fmt.Errorf("build google revoke request: %w", err)
		}
		req.Header.Set("Content-Type", "application/x-www-form-urlencoded")
		return c.doRevoke(req)
	case "slack":
		values := url.Values{}
		values.Set("token", accessToken)
		req, err := http.NewRequestWithContext(ctx, http.MethodPost, strings.TrimRight(c.cfg.APIBaseURL, "/")+"/auth.revoke", strings.NewReader(values.Encode()))
		if err != nil {
			return fmt.Errorf("build slack revoke request: %w", err)
		}
		req.Header.Set("Content-Type", "application/x-www-form-urlencoded")
		req.Header.Set("Authorization", "Bearer "+accessToken)
		return c.doRevoke(req)
	case "github":
		payload, err := json.Marshal(map[string]string{"access_token": accessToken})
		if err != nil {
			return fmt.Errorf("marshal github revoke request: %w", err)
		}
		req, err := http.NewRequestWithContext(ctx, http.MethodDelete, strings.TrimRight(c.cfg.APIBaseURL, "/")+"/applications/"+url.PathEscape(c.cfg.ClientID)+"/token", strings.NewReader(string(payload)))
		if err != nil {
			return fmt.Errorf("build github revoke request: %w", err)
		}
		req.SetBasicAuth(c.cfg.ClientID, c.cfg.ClientSecret)
		req.Header.Set("Accept", "application/vnd.github+json")
		req.Header.Set("Content-Type", "application/json")
		return c.doRevoke(req)
	default:
		return nil
	}
}

func (c *OAuth2Client) Profile(ctx context.Context, accessToken string, _ map[string]string) (ProviderProfile, error) {
	switch c.cfg.ProviderKey {
	case "slack":
		return c.slackProfile(ctx, accessToken)
	case "google":
		return c.googleProfile(ctx, accessToken)
	case "notion":
		return c.notionProfile(ctx, accessToken)
	case "github":
		return c.githubProfile(ctx, accessToken)
	case "stripe":
		return c.stripeProfile(ctx, accessToken)
	case "linkedin":
		return c.linkedinProfile(ctx, accessToken)
	case "x":
		return c.xProfile(ctx, accessToken)
	case "instagram":
		return c.instagramProfile(ctx, accessToken)
	case "facebook":
		return c.facebookProfile(ctx, accessToken)
	case "snapchat":
		return c.snapchatProfile(ctx, accessToken)
	default:
		return ProviderProfile{}, fmt.Errorf("profile discovery is not implemented for %s", c.cfg.ProviderKey)
	}
}

func (c *OAuth2Client) doRevoke(req *http.Request) error {
	resp, err := c.httpClient.Do(req)
	if err != nil {
		return fmt.Errorf("call %s revoke endpoint: %w", c.cfg.ProviderKey, err)
	}
	defer resp.Body.Close()
	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		return fmt.Errorf("%s revoke endpoint returned status %d", c.cfg.ProviderKey, resp.StatusCode)
	}
	return nil
}

func (c *OAuth2Client) tokenRequest(ctx context.Context, values url.Values) (TokenResult, error) {
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, c.cfg.TokenURL, strings.NewReader(values.Encode()))
	if err != nil {
		return TokenResult{}, fmt.Errorf("build %s token request: %w", c.cfg.ProviderKey, err)
	}
	req.Header.Set("Content-Type", "application/x-www-form-urlencoded")
	req.Header.Set("Accept", "application/json")
	if c.cfg.UseBasicAuth {
		req.SetBasicAuth(c.cfg.ClientID, c.cfg.ClientSecret)
	}
	return doTokenRequest(c.httpClient, req, c.cfg.ProviderKey)
}

func (c *OAuth2Client) slackProfile(ctx context.Context, accessToken string) (ProviderProfile, error) {
	authTest, err := c.getJSON(ctx, accessToken, strings.TrimRight(c.cfg.APIBaseURL, "/")+"/auth.test", nil)
	if err != nil {
		return ProviderProfile{}, err
	}
	if ok, _ := authTest["ok"].(bool); !ok {
		return ProviderProfile{}, fmt.Errorf("Slack auth.test failed: %s", stringValue(authTest["error"]))
	}
	profile := ProviderProfile{
		ID:            stringValue(authTest["user_id"]),
		DisplayName:   stringValue(authTest["user"]),
		WorkspaceID:   stringValue(authTest["team_id"]),
		WorkspaceName: stringValue(authTest["team"]),
		TenantID:      stringValue(authTest["team_id"]),
	}
	teamInfo, err := c.getJSON(ctx, accessToken, strings.TrimRight(c.cfg.APIBaseURL, "/")+"/team.info", nil)
	if err == nil {
		if team, ok := teamInfo["team"].(map[string]any); ok {
			if name := stringValue(team["name"]); name != "" {
				profile.WorkspaceName = name
			}
		}
	}
	return profile, nil
}

func (c *OAuth2Client) googleProfile(ctx context.Context, accessToken string) (ProviderProfile, error) {
	body, err := c.getJSON(ctx, accessToken, strings.TrimRight(c.cfg.APIBaseURL, "/")+"/oauth2/v3/userinfo", nil)
	if err != nil {
		return ProviderProfile{}, err
	}
	return ProviderProfile{
		ID:          stringValue(body["sub"]),
		DisplayName: stringValue(body["name"]),
		Email:       stringValue(body["email"]),
	}, nil
}

func (c *OAuth2Client) notionProfile(ctx context.Context, accessToken string) (ProviderProfile, error) {
	body, err := c.getJSON(ctx, accessToken, strings.TrimRight(c.cfg.APIBaseURL, "/")+"/v1/users/me", map[string]string{
		"Notion-Version": "2022-06-28",
	})
	if err != nil {
		return ProviderProfile{}, err
	}
	name := stringValue(body["name"])
	if bot, ok := body["bot"].(map[string]any); ok {
		if owner, ok := bot["owner"].(map[string]any); ok {
			if workspace, ok := owner["workspace"].(bool); ok && workspace && name == "" {
				name = "Notion workspace"
			}
		}
	}
	return ProviderProfile{
		ID:          stringValue(body["id"]),
		DisplayName: name,
	}, nil
}

func (c *OAuth2Client) githubProfile(ctx context.Context, accessToken string) (ProviderProfile, error) {
	body, err := c.getJSON(ctx, accessToken, strings.TrimRight(c.cfg.APIBaseURL, "/")+"/user", map[string]string{
		"Accept":               "application/vnd.github+json",
		"X-GitHub-Api-Version": "2022-11-28",
	})
	if err != nil {
		return ProviderProfile{}, err
	}
	return ProviderProfile{
		ID:          fmt.Sprintf("%v", body["id"]),
		DisplayName: firstNonEmpty(stringValue(body["name"]), stringValue(body["login"])),
		Email:       stringValue(body["email"]),
	}, nil
}

func (c *OAuth2Client) stripeProfile(ctx context.Context, accessToken string) (ProviderProfile, error) {
	body, err := c.getJSON(ctx, accessToken, strings.TrimRight(c.cfg.APIBaseURL, "/")+"/v1/account", nil)
	if err != nil {
		return ProviderProfile{}, err
	}
	businessProfile, _ := body["business_profile"].(map[string]any)
	return ProviderProfile{
		ID:            stringValue(body["id"]),
		DisplayName:   firstNonEmpty(stringValue(body["business_name"]), stringValue(body["display_name"]), stringValue(businessProfile["name"]), stringValue(body["id"])),
		Email:         stringValue(body["email"]),
		WorkspaceID:   stringValue(body["id"]),
		WorkspaceName: firstNonEmpty(stringValue(body["business_name"]), stringValue(body["display_name"]), stringValue(businessProfile["name"])),
		TenantID:      stringValue(body["id"]),
	}, nil
}

func (c *OAuth2Client) linkedinProfile(ctx context.Context, accessToken string) (ProviderProfile, error) {
	body, err := c.getJSON(ctx, accessToken, strings.TrimRight(c.cfg.APIBaseURL, "/")+"/v2/userinfo", nil)
	if err != nil {
		return ProviderProfile{}, err
	}
	return ProviderProfile{
		ID:          stringValue(body["sub"]),
		DisplayName: firstNonEmpty(stringValue(body["name"]), stringValue(body["localizedFirstName"])+" "+stringValue(body["localizedLastName"])),
		Email:       stringValue(body["email"]),
	}, nil
}

func (c *OAuth2Client) xProfile(ctx context.Context, accessToken string) (ProviderProfile, error) {
	body, err := c.getJSON(ctx, accessToken, strings.TrimRight(c.cfg.APIBaseURL, "/")+"/2/users/me?user.fields=username,name", nil)
	if err != nil {
		return ProviderProfile{}, err
	}
	data, _ := body["data"].(map[string]any)
	username := stringValue(data["username"])
	return ProviderProfile{
		ID:          stringValue(data["id"]),
		DisplayName: firstNonEmpty(stringValue(data["name"]), username),
		WorkspaceID: username,
	}, nil
}

func (c *OAuth2Client) instagramProfile(ctx context.Context, accessToken string) (ProviderProfile, error) {
	body, err := c.getJSON(ctx, accessToken, strings.TrimRight(c.cfg.APIBaseURL, "/")+"/me?fields=id,name", nil)
	if err != nil {
		return ProviderProfile{}, err
	}
	return ProviderProfile{
		ID:          stringValue(body["id"]),
		DisplayName: firstNonEmpty(stringValue(body["name"]), stringValue(body["id"])),
	}, nil
}

func (c *OAuth2Client) facebookProfile(ctx context.Context, accessToken string) (ProviderProfile, error) {
	body, err := c.getJSON(ctx, accessToken, strings.TrimRight(c.cfg.APIBaseURL, "/")+"/me?fields=id,name", nil)
	if err != nil {
		return ProviderProfile{}, err
	}
	return ProviderProfile{
		ID:          stringValue(body["id"]),
		DisplayName: firstNonEmpty(stringValue(body["name"]), stringValue(body["id"])),
	}, nil
}

func (c *OAuth2Client) snapchatProfile(ctx context.Context, accessToken string) (ProviderProfile, error) {
	body, err := c.getJSON(ctx, accessToken, strings.TrimRight(c.cfg.APIBaseURL, "/")+"/me/organizations", nil)
	if err != nil {
		return ProviderProfile{}, err
	}
	profile := ProviderProfile{
		ID:          "snapchat",
		DisplayName: "Snapchat Marketing",
	}
	if organizations, ok := body["organizations"].([]any); ok && len(organizations) > 0 {
		if wrapper, ok := organizations[0].(map[string]any); ok {
			if org, ok := wrapper["organization"].(map[string]any); ok {
				profile.ID = firstNonEmpty(stringValue(org["id"]), profile.ID)
				profile.DisplayName = firstNonEmpty(stringValue(org["name"]), profile.DisplayName)
				profile.WorkspaceID = stringValue(org["id"])
				profile.WorkspaceName = stringValue(org["name"])
				profile.TenantID = stringValue(org["id"])
			}
		}
	}
	return profile, nil
}

func (c *OAuth2Client) getJSON(ctx context.Context, accessToken, endpoint string, headers map[string]string) (map[string]any, error) {
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, endpoint, nil)
	if err != nil {
		return nil, fmt.Errorf("build %s profile request: %w", c.cfg.ProviderKey, err)
	}
	req.Header.Set("Authorization", "Bearer "+accessToken)
	req.Header.Set("Accept", "application/json")
	for key, value := range headers {
		req.Header.Set(key, value)
	}

	resp, err := c.httpClient.Do(req)
	if err != nil {
		return nil, fmt.Errorf("call %s profile: %w", c.cfg.ProviderKey, err)
	}
	defer resp.Body.Close()
	var body map[string]any
	if err := json.NewDecoder(resp.Body).Decode(&body); err != nil {
		return nil, fmt.Errorf("decode %s profile: %w", c.cfg.ProviderKey, err)
	}
	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		return nil, fmt.Errorf("%s profile returned status %d: %s", c.cfg.ProviderKey, resp.StatusCode, tokenError(body))
	}
	return body, nil
}

type NotionOAuthClient struct {
	base *OAuth2Client
}

func NewNotionOAuthClient(cfg OAuth2ClientConfig) *NotionOAuthClient {
	cfg.UseBasicAuth = true
	return &NotionOAuthClient{base: NewOAuth2Client(cfg)}
}

func (c *NotionOAuthClient) AuthorizationURL(state, redirectURI, codeVerifier string, scopes []string, providerContext map[string]string) (string, error) {
	return c.base.AuthorizationURL(state, redirectURI, codeVerifier, scopes, providerContext)
}

func (c *NotionOAuthClient) ExchangeCode(ctx context.Context, code, redirectURI, _ string, _ []string, _ map[string]string) (TokenResult, error) {
	body := map[string]string{
		"grant_type":   "authorization_code",
		"code":         code,
		"redirect_uri": redirectURI,
	}
	payload, err := json.Marshal(body)
	if err != nil {
		return TokenResult{}, fmt.Errorf("marshal notion token body: %w", err)
	}
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, c.base.cfg.TokenURL, strings.NewReader(string(payload)))
	if err != nil {
		return TokenResult{}, fmt.Errorf("build notion token request: %w", err)
	}
	req.Header.Set("Accept", "application/json")
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("Authorization", "Basic "+base64.StdEncoding.EncodeToString([]byte(c.base.cfg.ClientID+":"+c.base.cfg.ClientSecret)))
	return doTokenRequest(c.base.httpClient, req, "notion")
}

func (c *NotionOAuthClient) Refresh(_ context.Context, _ string, _ []string, _ map[string]string) (TokenResult, error) {
	return TokenResult{}, fmt.Errorf("notion tokens do not support refresh in this broker")
}

func (c *NotionOAuthClient) Revoke(_ context.Context, _ string, _ map[string]string) error {
	return nil
}

func (c *NotionOAuthClient) Profile(ctx context.Context, accessToken string, providerContext map[string]string) (ProviderProfile, error) {
	return c.base.Profile(ctx, accessToken, providerContext)
}

type ShopifyOAuthClient struct {
	clientID     string
	clientSecret string
	httpClient   *http.Client
}

func NewShopifyOAuthClient(clientID, clientSecret string, httpClient *http.Client) *ShopifyOAuthClient {
	if httpClient == nil {
		httpClient = &http.Client{Timeout: defaultHTTPTimeout}
	}
	return &ShopifyOAuthClient{clientID: clientID, clientSecret: clientSecret, httpClient: httpClient}
}

func (c *ShopifyOAuthClient) AuthorizationURL(state, redirectURI, _ string, scopes []string, providerContext map[string]string) (string, error) {
	shop, err := ShopifyShop(providerContext)
	if err != nil {
		return "", err
	}
	values := url.Values{}
	values.Set("client_id", c.clientID)
	values.Set("redirect_uri", redirectURI)
	values.Set("scope", strings.Join(scopes, ","))
	values.Set("state", state)
	return "https://" + shop + "/admin/oauth/authorize?" + values.Encode(), nil
}

func (c *ShopifyOAuthClient) ExchangeCode(ctx context.Context, code, _ string, _ string, _ []string, providerContext map[string]string) (TokenResult, error) {
	shop, err := ShopifyShop(providerContext)
	if err != nil {
		return TokenResult{}, err
	}
	values := url.Values{}
	values.Set("client_id", c.clientID)
	values.Set("client_secret", c.clientSecret)
	values.Set("code", code)
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, "https://"+shop+"/admin/oauth/access_token", strings.NewReader(values.Encode()))
	if err != nil {
		return TokenResult{}, fmt.Errorf("build shopify token request: %w", err)
	}
	req.Header.Set("Content-Type", "application/x-www-form-urlencoded")
	req.Header.Set("Accept", "application/json")
	return doTokenRequest(c.httpClient, req, "shopify")
}

func (c *ShopifyOAuthClient) Refresh(_ context.Context, _ string, _ []string, _ map[string]string) (TokenResult, error) {
	return TokenResult{}, fmt.Errorf("shopify offline access tokens do not support refresh")
}

func (c *ShopifyOAuthClient) Revoke(_ context.Context, _ string, _ map[string]string) error {
	return nil
}

func (c *ShopifyOAuthClient) Profile(ctx context.Context, accessToken string, providerContext map[string]string) (ProviderProfile, error) {
	shop, err := ShopifyShop(providerContext)
	if err != nil {
		return ProviderProfile{}, err
	}
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, "https://"+shop+"/admin/api/2026-01/shop.json", nil)
	if err != nil {
		return ProviderProfile{}, fmt.Errorf("build shopify profile request: %w", err)
	}
	req.Header.Set("X-Shopify-Access-Token", accessToken)
	req.Header.Set("Accept", "application/json")
	resp, err := c.httpClient.Do(req)
	if err != nil {
		return ProviderProfile{}, fmt.Errorf("call shopify profile: %w", err)
	}
	defer resp.Body.Close()
	var body struct {
		Shop struct {
			ID            any    `json:"id"`
			Name          string `json:"name"`
			Email         string `json:"email"`
			MyshopifyName string `json:"myshopify_domain"`
		} `json:"shop"`
	}
	if err := json.NewDecoder(resp.Body).Decode(&body); err != nil {
		return ProviderProfile{}, fmt.Errorf("decode shopify profile: %w", err)
	}
	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		return ProviderProfile{}, fmt.Errorf("shopify profile returned status %d", resp.StatusCode)
	}
	return ProviderProfile{
		ID:            fmt.Sprintf("%v", body.Shop.ID),
		DisplayName:   firstNonEmpty(body.Shop.Name, body.Shop.MyshopifyName, shop),
		Email:         body.Shop.Email,
		WorkspaceID:   firstNonEmpty(body.Shop.MyshopifyName, shop),
		WorkspaceName: firstNonEmpty(body.Shop.Name, body.Shop.MyshopifyName, shop),
		TenantID:      firstNonEmpty(body.Shop.MyshopifyName, shop),
	}, nil
}

func doTokenRequest(client *http.Client, req *http.Request, providerKey string) (TokenResult, error) {
	resp, err := client.Do(req)
	if err != nil {
		return TokenResult{}, fmt.Errorf("call %s token endpoint: %w", providerKey, err)
	}
	defer resp.Body.Close()

	var raw map[string]any
	if err := json.NewDecoder(resp.Body).Decode(&raw); err != nil {
		return TokenResult{}, fmt.Errorf("decode %s token response: %w", providerKey, err)
	}
	if ok, _ := raw["ok"].(bool); raw["ok"] != nil && !ok {
		return TokenResult{}, fmt.Errorf("%s token endpoint returned error: %s", providerKey, tokenError(raw))
	}
	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		return TokenResult{}, fmt.Errorf("%s token endpoint returned status %d: %s", providerKey, resp.StatusCode, tokenError(raw))
	}
	accessToken := firstNonEmpty(stringValue(raw["access_token"]), stringValue(raw["authed_user.access_token"]))
	if accessToken == "" {
		return TokenResult{}, fmt.Errorf("%s token endpoint returned empty access_token", providerKey)
	}
	refreshToken := stringValue(raw["refresh_token"])
	scopeValue := firstNonEmpty(stringValue(raw["scope"]), stringValue(raw["scopes"]))
	expiresIn := expiresInSeconds(raw["expires_in"])
	if expiresIn <= 0 {
		expiresIn = 0
	}
	expiresAt := time.Now().UTC().Add(365 * 24 * time.Hour)
	if expiresIn > 0 {
		expiresAt = time.Now().UTC().Add(time.Duration(expiresIn) * time.Second)
	}
	return TokenResult{
		AccessToken:  accessToken,
		RefreshToken: refreshToken,
		TokenType:    firstNonEmpty(stringValue(raw["token_type"]), "Bearer"),
		Scope:        splitReturnedScopes(scopeValue),
		ExpiresAt:    expiresAt,
		Raw:          raw,
	}, nil
}

func splitReturnedScopes(scopeValue string) []string {
	scopeValue = strings.TrimSpace(scopeValue)
	if scopeValue == "" {
		return nil
	}
	if strings.Contains(scopeValue, ",") {
		parts := strings.Split(scopeValue, ",")
		out := make([]string, 0, len(parts))
		for _, part := range parts {
			if trimmed := strings.TrimSpace(part); trimmed != "" {
				out = append(out, trimmed)
			}
		}
		return out
	}
	return strings.Fields(scopeValue)
}

func firstNonEmpty(values ...string) string {
	for _, value := range values {
		if strings.TrimSpace(value) != "" {
			return value
		}
	}
	return ""
}
