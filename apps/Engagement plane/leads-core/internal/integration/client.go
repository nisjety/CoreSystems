// Package integration is leads-core's client for integration-corev2 (the
// actions gateway). It mirrors social-core's internal/integration client:
// same {success,data,error} envelope, with an org-scoped Bearer token.
// leads-core never sees provider OAuth tokens — token resolution and the
// provider HTTP call happen inside integration-corev2's executor.
package integration

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net"
	"net/http"
	"net/url"
	"strings"
	"time"

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

// Connection is the subset of integration-corev2's connection JSON that the
// provider-lead sync needs. Capabilities gate which connections are eligible
// (social.leads.read); ProviderContext may carry the lead-forms owner URN.
type Connection struct {
	ID                string            `json:"id"`
	ProviderKey       string            `json:"providerKey"`
	OrganizationID    string            `json:"organizationId"`
	Status            string            `json:"status"`
	DisplayName       string            `json:"displayName"`
	ProviderAccountID string            `json:"providerAccountId"`
	ProviderContext   map[string]string `json:"providerContext"`
	Capabilities      []string          `json:"capabilities"`
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
	Connections []Connection `json:"connections"`
}

type actionPayload struct {
	Action struct {
		ProviderKey string          `json:"providerKey"`
		Operation   string          `json:"operation"`
		Result      json.RawMessage `json:"result"`
	} `json:"action"`
}

func NewClient(cfg Config) *Client {
	httpClient := http.Client{Timeout: 30 * time.Second}
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

// ListConnections lists only the configured tenant's connections. An empty
// orgID means that tenant, never a cross-organization listing.
func (c *Client) ListConnections(ctx context.Context, orgID, providerKey string) ([]Connection, error) {
	if err := c.ensureConfigured(); err != nil {
		return nil, err
	}
	if strings.TrimSpace(orgID) != "" && strings.TrimSpace(orgID) != c.tenantOrganizationID {
		return nil, fmt.Errorf("organization does not match configured integration-core tenant")
	}
	query := url.Values{}
	query.Set("organizationId", c.tenantOrganizationID)
	if strings.TrimSpace(providerKey) != "" {
		query.Set("providerKey", strings.TrimSpace(providerKey))
	}
	endpoint := c.baseURL + "/api/v1/connections"
	if encoded := query.Encode(); encoded != "" {
		endpoint += "?" + encoded
	}
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, endpoint, nil)
	if err != nil {
		return nil, err
	}
	var payload connectionsPayload
	if err := c.do(req, &payload); err != nil {
		return nil, fmt.Errorf("list connections: %w", err)
	}
	connections := make([]Connection, 0, len(payload.Connections))
	for _, connection := range payload.Connections {
		if strings.TrimSpace(connection.OrganizationID) == c.tenantOrganizationID {
			connections = append(connections, connection)
		}
	}
	return connections, nil
}

// ExecuteAction calls POST /api/v1/actions/execute and returns the raw
// provider result (the executor's pass-through of the provider response).
func (c *Client) ExecuteAction(ctx context.Context, connectionID, operation string, params map[string]any) (json.RawMessage, error) {
	if err := c.ensureConfigured(); err != nil {
		return nil, err
	}
	connectionID = strings.TrimSpace(connectionID)
	operation = strings.TrimSpace(operation)
	if connectionID == "" {
		return nil, fmt.Errorf("connection id is required")
	}
	if operation == "" {
		return nil, fmt.Errorf("operation is required")
	}
	body, err := json.Marshal(map[string]any{
		"connectionId": connectionID,
		"operation":    operation,
		"params":       params,
	})
	if err != nil {
		return nil, err
	}
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, c.baseURL+"/api/v1/actions/execute", bytes.NewReader(body))
	if err != nil {
		return nil, err
	}
	req.Header.Set("Content-Type", "application/json")
	var payload actionPayload
	if err := c.do(req, &payload); err != nil {
		return nil, fmt.Errorf("execute %s: %w", operation, err)
	}
	return payload.Action.Result, nil
}

func (c *Client) ensureConfigured() error {
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
		token, err = planeclient.IngestionBearer("leads-core", c.tenantOrganizationID, c.tenantBearerTokenFile)
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
	body := io.LimitReader(resp.Body, 4<<20)
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
