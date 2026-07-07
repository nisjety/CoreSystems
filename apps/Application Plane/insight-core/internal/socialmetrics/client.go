// Package socialmetrics is insight-core's outbound client to social-core's
// metrics read endpoint. social-core's metrics.snapshotted lifecycle event
// (which the metric subscriber consumes off NATS) intentionally carries only
// a summary count to keep the event payload small — the real values are
// fetched here on demand, org+account+date scoped.
package socialmetrics

import (
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"strings"
	"time"
)

// Metric mirrors social-core's ProviderMetric wire shape
// (internal/social/types.go — snake_case tags).
type Metric struct {
	OrgID        string         `json:"org_id"`
	AccountID    string         `json:"account_id"`
	ConnectionID string         `json:"connection_id,omitempty"`
	ProviderKey  string         `json:"provider_key"`
	MetricName   string         `json:"metric_name"`
	MetricValue  float64        `json:"metric_value"`
	Dimensions   map[string]any `json:"dimensions"`
	SnapshotDate time.Time      `json:"snapshot_date"`
}

type Client struct {
	baseURL    string
	apiKey     string
	httpClient *http.Client
}

func NewClient(baseURL, apiKey string) *Client {
	return &Client{
		baseURL:    strings.TrimRight(strings.TrimSpace(baseURL), "/"),
		apiKey:     strings.TrimSpace(apiKey),
		httpClient: &http.Client{Timeout: 10 * time.Second},
	}
}

// Configured reports whether the client has enough config to make calls.
func (c *Client) Configured() bool {
	return c != nil && c.baseURL != "" && c.apiKey != ""
}

// ListMetrics fetches persisted metric rows for one org, optionally narrowed
// to one account and snapshot date (YYYY-MM-DD; zero time omits the filter).
func (c *Client) ListMetrics(ctx context.Context, orgID, accountID string, snapshotDate time.Time) ([]Metric, error) {
	if !c.Configured() {
		return nil, fmt.Errorf("socialmetrics: client is not configured")
	}
	orgID = strings.TrimSpace(orgID)
	if orgID == "" {
		return nil, fmt.Errorf("socialmetrics: org_id is required")
	}

	query := url.Values{}
	query.Set("accountId", accountID)
	if !snapshotDate.IsZero() {
		query.Set("snapshotDate", snapshotDate.Format("2006-01-02"))
	}
	reqURL := fmt.Sprintf("%s/api/v1/social/metrics?%s", c.baseURL, query.Encode())

	req, err := http.NewRequestWithContext(ctx, http.MethodGet, reqURL, nil)
	if err != nil {
		return nil, fmt.Errorf("socialmetrics: build request: %w", err)
	}
	req.Header.Set("x-internal-api-key", c.apiKey)
	req.Header.Set("x-org-id", orgID)

	resp, err := c.httpClient.Do(req)
	if err != nil {
		return nil, fmt.Errorf("socialmetrics: call social-core: %w", err)
	}
	defer resp.Body.Close()

	body, _ := io.ReadAll(io.LimitReader(resp.Body, 2<<20))
	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		return nil, fmt.Errorf("socialmetrics: social-core returned status %d: %s", resp.StatusCode, string(body))
	}

	var decoded struct {
		Data []Metric `json:"data"`
	}
	if err := json.Unmarshal(body, &decoded); err != nil {
		return nil, fmt.Errorf("socialmetrics: decode response: %w", err)
	}
	return decoded.Data, nil
}
