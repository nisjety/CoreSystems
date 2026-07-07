// Package integration is leads-core's client for integration-corev2 (the
// actions gateway). It mirrors social-core's internal/integration client:
// same {success,data,error} envelope, same X-Internal-API-Key header.
// leads-core never sees provider OAuth tokens — token resolution and the
// provider HTTP call happen inside integration-corev2's executor.
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
	httpClient := cfg.HTTPClient
	if httpClient == nil {
		httpClient = &http.Client{Timeout: 30 * time.Second}
	}
	return &Client{
		baseURL:        strings.TrimRight(strings.TrimSpace(cfg.BaseURL), "/"),
		internalAPIKey: strings.TrimSpace(cfg.InternalAPIKey),
		httpClient:     httpClient,
	}
}

// ListConnections lists connections, optionally filtered by org and provider.
// An empty orgID (internal call) lists across all orgs — that is how the
// interval worker discovers which orgs have a LinkedIn connection at all.
func (c *Client) ListConnections(ctx context.Context, orgID, providerKey string) ([]Connection, error) {
	if err := c.ensureConfigured(); err != nil {
		return nil, err
	}
	query := url.Values{}
	if strings.TrimSpace(orgID) != "" {
		query.Set("organizationId", strings.TrimSpace(orgID))
	}
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
	c.addInternalHeaders(req)
	var payload connectionsPayload
	if err := c.do(req, &payload); err != nil {
		return nil, fmt.Errorf("list connections: %w", err)
	}
	return payload.Connections, nil
}

// ExecuteAction calls POST /api/v1/actions/execute and returns the raw
// provider result (the executor's pass-through of the provider response).
func (c *Client) ExecuteAction(ctx context.Context, connectionID, operation string, params map[string]any) (json.RawMessage, error) {
	if err := c.ensureConfigured(); err != nil {
		return nil, err
	}
	body, err := json.Marshal(map[string]any{
		"connectionId": strings.TrimSpace(connectionID),
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
	c.addInternalHeaders(req)
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
