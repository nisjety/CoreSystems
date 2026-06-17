package integration

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"strings"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/social-core/internal/social"
)

type Client struct {
	baseURL        string
	internalAPIKey string
	httpClient     *http.Client
}

type Config struct {
	BaseURL        string
	InternalAPIKey string
	HTTPClient     *http.Client
}

type envelope struct {
	Success bool            `json:"success"`
	Data    json.RawMessage `json:"data"`
	Error   *struct {
		Code    string `json:"code"`
		Message string `json:"message"`
	} `json:"error"`
}

type connectionsPayload struct {
	Connections []connection `json:"connections"`
}

type connection struct {
	ID                   string            `json:"id"`
	ProviderKey          string            `json:"providerKey"`
	ConnectorType        string            `json:"connectorType"`
	OrganizationID       string            `json:"organizationId"`
	Status               string            `json:"status"`
	DisplayName          string            `json:"displayName"`
	ProviderAccountID    string            `json:"providerAccountId"`
	ProviderContext      map[string]string `json:"providerContext"`
	Capabilities         []string          `json:"capabilities"`
	AccessTokenExpiresAt time.Time         `json:"accessTokenExpiresAt"`
}

type tokenPayload struct {
	ConnectionID string    `json:"connectionId"`
	ProviderKey  string    `json:"providerKey"`
	AccessToken  string    `json:"accessToken"`
	ExpiresAt    time.Time `json:"expiresAt"`
	Scopes       []string  `json:"scopes"`
	Capabilities []string  `json:"capabilities"`
}

func NewClient(cfg Config) *Client {
	httpClient := cfg.HTTPClient
	if httpClient == nil {
		httpClient = &http.Client{Timeout: 15 * time.Second}
	}
	return &Client{
		baseURL:        strings.TrimRight(strings.TrimSpace(cfg.BaseURL), "/"),
		internalAPIKey: strings.TrimSpace(cfg.InternalAPIKey),
		httpClient:     httpClient,
	}
}

func (c *Client) ListSocialAccounts(ctx context.Context, orgID string) ([]social.Account, error) {
	if err := c.ensureConfigured(); err != nil {
		return nil, err
	}
	query := url.Values{}
	query.Set("organizationId", strings.TrimSpace(orgID))
	query.Set("category", "social")
	endpoint := c.baseURL + "/api/v1/connections?" + query.Encode()
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, endpoint, nil)
	if err != nil {
		return nil, err
	}
	c.addInternalHeaders(req)
	var payload connectionsPayload
	if err := c.do(req, &payload); err != nil {
		return nil, err
	}
	accounts := make([]social.Account, 0, len(payload.Connections))
	for _, connection := range payload.Connections {
		providerKey := normalizeProvider(connection.ProviderKey)
		if !isSocialProvider(providerKey) {
			continue
		}
		metadata := map[string]any{
			"connector_type":       connection.ConnectorType,
			"provider_account_id":  connection.ProviderAccountID,
			"provider_context":     connection.ProviderContext,
			"access_expires_at":    connection.AccessTokenExpiresAt,
			"integration_core_ref": connection.ID,
		}
		for key, value := range connection.ProviderContext {
			metadata[key] = value
		}
		accounts = append(accounts, social.Account{
			ID:             stableAccountID(connection.OrganizationID, providerKey, connection.ID),
			OrgID:          connection.OrganizationID,
			ProviderKey:    providerKey,
			ConnectionID:   connection.ID,
			DisplayName:    firstNonEmpty(connection.DisplayName, providerKey),
			Handle:         firstNonEmpty(connection.ProviderAccountID, connection.ProviderContext["handle"], connection.ProviderContext["username"]),
			Status:         accountStatus(connection.Status),
			Capabilities:   connection.Capabilities,
			TokenState:     tokenState(connection.Status, connection.AccessTokenExpiresAt),
			TokenExpiresAt: tokenExpiresAt(connection.AccessTokenExpiresAt),
			Metadata:       metadata,
		})
	}
	return accounts, nil
}

func (c *Client) AccessToken(ctx context.Context, request social.TokenRequest) (*social.TokenLease, error) {
	if err := c.ensureConfigured(); err != nil {
		return nil, err
	}
	body := map[string]string{
		"organizationId": strings.TrimSpace(request.OrganizationID),
		"connectorType":  strings.TrimSpace(request.ConnectorType),
		"connectionId":   strings.TrimSpace(request.ConnectionID),
		"consumer":       strings.TrimSpace(request.Consumer),
	}
	payload, err := json.Marshal(body)
	if err != nil {
		return nil, err
	}
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, c.baseURL+"/internal/connectors/token", bytes.NewReader(payload))
	if err != nil {
		return nil, err
	}
	c.addInternalHeaders(req)
	req.Header.Set("Content-Type", "application/json")
	var token tokenPayload
	if err := c.do(req, &token); err != nil {
		return nil, err
	}
	return &social.TokenLease{
		ConnectionID: token.ConnectionID,
		ProviderKey:  normalizeProvider(token.ProviderKey),
		AccessToken:  token.AccessToken,
		ExpiresAt:    token.ExpiresAt,
		Scopes:       token.Scopes,
		Capabilities: token.Capabilities,
	}, nil
}

func (c *Client) ensureConfigured() error {
	if c == nil || c.baseURL == "" {
		return fmt.Errorf("integration-core url is not configured")
	}
	if c.internalAPIKey == "" {
		return fmt.Errorf("internal api key is not configured")
	}
	return nil
}

func (c *Client) addInternalHeaders(req *http.Request) {
	req.Header.Set("X-Internal-API-Key", c.internalAPIKey)
	req.Header.Set("Accept", "application/json")
}

func (c *Client) do(req *http.Request, target any) error {
	resp, err := c.httpClient.Do(req)
	if err != nil {
		return err
	}
	defer resp.Body.Close()
	var env envelope
	body := io.LimitReader(resp.Body, 1<<20)
	if err := json.NewDecoder(body).Decode(&env); err != nil {
		return fmt.Errorf("decode integration-core response: %w", err)
	}
	if resp.StatusCode < 200 || resp.StatusCode >= 300 || !env.Success {
		if env.Error != nil {
			return fmt.Errorf("%s: %s", env.Error.Code, env.Error.Message)
		}
		return fmt.Errorf("integration-core returned status %d", resp.StatusCode)
	}
	if len(env.Data) == 0 {
		return nil
	}
	if err := json.Unmarshal(env.Data, target); err != nil {
		return fmt.Errorf("decode integration-core data: %w", err)
	}
	return nil
}

func isSocialProvider(providerKey string) bool {
	switch providerKey {
	case "linkedin", "x", "instagram", "facebook", "tiktok", "snapchat":
		return true
	default:
		return false
	}
}

func normalizeProvider(value string) string {
	switch strings.ToLower(strings.TrimSpace(value)) {
	case "twitter":
		return "x"
	case "linked_in":
		return "linkedin"
	case "tik_tok", "tik-tok":
		return "tiktok"
	case "facebook-page", "facebook-pages", "meta-facebook":
		return "facebook"
	case "snap", "snapchat-ads", "snapchat-marketing":
		return "snapchat"
	default:
		return strings.ToLower(strings.TrimSpace(value))
	}
}

func accountStatus(status string) string {
	switch strings.ToLower(strings.TrimSpace(status)) {
	case "connected", "active", "ok":
		return social.AccountStatusConnected
	case "expired":
		return social.AccountStatusExpired
	case "error":
		return social.AccountStatusError
	default:
		return social.AccountStatusDisconnected
	}
}

func tokenState(status string, expiresAt time.Time) string {
	if accountStatus(status) != social.AccountStatusConnected {
		return "missing"
	}
	if !expiresAt.IsZero() && time.Now().UTC().After(expiresAt) {
		return "expired"
	}
	return "available"
}

func tokenExpiresAt(value time.Time) *time.Time {
	if value.IsZero() {
		return nil
	}
	return &value
}

func firstNonEmpty(values ...string) string {
	for _, value := range values {
		if strings.TrimSpace(value) != "" {
			return strings.TrimSpace(value)
		}
	}
	return ""
}

func stableAccountID(orgID, providerKey, connectionID string) string {
	return "socacct_" + strings.NewReplacer(":", "_", "/", "_").Replace(strings.ToLower(strings.Join([]string{orgID, providerKey, connectionID}, ":")))
}
