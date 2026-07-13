// Package modelplane is shipping-core's client into the Model Plane for
// F5's AI recommendation reasoning: mint a service-to-service JWT from
// auth-core, then call model-gateway's synchronous /v1/invoke with a
// structured_output_schema so the response is parseable JSON rather than
// free text. Same trust chain eval-lab-py uses for its LLM judge — no dev
// bypass, real RS256 tokens, aud=model-gateway.
//
// shipping-core has no per-request org/user identity today (no caller
// forwards one), so calls mint a token for a fixed service identity —
// System / SystemUser below. When a real org/user context starts flowing
// into shipping-core, pass it through Recommend's actor parameter instead
// of relying on the fallback.
package modelplane

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"os"
	"sync"
	"time"
)

// SystemOrgID/SystemUserID identify shipping-core's own service-to-service
// calls when no real org/user context is available. Deliberately
// distinctive strings — greppable if this ever needs to be replaced by a
// forwarded real identity.
const (
	SystemOrgID  = "shipping-core-system"
	SystemUserID = "shipping-core-ai-recommendation"
)

// Config points at the two services this client talks to.
type Config struct {
	AuthCoreURL     string
	ModelGatewayURL string
	InternalAPIKey  string
}

// NewConfigFromEnv reads AUTH_CORE_URL, MODEL_GATEWAY_URL, and
// INTERNAL_API_KEY. A missing InternalAPIKey means token minting will
// fail closed (Configured() reports false) rather than silently sending
// an empty key — auth-core itself refuses issuance without one anyway.
func NewConfigFromEnv() Config {
	return Config{
		AuthCoreURL:     defaultEnv("AUTH_CORE_URL", "http://auth-core:3011"),
		ModelGatewayURL: defaultEnv("MODEL_GATEWAY_URL", "http://model-gateway:8080"),
		InternalAPIKey:  os.Getenv("INTERNAL_API_KEY"),
	}
}

func defaultEnv(key, fallback string) string {
	if v := os.Getenv(key); v != "" {
		return v
	}
	return fallback
}

// Client mints and caches Model Plane tokens and drives /v1/invoke.
type Client struct {
	cfg  Config
	http *http.Client

	mu          sync.Mutex
	cachedToken string
	tokenExp    time.Time
	tokenKey    string // orgID+"|"+userID the cached token was minted for
}

// New builds a Client. Safe for concurrent use.
func New(cfg Config) *Client {
	return &Client{cfg: cfg, http: &http.Client{Timeout: 30 * time.Second}}
}

// Configured reports whether enough configuration exists to mint tokens.
// Callers use this to skip AI recommendation gracefully rather than
// returning a confusing transport error.
func (c *Client) Configured() bool {
	return c != nil && c.cfg.InternalAPIKey != "" && c.cfg.AuthCoreURL != "" && c.cfg.ModelGatewayURL != ""
}

type internalTokenRequest struct {
	OrgID  string `json:"orgId"`
	UserID string `json:"userId"`
	Email  string `json:"email,omitempty"`
}

type internalTokenResponse struct {
	Token            string `json:"token"`
	ExpiresInSeconds int    `json:"expiresInSeconds"`
}

// token returns a cached, still-valid token for (orgID, userID), minting a
// fresh one when absent, for a different identity, or within 60s of
// expiry.
func (c *Client) token(ctx context.Context, orgID, userID string) (string, error) {
	key := orgID + "|" + userID

	c.mu.Lock()
	if c.cachedToken != "" && c.tokenKey == key && time.Until(c.tokenExp) > 60*time.Second {
		tok := c.cachedToken
		c.mu.Unlock()
		return tok, nil
	}
	c.mu.Unlock()

	body, err := json.Marshal(internalTokenRequest{
		OrgID: orgID, UserID: userID, Email: userID + "@shipping-core.internal.velion",
	})
	if err != nil {
		return "", fmt.Errorf("modelplane: encode token request: %w", err)
	}
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, c.cfg.AuthCoreURL+"/api/model-plane/internal-token", bytes.NewReader(body))
	if err != nil {
		return "", fmt.Errorf("modelplane: build token request: %w", err)
	}
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("X-Internal-Api-Key", c.cfg.InternalAPIKey)

	resp, err := c.http.Do(req)
	if err != nil {
		return "", fmt.Errorf("modelplane: token request failed: %w", err)
	}
	defer resp.Body.Close()
	raw, err := io.ReadAll(io.LimitReader(resp.Body, 1<<16))
	if err != nil {
		return "", fmt.Errorf("modelplane: read token response: %w", err)
	}
	if resp.StatusCode != http.StatusOK && resp.StatusCode != http.StatusCreated {
		return "", fmt.Errorf("modelplane: token issuance returned %d: %s", resp.StatusCode, raw)
	}
	var parsed internalTokenResponse
	if err := json.Unmarshal(raw, &parsed); err != nil {
		return "", fmt.Errorf("modelplane: decode token response: %w", err)
	}
	if parsed.Token == "" {
		return "", fmt.Errorf("modelplane: token response missing token field")
	}

	c.mu.Lock()
	c.cachedToken = parsed.Token
	c.tokenKey = key
	ttl := time.Duration(parsed.ExpiresInSeconds) * time.Second
	if ttl <= 0 {
		ttl = 5 * time.Minute // conservative fallback if the field is ever absent
	}
	c.tokenExp = time.Now().Add(ttl)
	c.mu.Unlock()

	return parsed.Token, nil
}

// InvokeRequest mirrors model-gateway's InvokeRequest (only the fields
// this client uses — the gateway defaults the rest).
type InvokeRequest struct {
	OrgID                  string
	UserID                 string
	Content                string
	Model                  string
	StructuredOutputSchema string
}

// InvokeResponse mirrors model-gateway's InvokeResponse.
type InvokeResponse struct {
	RequestID string `json:"request_id"`
	Content   string `json:"content"`
	ModelUsed string `json:"model_used"`
}

// Invoke mints a token for (req.OrgID, req.UserID) and calls model-gateway's
// synchronous /v1/invoke. When req.StructuredOutputSchema is set, callers
// should expect Content to be JSON matching that schema — but the model is
// a model, not a compiler, so callers MUST handle a parse failure honestly
// (retry, skip, or surface an error) rather than assume it always holds.
func (c *Client) Invoke(ctx context.Context, req InvokeRequest) (InvokeResponse, error) {
	if !c.Configured() {
		return InvokeResponse{}, fmt.Errorf("modelplane: not configured (missing INTERNAL_API_KEY/AUTH_CORE_URL/MODEL_GATEWAY_URL)")
	}
	orgID, userID := req.OrgID, req.UserID
	if orgID == "" {
		orgID = SystemOrgID
	}
	if userID == "" {
		userID = SystemUserID
	}

	tok, err := c.token(ctx, orgID, userID)
	if err != nil {
		return InvokeResponse{}, err
	}

	body, err := json.Marshal(map[string]any{
		"content":                  req.Content,
		"model":                    req.Model,
		"structured_output_schema": req.StructuredOutputSchema,
	})
	if err != nil {
		return InvokeResponse{}, fmt.Errorf("modelplane: encode invoke request: %w", err)
	}
	httpReq, err := http.NewRequestWithContext(ctx, http.MethodPost, c.cfg.ModelGatewayURL+"/v1/invoke", bytes.NewReader(body))
	if err != nil {
		return InvokeResponse{}, fmt.Errorf("modelplane: build invoke request: %w", err)
	}
	httpReq.Header.Set("Content-Type", "application/json")
	httpReq.Header.Set("Authorization", "Bearer "+tok)

	resp, err := c.http.Do(httpReq)
	if err != nil {
		return InvokeResponse{}, fmt.Errorf("modelplane: invoke request failed: %w", err)
	}
	defer resp.Body.Close()
	raw, err := io.ReadAll(io.LimitReader(resp.Body, 1<<20))
	if err != nil {
		return InvokeResponse{}, fmt.Errorf("modelplane: read invoke response: %w", err)
	}
	if resp.StatusCode != http.StatusOK {
		return InvokeResponse{}, fmt.Errorf("modelplane: invoke returned %d: %s", resp.StatusCode, raw)
	}
	var parsed InvokeResponse
	if err := json.Unmarshal(raw, &parsed); err != nil {
		return InvokeResponse{}, fmt.Errorf("modelplane: decode invoke response: %w", err)
	}
	return parsed, nil
}
