// Package dataplane pushes factual, non-fabricated evidence into Data
// Plane v2's documents-api so shipping history becomes discoverable via
// Velion's knowledge/retrieval surface (citations, chat grounding) — the
// "Ingestion Plane persists durable knowledge through Data Plane
// contracts only" architecture rule. Ports integration-corev2's
// DataPlaneDocumentsClient (internal/handoff/dataplane.go) verbatim: same
// route, same headers, same Configured()-gated nil-safety. Each document
// pushed here is a real, derived fact (booking ref, carrier, delivered
// date vs estimate) — never an LLM-generated or placeholder summary; see
// that file's own warning about not calling this with synthetic content.
package dataplane

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"os"
	"strings"
	"sync"
	"time"
)

// DocumentRequest mirrors documents-api-go's POST /v1/documents body.
type DocumentRequest struct {
	OrgID          string         `json:"org_id"`
	Source         string         `json:"source"`
	Type           string         `json:"type"`
	Title          string         `json:"title"`
	Content        string         `json:"content,omitempty"`
	Metadata       map[string]any `json:"metadata,omitempty"`
	CreatedBy      string         `json:"created_by,omitempty"`
	IdempotencyKey string         `json:"idempotency_key,omitempty"`
	IngestPolicy   *IngestPolicy  `json:"ingest_policy,omitempty"`
}

type IngestPolicy struct {
	ZDRMode       string `json:"zdr_mode"`
	EphemeralOnly bool   `json:"ephemeral_only"`
}

// Document is documents-api-go's response shape.
type Document struct {
	ID         string `json:"id,omitempty"`
	DocumentID string `json:"document_id,omitempty"`
}

// Config points at Data Plane v2's documents-api.
type Config struct {
	BaseURL           string
	AuthCoreURL       string
	ServiceID         string
	ServiceCredential string
}

// NewConfigFromEnv reads the Data Plane endpoint plus a dedicated Auth Core
// service principal used to mint short-lived org/scoped credentials.
func NewConfigFromEnv() Config {
	return Config{
		BaseURL:           os.Getenv("DATA_PLANE_DOCUMENTS_URL"),
		AuthCoreURL:       os.Getenv("AUTH_CORE_URL"),
		ServiceID:         envOr("INGESTION_SERVICE_ID", "shipping-core"),
		ServiceCredential: firstEnv("SHIPPING_SERVICE_API_KEY", "INGESTION_SERVICE_API_KEY"),
	}
}

// Client pushes documents into Data Plane v2.
type Client struct {
	cfg    Config
	http   *http.Client
	mu     sync.Mutex
	tokens map[string]cachedToken
}

type cachedToken struct {
	value   string
	expires time.Time
}

func New(cfg Config) *Client {
	return &Client{cfg: cfg, http: &http.Client{Timeout: 15 * time.Second}, tokens: make(map[string]cachedToken)}
}

// Configured reports whether enough configuration exists to push
// documents. Callers skip this integration silently when false, exactly
// like finspo-worker does for the same client shape in integration-corev2.
func (c *Client) Configured() bool {
	return c != nil && strings.TrimSpace(c.cfg.BaseURL) != "" && strings.TrimSpace(c.cfg.AuthCoreURL) != "" && strings.TrimSpace(c.cfg.ServiceID) != "" && strings.TrimSpace(c.cfg.ServiceCredential) != ""
}

func (c *Client) dataPlaneToken(ctx context.Context, orgID string) (string, error) {
	c.mu.Lock()
	defer c.mu.Unlock()
	if token, ok := c.tokens[orgID]; ok && time.Now().Before(token.expires) {
		return token.value, nil
	}
	body, _ := json.Marshal(map[string]any{
		"orgId": orgID, "scopes": []string{"documents:write"},
		"reason": "shipping-core delivery evidence",
	})
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, strings.TrimRight(c.cfg.AuthCoreURL, "/")+"/api/data-plane/internal-token", bytes.NewReader(body))
	if err != nil {
		return "", fmt.Errorf("dataplane: build token request: %w", err)
	}
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("X-Service-ID", c.cfg.ServiceID)
	req.Header.Set("X-Service-API-Key", c.cfg.ServiceCredential)
	resp, err := c.http.Do(req)
	if err != nil {
		return "", fmt.Errorf("dataplane: token request failed: %w", err)
	}
	defer resp.Body.Close()
	raw, err := io.ReadAll(io.LimitReader(resp.Body, 1<<20))
	if err != nil {
		return "", fmt.Errorf("dataplane: read token response: %w", err)
	}
	if resp.StatusCode != http.StatusOK {
		return "", fmt.Errorf("dataplane: auth core returned %d", resp.StatusCode)
	}
	var parsed struct {
		Token string `json:"token"`
	}
	if err := json.Unmarshal(raw, &parsed); err != nil || strings.TrimSpace(parsed.Token) == "" {
		return "", fmt.Errorf("dataplane: invalid token response")
	}
	c.tokens[orgID] = cachedToken{value: parsed.Token, expires: time.Now().Add(4 * time.Minute)}
	return parsed.Token, nil
}

// CreateDocument POSTs to documents-api-go's real route (no "/internal"
// prefix — see integration-corev2's own correction of that historical
// mistake). Requires input.Content to be genuinely non-empty factual text;
// see the package doc for why this must never be fabricated.
func (c *Client) CreateDocument(ctx context.Context, input DocumentRequest) (Document, error) {
	if !c.Configured() {
		return Document{}, fmt.Errorf("dataplane: endpoint or scoped service credential is not configured")
	}
	if input.Content == "" {
		return Document{}, fmt.Errorf("dataplane: content is required")
	}
	body, err := json.Marshal(input)
	if err != nil {
		return Document{}, fmt.Errorf("dataplane: encode document: %w", err)
	}
	token, err := c.dataPlaneToken(ctx, input.OrgID)
	if err != nil {
		return Document{}, err
	}
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, strings.TrimRight(c.cfg.BaseURL, "/")+"/v1/documents", bytes.NewReader(body))
	if err != nil {
		return Document{}, fmt.Errorf("dataplane: build request: %w", err)
	}
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("Authorization", "Bearer "+token)
	req.Header.Set("X-Org-ID", input.OrgID)

	resp, err := c.http.Do(req)
	if err != nil {
		return Document{}, fmt.Errorf("dataplane: request failed: %w", err)
	}
	defer resp.Body.Close()
	raw, err := io.ReadAll(io.LimitReader(resp.Body, 1<<16))
	if err != nil {
		return Document{}, fmt.Errorf("dataplane: read response: %w", err)
	}
	if resp.StatusCode != http.StatusOK && resp.StatusCode != http.StatusCreated {
		return Document{}, fmt.Errorf("dataplane: documents returned %d: %s", resp.StatusCode, raw)
	}
	var parsed Document
	if err := json.Unmarshal(raw, &parsed); err != nil {
		return Document{}, fmt.Errorf("dataplane: decode response: %w", err)
	}
	return parsed, nil
}

func envOr(key, fallback string) string {
	if value := strings.TrimSpace(os.Getenv(key)); value != "" {
		return value
	}
	return fallback
}

func firstEnv(keys ...string) string {
	for _, key := range keys {
		if value := strings.TrimSpace(os.Getenv(key)); value != "" {
			return value
		}
	}
	return ""
}
