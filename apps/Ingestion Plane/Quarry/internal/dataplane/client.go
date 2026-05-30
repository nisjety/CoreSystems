package dataplane

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"strings"
	"time"
)

// Client communicates with the data plane (documents-service).
type Client struct {
	baseURL          string
	retrievalBaseURL string
	internalAPIKey   string
	httpClient       *http.Client
}

// NewClient creates a new data plane client pointing to documents-service.
func NewClient(baseURL string) *Client {
	if baseURL == "" {
		baseURL = "http://data-documents-service:8001"
	}
	return &Client{
		baseURL:          baseURL,
		retrievalBaseURL: baseURL,
		httpClient: &http.Client{
			Timeout: 30 * time.Second,
		},
	}
}

func (c *Client) SetRetrievalBaseURL(baseURL string) {
	if c == nil {
		return
	}
	trimmed := strings.TrimSpace(baseURL)
	if trimmed == "" {
		return
	}
	c.retrievalBaseURL = strings.TrimRight(trimmed, "/")
}

func (c *Client) SetInternalAPIKey(key string) {
	if c == nil {
		return
	}
	c.internalAPIKey = strings.TrimSpace(key)
}

func (c *Client) RetrievalConfigured() bool {
	return c != nil && strings.TrimSpace(c.internalAPIKey) != ""
}

// DocumentCreateRequest matches the data plane documents-service API.
type DocumentCreateRequest struct {
	OrgID    string                 `json:"org_id"`
	Source   string                 `json:"source"` // e.g., "web", "quarry"
	Type     string                 `json:"type"`   // e.g., "product", "content"
	Title    string                 `json:"title"`
	Content  string                 `json:"content"`
	Metadata map[string]interface{} `json:"metadata,omitempty"`
}

// DocumentResponse is the response from documents-service.
type DocumentResponse struct {
	DocumentID string                 `json:"document_id"`
	OrgID      string                 `json:"org_id"`
	Source     string                 `json:"source"`
	Type       string                 `json:"type"`
	Title      string                 `json:"title"`
	Status     string                 `json:"status"`
	Metadata   map[string]interface{} `json:"metadata"`
	CreatedAt  time.Time              `json:"created_at"`
	UpdatedAt  time.Time              `json:"updated_at"`
	Content    string                 `json:"content,omitempty"`
}

type RetrieveFilters struct {
	DocumentTypes []string `json:"document_types,omitempty"`
	Departments   []string `json:"departments,omitempty"`
	Languages     []string `json:"languages,omitempty"`
	DocumentIDs   []string `json:"document_ids,omitempty"`
	Region        string   `json:"region,omitempty"`
}

type RetrieveRequest struct {
	Query   string          `json:"query"`
	Filters RetrieveFilters `json:"filters,omitempty"`
	TopK    int             `json:"top_k,omitempty"`
	TopN    int             `json:"top_n,omitempty"`
}

type RetrieveFact struct {
	KnowledgeID string                 `json:"knowledge_id"`
	DocumentID  string                 `json:"document_id"`
	Text        string                 `json:"text"`
	Score       float64                `json:"score"`
	RerankScore *float64               `json:"rerank_score,omitempty"`
	Metadata    map[string]interface{} `json:"metadata,omitempty"`
}

type RetrieveSource struct {
	DocumentID string `json:"document_id"`
	Title      string `json:"title"`
	Source     string `json:"source"`
	Type       string `json:"type"`
}

type RetrieveResponse struct {
	Facts   []RetrieveFact   `json:"facts"`
	Sources []RetrieveSource `json:"sources"`
	Query   string           `json:"query"`
	OrgID   string           `json:"org_id"`
}

// IngestExtraction sends extracted content to the data plane for document storage and processing.
func (c *Client) IngestExtraction(ctx context.Context, orgID string, req *DocumentCreateRequest) (*DocumentResponse, error) {
	if orgID != "" {
		req.OrgID = orgID
	}
	if req.OrgID == "" {
		return nil, fmt.Errorf("org_id is required")
	}
	if req.Source == "" {
		req.Source = "quarry"
	}

	body, err := json.Marshal(req)
	if err != nil {
		return nil, fmt.Errorf("marshal request: %w", err)
	}

	httpReq, err := http.NewRequestWithContext(ctx, "POST", fmt.Sprintf("%s/internal/v1/documents", c.baseURL), bytes.NewReader(body))
	if err != nil {
		return nil, fmt.Errorf("create request: %w", err)
	}

	httpReq.Header.Set("Content-Type", "application/json")
	httpReq.Header.Set("X-Internal-Api-Key", c.internalAPIKey)
	httpReq.Header.Set("X-Org-Id", req.OrgID)
	httpReq.Header.Set("X-Service-Name", "quarry")

	resp, err := c.httpClient.Do(httpReq)
	if err != nil {
		return nil, fmt.Errorf("request failed: %w", err)
	}
	defer resp.Body.Close()

	respBody, err := io.ReadAll(resp.Body)
	if err != nil {
		return nil, fmt.Errorf("read response: %w", err)
	}

	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		return nil, fmt.Errorf("dataplane error (status %d): %s", resp.StatusCode, string(respBody))
	}

	var docResp DocumentResponse
	if err := json.Unmarshal(respBody, &docResp); err != nil {
		return nil, fmt.Errorf("unmarshal response: %w", err)
	}

	return &docResp, nil
}

func (c *Client) Retrieve(ctx context.Context, orgID string, req *RetrieveRequest) (*RetrieveResponse, error) {
	if c == nil {
		return nil, fmt.Errorf("dataplane client is not initialized")
	}
	if strings.TrimSpace(orgID) == "" {
		return nil, fmt.Errorf("org_id is required")
	}
	if strings.TrimSpace(c.internalAPIKey) == "" {
		return nil, fmt.Errorf("dataplane retrieval is not configured")
	}
	if req == nil {
		return nil, fmt.Errorf("request is required")
	}
	if strings.TrimSpace(req.Query) == "" {
		return nil, fmt.Errorf("query is required")
	}

	body, err := json.Marshal(req)
	if err != nil {
		return nil, fmt.Errorf("marshal request: %w", err)
	}

	baseURL := c.retrievalBaseURL
	if strings.TrimSpace(baseURL) == "" {
		baseURL = c.baseURL
	}
	httpReq, err := http.NewRequestWithContext(ctx, http.MethodPost, fmt.Sprintf("%s/v1/retrieve", strings.TrimRight(baseURL, "/")), bytes.NewReader(body))
	if err != nil {
		return nil, fmt.Errorf("create request: %w", err)
	}
	httpReq.Header.Set("Content-Type", "application/json")
	httpReq.Header.Set("X-Internal-Api-Key", c.internalAPIKey)
	httpReq.Header.Set("X-Org-Id", orgID)
	httpReq.Header.Set("X-Service-Name", "quarry")

	resp, err := c.httpClient.Do(httpReq)
	if err != nil {
		return nil, fmt.Errorf("request failed: %w", err)
	}
	defer resp.Body.Close()

	respBody, err := io.ReadAll(resp.Body)
	if err != nil {
		return nil, fmt.Errorf("read response: %w", err)
	}
	if resp.StatusCode < http.StatusOK || resp.StatusCode >= http.StatusMultipleChoices {
		return nil, fmt.Errorf("dataplane retrieval error (status %d): %s", resp.StatusCode, string(respBody))
	}

	var retrievalResp RetrieveResponse
	if err := json.Unmarshal(respBody, &retrievalResp); err != nil {
		return nil, fmt.Errorf("unmarshal response: %w", err)
	}

	return &retrievalResp, nil
}

// Health checks if the documents-service is reachable and healthy.
func (c *Client) Health(ctx context.Context) error {
	httpReq, err := http.NewRequestWithContext(ctx, "GET", fmt.Sprintf("%s/health", c.baseURL), nil)
	if err != nil {
		return fmt.Errorf("create health request: %w", err)
	}

	resp, err := c.httpClient.Do(httpReq)
	if err != nil {
		return fmt.Errorf("health check failed: %w", err)
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		return fmt.Errorf("dataplane not healthy (status %d)", resp.StatusCode)
	}

	return nil
}
