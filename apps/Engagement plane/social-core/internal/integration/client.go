package integration

import (
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net"
	"net/http"
	"net/url"
	"strings"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/social-core/internal/social"
	"github.com/triodelab/studio-plane/planeclient"
)

type Client struct {
	baseURL               string
	tenantBearerToken     string
	tenantBearerTokenFile string
	tenantOrganizationID  string
	httpClient            *http.Client
}

type Config struct {
	BaseURL               string
	TenantBearerToken     string
	TenantBearerTokenFile string
	TenantOrganizationID  string
	HTTPClient            *http.Client
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

func NewClient(cfg Config) *Client {
	httpClient := http.Client{Timeout: 15 * time.Second}
	if cfg.HTTPClient != nil {
		httpClient = *cfg.HTTPClient
	}
	httpClient.CheckRedirect = func(*http.Request, []*http.Request) error { return http.ErrUseLastResponse }
	return &Client{
		baseURL:               strings.TrimSpace(cfg.BaseURL),
		tenantBearerToken:     strings.TrimSpace(cfg.TenantBearerToken),
		tenantBearerTokenFile: cfg.TenantBearerTokenFile,
		tenantOrganizationID:  strings.TrimSpace(cfg.TenantOrganizationID),
		httpClient:            &httpClient,
	}
}

func (c *Client) ListSocialAccounts(ctx context.Context, orgID string) ([]social.Account, error) {
	if err := c.ensureTenantConfigured(); err != nil {
		return nil, err
	}
	if strings.TrimSpace(orgID) == "" || strings.TrimSpace(orgID) != c.tenantOrganizationID {
		return nil, fmt.Errorf("organization does not match configured integration-core tenant")
	}
	query := url.Values{}
	query.Set("organizationId", strings.TrimSpace(orgID))
	query.Set("category", "social")
	endpoint := c.baseURL + "/api/v1/connections?" + query.Encode()
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, endpoint, nil)
	if err != nil {
		return nil, err
	}
	var payload connectionsPayload
	if err := c.do(req, &payload); err != nil {
		return nil, err
	}
	accounts := make([]social.Account, 0, len(payload.Connections))
	for _, connection := range payload.Connections {
		if strings.TrimSpace(connection.OrganizationID) != c.tenantOrganizationID {
			continue
		}
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

func (c *Client) ensureTenantConfigured() error {
	if c == nil || c.baseURL == "" {
		return fmt.Errorf("integration-core url is not configured")
	}
	if err := validateIntegrationBaseURL(c.baseURL); err != nil {
		return err
	}
	if (c.tenantBearerToken == "" && c.tenantBearerTokenFile == "") || c.tenantOrganizationID == "" {
		return fmt.Errorf("integration-core tenant bearer and organization are required")
	}
	return nil
}

func validateIntegrationBaseURL(baseURL string) error {
	u, err := url.Parse(strings.TrimSpace(baseURL))
	if err != nil || u.Hostname() == "" || u.User != nil || u.Opaque != "" || u.Path != "" || u.RawPath != "" || u.RawQuery != "" || u.ForceQuery || u.Fragment != "" || u.RawFragment != "" {
		return fmt.Errorf("integration-core url must be an origin without credentials, path, query, or fragment")
	}
	if u.Scheme == "https" {
		return nil
	}
	if u.Scheme != "http" {
		return fmt.Errorf("integration-core url must use HTTPS or approved local HTTP")
	}
	host := u.Hostname()
	if host == "integration-api" && u.Port() == "3026" {
		return nil
	}
	if host == "localhost" {
		return nil
	}
	if ip := net.ParseIP(host); ip != nil && ip.IsLoopback() {
		return nil
	}
	return fmt.Errorf("integration-core url must use HTTPS or approved local HTTP")
}

func (c *Client) addTenantHeaders(req *http.Request) error {
	token := c.tenantBearerToken
	if c.tenantBearerTokenFile != "" {
		var err error
		token, err = planeclient.IngestionBearer("social-core", c.tenantOrganizationID, c.tenantBearerTokenFile)
		if err != nil {
			return err
		}
	}
	req.Header.Set("Authorization", "Bearer "+token)
	req.Header.Set("Accept", "application/json")
	return nil
}

func (c *Client) do(req *http.Request, target any) error {
	if err := c.addTenantHeaders(req); err != nil {
		return err
	}
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
	// "meta" is the unified Meta connection (Facebook Pages + Instagram +
	// WhatsApp + Meta Ads in one grant); the four legacy keys stay accepted for
	// pre-consolidation connections. Ads readiness is expressed via the
	// social.ads.manage capability carried on the account, not a separate
	// provider.
	case "linkedin", "x", "meta", "instagram", "facebook", "whatsapp", "meta-ads", "tiktok", "snapchat":
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
	case "meta-business", "meta-suite", "facebook-business", "meta-unified":
		return "meta"
	case "meta_ads", "facebook-ads", "facebook-marketing", "meta-marketing":
		return "meta-ads"
	case "whatsapp-business", "whatsapp-cloud":
		return "whatsapp"
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
