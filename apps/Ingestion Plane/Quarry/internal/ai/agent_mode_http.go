package ai

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"net/http"
	"strings"
	"time"
)

type AgentModeHTTPClient struct {
	baseURL      string
	apiKey       string
	apiKeyHeader string
	client       *http.Client
}

type AgentModeRequest struct {
	UserID             string                 `json:"user_id"`
	Tier               string                 `json:"tier,omitempty"`
	Objective          string                 `json:"objective"`
	TargetURL          string                 `json:"target_url,omitempty"`
	TargetURLs         []string               `json:"target_urls,omitempty"`
	Schema             string                 `json:"schema,omitempty"`
	Model              string                 `json:"model,omitempty"`
	Context            map[string]interface{} `json:"context,omitempty"`
	MaxSteps           int                    `json:"max_steps,omitempty"`
	EnableWebSearch    bool                   `json:"enable_web_search,omitempty"`
	AllowExternalLinks bool                   `json:"allow_external_links,omitempty"`
	ChangeContext      map[string]interface{} `json:"change_context,omitempty"`
	ScrapedPages       []ScrapedPage          `json:"scraped_pages,omitempty"`
}

type ScrapedPage struct {
	URL     string `json:"url"`
	Content string `json:"content"`
}

type AgentModeResponse struct {
	Success      bool    `json:"success"`
	Content      string  `json:"content"`
	ModelUsed    string  `json:"model_used"`
	Intent       string  `json:"intent"`
	Confidence   float64 `json:"confidence"`
	RequestID    string  `json:"request_id"`
	TotalCostUSD float64 `json:"total_cost_usd"`
	DurationMs   float64 `json:"duration_ms"`
}

func NewAgentModeHTTPClient(baseURL, apiKey, apiKeyHeader string) *AgentModeHTTPClient {
	return &AgentModeHTTPClient{
		baseURL:      strings.TrimRight(strings.TrimSpace(baseURL), "/"),
		apiKey:       strings.TrimSpace(apiKey),
		apiKeyHeader: strings.TrimSpace(apiKeyHeader),
		client: &http.Client{
			Timeout: 30 * time.Second,
		},
	}
}

func (c *AgentModeHTTPClient) Enabled() bool {
	return c != nil && c.baseURL != ""
}

func (c *AgentModeHTTPClient) Run(ctx context.Context, req *AgentModeRequest) (*AgentModeResponse, error) {
	if c == nil || c.baseURL == "" {
		return nil, fmt.Errorf("agent mode client is not configured")
	}
	if req == nil {
		return nil, fmt.Errorf("agent mode request is nil")
	}

	body, err := json.Marshal(req)
	if err != nil {
		return nil, fmt.Errorf("marshal agent mode request: %w", err)
	}

	httpReq, err := http.NewRequestWithContext(ctx, http.MethodPost, c.baseURL+"/v1/agent/run", bytes.NewBuffer(body))
	if err != nil {
		return nil, fmt.Errorf("build agent mode request: %w", err)
	}
	httpReq.Header.Set("Content-Type", "application/json")
	if c.apiKey != "" {
		header := c.apiKeyHeader
		if header == "" {
			header = "X-API-Key"
		}
		httpReq.Header.Set(header, c.apiKey)
	}

	resp, err := c.client.Do(httpReq)
	if err != nil {
		return nil, fmt.Errorf("call ai-core agent mode: %w", err)
	}
	defer resp.Body.Close()

	var out AgentModeResponse
	if err := json.NewDecoder(resp.Body).Decode(&out); err != nil {
		return nil, fmt.Errorf("decode ai-core agent mode response: %w", err)
	}

	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		return nil, fmt.Errorf("ai-core agent mode returned %d", resp.StatusCode)
	}

	return &out, nil
}
