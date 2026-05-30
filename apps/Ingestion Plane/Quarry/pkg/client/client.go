package client

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

type Client struct {
	baseURL    string
	apiKey     string
	httpClient *http.Client
}

type Option func(*Client)

func WithHTTPClient(httpClient *http.Client) Option {
	return func(c *Client) {
		if httpClient != nil {
			c.httpClient = httpClient
		}
	}
}

func WithTimeout(timeout time.Duration) Option {
	return func(c *Client) {
		if timeout > 0 {
			c.httpClient.Timeout = timeout
		}
	}
}

func New(baseURL, apiKey string, opts ...Option) *Client {
	trimmedBaseURL := strings.TrimRight(strings.TrimSpace(baseURL), "/")
	if trimmedBaseURL == "" {
		trimmedBaseURL = "http://localhost:8080"
	}
	client := &Client{
		baseURL: trimmedBaseURL,
		apiKey:  strings.TrimSpace(apiKey),
		httpClient: &http.Client{
			Timeout: 30 * time.Second,
		},
	}
	for _, opt := range opts {
		if opt != nil {
			opt(client)
		}
	}
	return client
}

func (c *Client) StartCrawl(ctx context.Context, req CrawlRequest) (*AsyncCreateResponse, error) {
	return c.postAsyncCreate(ctx, "/v1/crawl", req)
}

func (c *Client) StartSearch(ctx context.Context, req SearchRequest) (*AsyncCreateResponse, error) {
	return c.postAsyncCreate(ctx, "/v1/search", req)
}

func (c *Client) StartExtract(ctx context.Context, req ExtractRequest) (*AsyncCreateResponse, error) {
	return c.postAsyncCreate(ctx, "/v1/extract", req)
}

func (c *Client) StartResearch(ctx context.Context, req ResearchRequest) (*AsyncCreateResponse, error) {
	return c.postAsyncCreate(ctx, "/v1/research", req)
}

func (c *Client) postAsyncCreate(ctx context.Context, path string, payload any) (*AsyncCreateResponse, error) {
	if c == nil {
		return nil, fmt.Errorf("client is nil")
	}
	body, err := json.Marshal(payload)
	if err != nil {
		return nil, fmt.Errorf("marshal request: %w", err)
	}
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, c.baseURL+path, bytes.NewReader(body))
	if err != nil {
		return nil, fmt.Errorf("create request: %w", err)
	}
	req.Header.Set("Content-Type", "application/json")
	if c.apiKey != "" {
		req.Header.Set("Authorization", "Bearer "+c.apiKey)
	}
	resp, err := c.httpClient.Do(req)
	if err != nil {
		return nil, fmt.Errorf("request failed: %w", err)
	}
	defer resp.Body.Close()

	respBody, err := io.ReadAll(resp.Body)
	if err != nil {
		return nil, fmt.Errorf("read response: %w", err)
	}
	if resp.StatusCode < http.StatusOK || resp.StatusCode >= http.StatusMultipleChoices {
		return nil, fmt.Errorf("request failed with status %d: %s", resp.StatusCode, strings.TrimSpace(string(respBody)))
	}

	var decoded AsyncCreateResponse
	if err := json.Unmarshal(respBody, &decoded); err != nil {
		return nil, fmt.Errorf("decode response: %w", err)
	}
	return &decoded, nil
}
