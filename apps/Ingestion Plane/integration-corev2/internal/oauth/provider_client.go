package oauth

import (
	"context"
	"encoding/base64"
	"encoding/json"
	"errors"
	"fmt"
	"net/http"
	"net/url"
	"slices"
	"strings"
	"time"
)

var ErrRevocationUnsupported = errors.New("provider token revocation is unsupported")

// TokenEndpointError retains the provider's machine-readable OAuth failure
// code while preserving the existing safe error text for logs and operators.
// Callers use it to distinguish a reconnect-required grant from a transient
// provider outage without inspecting an error string.
type TokenEndpointError struct {
	ProviderKey string
	StatusCode  int
	Code        string
	Description string
}

func (e *TokenEndpointError) Error() string {
	if e.StatusCode > 0 {
		return fmt.Sprintf("%s token endpoint returned status %d: %s", e.ProviderKey, e.StatusCode, e.Description)
	}
	return fmt.Sprintf("%s token endpoint returned error: %s", e.ProviderKey, e.Description)
}

// IsAuthorizationRefreshRequired reports whether OAuth has authoritatively
// rejected a refresh grant. This state can only recover through a new human
// authorization flow; retrying it in the background cannot succeed.
func IsAuthorizationRefreshRequired(err error) bool {
	var tokenErr *TokenEndpointError
	return errors.As(err, &tokenErr) && strings.EqualFold(strings.TrimSpace(tokenErr.Code), "invalid_grant")
}

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
		return ErrRevocationUnsupported
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
	case "whatsapp", "meta-ads", "meta":
		return c.facebookProfile(ctx, accessToken)
	case "snapchat":
		return c.snapchatProfile(ctx, accessToken)
	case "tiktok":
		return c.tiktokProfile(ctx, accessToken)
	case "discord":
		return c.discordProfile(ctx, accessToken)
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
	headers := map[string]string{
		"Accept":               "application/vnd.github+json",
		"X-GitHub-Api-Version": "2022-11-28",
	}
	body, err := c.getJSON(ctx, accessToken, strings.TrimRight(c.cfg.APIBaseURL, "/")+"/user", map[string]string{
		"Accept":               "application/vnd.github+json",
		"X-GitHub-Api-Version": "2022-11-28",
	})
	if err != nil {
		return ProviderProfile{}, err
	}
	email := stringValue(body["email"])
	if email == "" {
		if fallback, err := c.githubPrimaryEmail(ctx, accessToken, headers); err == nil {
			email = fallback
		}
	}
	return ProviderProfile{
		ID:          fmt.Sprintf("%v", body["id"]),
		DisplayName: firstNonEmpty(stringValue(body["name"]), stringValue(body["login"])),
		Email:       email,
	}, nil
}

func (c *OAuth2Client) githubPrimaryEmail(ctx context.Context, accessToken string, headers map[string]string) (string, error) {
	endpoint := strings.TrimRight(c.cfg.APIBaseURL, "/") + "/user/emails?per_page=100"
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, endpoint, nil)
	if err != nil {
		return "", fmt.Errorf("build github email request: %w", err)
	}
	req.Header.Set("Authorization", "Bearer "+accessToken)
	req.Header.Set("Accept", "application/json")
	for key, value := range headers {
		req.Header.Set(key, value)
	}
	resp, err := c.httpClient.Do(req)
	if err != nil {
		return "", fmt.Errorf("call github email endpoint: %w", err)
	}
	defer resp.Body.Close()
	var emails []map[string]any
	if err := json.NewDecoder(resp.Body).Decode(&emails); err != nil {
		return "", fmt.Errorf("decode github email response: %w", err)
	}
	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		return "", fmt.Errorf("github email endpoint returned status %d", resp.StatusCode)
	}

	firstVerified := ""
	firstEmail := ""
	for _, item := range emails {
		email := stringValue(item["email"])
		if email == "" {
			continue
		}
		if firstEmail == "" {
			firstEmail = email
		}
		verified, _ := item["verified"].(bool)
		primary, _ := item["primary"].(bool)
		if verified && firstVerified == "" {
			firstVerified = email
		}
		if verified && primary {
			return email, nil
		}
	}
	return firstNonEmpty(firstVerified, firstEmail), nil
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
	profile := ProviderProfile{
		ID:          "snapchat",
		DisplayName: "Snapchat Marketing",
	}
	// /me/organizations requires the snapchat-marketing-api scope. A
	// connection made with only social.profile.read (scope
	// snapchat-profile-api, e.g. the "onboarding" bundle) will always fail
	// this call -- that's an expected consequence of the scope it was
	// granted, not a real error, so degrade to the generic identity above
	// instead of an empty ProviderProfile{} (which would otherwise surface
	// as the "snapchat connection" placeholder name in persistConnection).
	body, err := c.getJSON(ctx, accessToken, strings.TrimRight(c.cfg.APIBaseURL, "/")+"/me/organizations", nil)
	if err != nil {
		return profile, err
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

// tiktokProfile reads the Login Kit v2 user object. The response is enveloped:
// {"data":{"user":{...}},"error":{...}} and field selection is mandatory via
// the `fields` query parameter.
func (c *OAuth2Client) tiktokProfile(ctx context.Context, accessToken string) (ProviderProfile, error) {
	body, err := c.getJSON(ctx, accessToken, strings.TrimRight(c.cfg.APIBaseURL, "/")+"/user/info/?fields=open_id,union_id,display_name,avatar_url", nil)
	if err != nil {
		return ProviderProfile{}, err
	}
	data, _ := body["data"].(map[string]any)
	user, _ := data["user"].(map[string]any)
	id := firstNonEmpty(stringValue(user["open_id"]), stringValue(user["union_id"]))
	if id == "" {
		return ProviderProfile{}, fmt.Errorf("tiktok profile response missing open_id: %s", tokenError(body))
	}
	return ProviderProfile{
		ID:          id,
		DisplayName: firstNonEmpty(stringValue(user["display_name"]), id),
	}, nil
}

func (c *OAuth2Client) discordProfile(ctx context.Context, accessToken string) (ProviderProfile, error) {
	body, err := c.getJSON(ctx, accessToken, strings.TrimRight(c.cfg.APIBaseURL, "/")+"/users/@me", nil)
	if err != nil {
		return ProviderProfile{}, err
	}
	return ProviderProfile{
		ID:          stringValue(body["id"]),
		DisplayName: firstNonEmpty(stringValue(body["global_name"]), stringValue(body["username"]), stringValue(body["id"])),
		Email:       stringValue(body["email"]),
	}, nil
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
	return ErrRevocationUnsupported
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
	return ErrRevocationUnsupported
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

// MetaOAuthClient implements the unified Meta provider (Facebook Pages +
// Instagram + WhatsApp Business + Marketing API in one dialog).
//
// With a Facebook Login for Business configuration id it sends config_id
// (which replaces `scope`) plus response_type=code and
// override_default_response_type=true — required when the configuration issues
// Business Integration System User tokens. Without one it falls back to
// classic Facebook Login with comma-separated scopes (all scopes work in dev
// mode for users holding a role on a Business-type app).
//
// Meta has no PKCE and never issues refresh tokens: short-lived user tokens
// are upgraded via grant_type=fb_exchange_token to ~60-day tokens (Refresh).
type MetaOAuthClient struct {
	base      *OAuth2Client
	configIDs map[string]string
}

// NewMetaOAuthClient takes a set of named Facebook Login for Business
// configurations, keyed by purpose (e.g. "default", "conversions"). Meta
// bakes permissions into the configuration itself rather than a per-request
// scope param, so a Business-type app needing more than one permission set
// needs one configuration per set — a single shared config_id can't express
// "general Pages/Instagram access" and "narrower Conversions API partner
// access" at once. Selected per connect session via
// providerContext["business_login_config"]; "default" is used when unset.
func NewMetaOAuthClient(cfg OAuth2ClientConfig, businessLoginConfigIDs map[string]string) *MetaOAuthClient {
	ids := make(map[string]string, len(businessLoginConfigIDs))
	for key, id := range businessLoginConfigIDs {
		if id := strings.TrimSpace(id); id != "" {
			ids[key] = id
		}
	}
	return &MetaOAuthClient{base: NewOAuth2Client(cfg), configIDs: ids}
}

const defaultMetaBusinessLoginConfig = "default"

func (c *MetaOAuthClient) businessLoginConfigID(providerContext map[string]string) string {
	key := strings.TrimSpace(providerContext["business_login_config"])
	if key == "" {
		// General Meta connects use classic Facebook Login so the exact
		// requested bundle scopes are visible in the dialog. A Business Login
		// configuration is selected only when the caller explicitly names one
		// (for example the narrower Conversions API configuration). Treating a
		// configured "default" id as implicit caused misconfigured dashboard
		// presets to silently issue public_profile-only tokens.
		return ""
	}
	return c.configIDs[key]
}

func (c *MetaOAuthClient) AuthorizationURL(state, redirectURI, _ string, scopes []string, providerContext map[string]string) (string, error) {
	values := url.Values{}
	values.Set("client_id", c.base.cfg.ClientID)
	values.Set("redirect_uri", redirectURI)
	values.Set("response_type", "code")
	values.Set("state", state)
	if configID := c.businessLoginConfigID(providerContext); configID != "" {
		values.Set("config_id", configID)
		// Configurations may default to a non-code response type (e.g. BISU
		// token configs); force the authorization-code grant.
		values.Set("override_default_response_type", "true")
	} else if len(scopes) > 0 {
		values.Set("scope", strings.Join(scopes, ","))
	}
	return c.base.cfg.AuthorizationURL + "?" + values.Encode(), nil
}

func (c *MetaOAuthClient) ExchangeCode(ctx context.Context, code, redirectURI, codeVerifier string, scopes []string, providerContext map[string]string) (TokenResult, error) {
	token, err := c.base.ExchangeCode(ctx, code, redirectURI, codeVerifier, scopes, providerContext)
	if err != nil {
		return TokenResult{}, err
	}
	if token.AccessToken == "" {
		return token, nil
	}
	if c.businessLoginConfigID(providerContext) != "" {
		// Business Login configurations can issue Business Integration System
		// User tokens directly from the authorization-code exchange. Those are
		// not classic short-lived user tokens, so do not force fb_exchange_token
		// during callback.
		if token.RefreshToken == "" {
			token.RefreshToken = token.AccessToken
		}
		token.Scope, err = c.grantedPermissions(ctx, token.AccessToken)
		if err != nil {
			return TokenResult{}, err
		}
		token.ScopesVerified = true
		return token, nil
	}
	longLived, err := c.Refresh(ctx, token.AccessToken, scopes, providerContext)
	if err != nil {
		return TokenResult{}, err
	}
	return longLived, nil
}

func (c *MetaOAuthClient) grantedPermissions(ctx context.Context, accessToken string) ([]string, error) {
	next := strings.TrimRight(c.base.cfg.APIBaseURL, "/") + "/me/permissions?limit=100"
	granted := map[string]struct{}{}
	for page := 0; next != "" && page < 10; page++ {
		req, err := http.NewRequestWithContext(ctx, http.MethodGet, next, nil)
		if err != nil {
			return nil, fmt.Errorf("build Meta permissions request: %w", err)
		}
		req.Header.Set("Authorization", "Bearer "+accessToken)
		resp, err := sameOriginHTTPClient(c.base.httpClient, c.base.cfg.APIBaseURL).Do(req)
		if err != nil {
			return nil, fmt.Errorf("read Meta granted permissions: %w", err)
		}
		var payload struct {
			Data []struct {
				Permission string `json:"permission"`
				Status     string `json:"status"`
			} `json:"data"`
			Paging struct {
				Next string `json:"next"`
			} `json:"paging"`
			Error *struct {
				Message string `json:"message"`
			} `json:"error"`
		}
		decodeErr := json.NewDecoder(resp.Body).Decode(&payload)
		_ = resp.Body.Close()
		if decodeErr != nil {
			return nil, fmt.Errorf("decode Meta granted permissions: %w", decodeErr)
		}
		if resp.StatusCode < 200 || resp.StatusCode >= 300 || payload.Error != nil {
			message := "provider rejected permission inspection"
			if payload.Error != nil && strings.TrimSpace(payload.Error.Message) != "" {
				message = payload.Error.Message
			}
			return nil, fmt.Errorf("read Meta granted permissions: %s", message)
		}
		for _, permission := range payload.Data {
			if permission.Status == "granted" && strings.TrimSpace(permission.Permission) != "" {
				granted[permission.Permission] = struct{}{}
			}
		}
		next, err = c.safeGraphPageURL(payload.Paging.Next)
		if err != nil {
			return nil, err
		}
	}
	out := make([]string, 0, len(granted))
	for permission := range granted {
		out = append(out, permission)
	}
	slices.Sort(out)
	return out, nil
}

func sameOriginHTTPClient(client *http.Client, baseURL string) *http.Client {
	if client == nil {
		client = &http.Client{Timeout: 15 * time.Second}
	}
	cloned := *client
	configured, _ := url.Parse(baseURL)
	previous := client.CheckRedirect
	cloned.CheckRedirect = func(req *http.Request, via []*http.Request) error {
		if len(via) >= 10 || configured == nil || req.URL.Scheme != configured.Scheme || req.URL.Host != configured.Host {
			return fmt.Errorf("provider redirect left the configured origin")
		}
		if previous != nil {
			return previous(req, via)
		}
		return nil
	}
	return &cloned
}

func (c *MetaOAuthClient) safeGraphPageURL(raw string) (string, error) {
	raw = strings.TrimSpace(raw)
	if raw == "" {
		return "", nil
	}
	base, err := url.Parse(c.base.cfg.APIBaseURL)
	if err != nil {
		return "", fmt.Errorf("parse Meta Graph base URL: %w", err)
	}
	next, err := url.Parse(raw)
	if err != nil || !next.IsAbs() || next.Scheme != base.Scheme || next.Host != base.Host || next.User != nil {
		return "", fmt.Errorf("read Meta granted permissions: provider returned an off-origin pagination URL")
	}
	next.Fragment = ""
	return next.String(), nil
}

// Refresh upgrades/renews a Meta user token via fb_exchange_token (Meta issues
// no refresh tokens; the "refresh token" we persist is the access token
// itself, re-exchanged for a fresh ~60-day long-lived token).
func (c *MetaOAuthClient) Refresh(ctx context.Context, refreshToken string, _ []string, _ map[string]string) (TokenResult, error) {
	values := url.Values{}
	values.Set("grant_type", "fb_exchange_token")
	values.Set("client_id", c.base.cfg.ClientID)
	values.Set("client_secret", c.base.cfg.ClientSecret)
	values.Set("fb_exchange_token", refreshToken)
	token, err := c.base.tokenRequest(ctx, values)
	if err != nil {
		return TokenResult{}, err
	}
	if token.RefreshToken == "" {
		token.RefreshToken = token.AccessToken
	}
	token.Scope, err = c.grantedPermissions(ctx, token.AccessToken)
	if err != nil {
		return TokenResult{}, err
	}
	token.ScopesVerified = true
	return token, nil
}

func (c *MetaOAuthClient) Profile(ctx context.Context, accessToken string, providerContext map[string]string) (ProviderProfile, error) {
	return c.base.Profile(ctx, accessToken, providerContext)
}

func (c *MetaOAuthClient) Revoke(ctx context.Context, accessToken string, _ map[string]string) error {
	// DELETE /me/permissions de-authorizes the app for this user.
	req, err := http.NewRequestWithContext(ctx, http.MethodDelete, strings.TrimRight(c.base.cfg.APIBaseURL, "/")+"/me/permissions", nil)
	if err != nil {
		return fmt.Errorf("build meta revoke request: %w", err)
	}
	req.Header.Set("Authorization", "Bearer "+accessToken)
	return c.base.doRevoke(req)
}

// TikTokOAuthClient implements TikTok Login Kit v2. TikTok's OAuth deviates
// from RFC naming: the client identifier is `client_key` in BOTH the authorize
// query and the token body (client_id is rejected), scopes are comma-separated,
// credentials always go in the form body (no Basic auth), and PKCE challenges
// are SHA-256 hex encoded.
type TikTokOAuthClient struct {
	base *OAuth2Client
}

func NewTikTokOAuthClient(cfg OAuth2ClientConfig) *TikTokOAuthClient {
	return &TikTokOAuthClient{base: NewOAuth2Client(cfg)}
}

func (c *TikTokOAuthClient) AuthorizationURL(state, redirectURI, codeVerifier string, scopes []string, _ map[string]string) (string, error) {
	if strings.TrimSpace(codeVerifier) == "" {
		return "", fmt.Errorf("code verifier is required for TikTok OAuth")
	}
	values := url.Values{}
	values.Set("client_key", c.base.cfg.ClientID)
	values.Set("code_challenge", CodeChallengeS256Hex(codeVerifier))
	values.Set("code_challenge_method", "S256")
	values.Set("redirect_uri", redirectURI)
	values.Set("response_type", "code")
	values.Set("state", state)
	if len(scopes) > 0 {
		values.Set("scope", strings.Join(scopes, ","))
	}
	return c.base.cfg.AuthorizationURL + "?" + values.Encode(), nil
}

func (c *TikTokOAuthClient) ExchangeCode(ctx context.Context, code, redirectURI, codeVerifier string, _ []string, _ map[string]string) (TokenResult, error) {
	values := url.Values{}
	values.Set("client_key", c.base.cfg.ClientID)
	values.Set("client_secret", c.base.cfg.ClientSecret)
	values.Set("grant_type", "authorization_code")
	values.Set("code", code)
	values.Set("redirect_uri", redirectURI)
	if strings.TrimSpace(codeVerifier) != "" {
		values.Set("code_verifier", codeVerifier)
	}
	return c.base.tokenRequest(ctx, values)
}

func (c *TikTokOAuthClient) Refresh(ctx context.Context, refreshToken string, _ []string, _ map[string]string) (TokenResult, error) {
	values := url.Values{}
	values.Set("client_key", c.base.cfg.ClientID)
	values.Set("client_secret", c.base.cfg.ClientSecret)
	values.Set("grant_type", "refresh_token")
	values.Set("refresh_token", refreshToken)
	return c.base.tokenRequest(ctx, values)
}

func (c *TikTokOAuthClient) Profile(ctx context.Context, accessToken string, providerContext map[string]string) (ProviderProfile, error) {
	return c.base.Profile(ctx, accessToken, providerContext)
}

func (c *TikTokOAuthClient) Revoke(ctx context.Context, accessToken string, _ map[string]string) error {
	values := url.Values{}
	values.Set("client_key", c.base.cfg.ClientID)
	values.Set("client_secret", c.base.cfg.ClientSecret)
	values.Set("token", accessToken)
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, strings.TrimRight(c.base.cfg.APIBaseURL, "/")+"/oauth/revoke/", strings.NewReader(values.Encode()))
	if err != nil {
		return fmt.Errorf("build tiktok revoke request: %w", err)
	}
	req.Header.Set("Content-Type", "application/x-www-form-urlencoded")
	return c.base.doRevoke(req)
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
		return TokenResult{}, tokenEndpointError(providerKey, 0, raw)
	}
	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		return TokenResult{}, tokenEndpointError(providerKey, resp.StatusCode, raw)
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

func tokenEndpointError(providerKey string, statusCode int, raw map[string]any) *TokenEndpointError {
	return &TokenEndpointError{
		ProviderKey: providerKey,
		StatusCode:  statusCode,
		Code:        stringValue(raw["error"]),
		Description: tokenError(raw),
	}
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
