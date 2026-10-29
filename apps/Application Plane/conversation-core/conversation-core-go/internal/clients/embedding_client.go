package clients

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"net/http"
	"strings"
	"time"
)

// EmbeddingClient calls Data Plane v2's embedding-engine-rs synchronous
// EmbedText RPC (POST /v1/embed-text). Short bounded text only — never a
// customer transcript.
type EmbeddingClient struct {
	baseURL    string
	httpClient *http.Client
}

// NewEmbeddingClient returns nil when baseURL is unset, so a caller can fail
// open (corpus builder skips embedding this cycle) rather than fail startup.
func NewEmbeddingClient(baseURL string) *EmbeddingClient {
	baseURL = strings.TrimRight(strings.TrimSpace(baseURL), "/")
	if baseURL == "" {
		return nil
	}
	return &EmbeddingClient{
		baseURL:    baseURL,
		httpClient: &http.Client{Timeout: 15 * time.Second},
	}
}

type embedTextRequest struct {
	OrgID string `json:"org_id"`
	Text  string `json:"text"`
	ZDR   bool   `json:"zdr"`
}

type embedTextResponse struct {
	Vector   []float32 `json:"vector"`
	Provider string    `json:"provider"`
	Model    string    `json:"model"`
}

// EmbedText embeds a short, bounded text for a specific org. zdr must be
// false for any call this client makes — the corpus builder never calls
// this for a ZDR-enabled org in the first place, but the flag is threaded
// through anyway so a defensive misuse here still degrades to the
// provider's own no-retention behavior rather than a silent write.
func (c *EmbeddingClient) EmbedText(ctx context.Context, orgID, text string, zdr bool) ([]float32, error) {
	if c == nil {
		return nil, fmt.Errorf("embedding client is not configured")
	}
	body, err := json.Marshal(embedTextRequest{OrgID: orgID, Text: text, ZDR: zdr})
	if err != nil {
		return nil, fmt.Errorf("encode embed-text request: %w", err)
	}
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, c.baseURL+"/v1/embed-text", bytes.NewReader(body))
	if err != nil {
		return nil, fmt.Errorf("build embed-text request: %w", err)
	}
	req.Header.Set("Content-Type", "application/json")

	resp, err := c.httpClient.Do(req)
	if err != nil {
		return nil, fmt.Errorf("call embed-text: %w", err)
	}
	defer resp.Body.Close()
	if resp.StatusCode != http.StatusOK {
		return nil, fmt.Errorf("embed-text returned status %d", resp.StatusCode)
	}
	var decoded embedTextResponse
	if err := json.NewDecoder(resp.Body).Decode(&decoded); err != nil {
		return nil, fmt.Errorf("decode embed-text response: %w", err)
	}
	return decoded.Vector, nil
}
