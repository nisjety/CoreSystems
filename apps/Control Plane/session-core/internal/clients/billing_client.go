package clients

import (
	"context"
	"encoding/json"
	"fmt"
	"net/http"
	"time"
)

// BillingAccount mirrors billing-core's GET /api/v1/billing/orgs/{orgId}/account
// response (the fields the Control Session aggregator surfaces to velion).
type BillingAccount struct {
	OrgID            string `json:"orgId"`
	Plan             string `json:"plan,omitempty"`
	Status           string `json:"status,omitempty"`
	SubscriptionID   string `json:"subscriptionId,omitempty"`
	CurrentPeriodEnd string `json:"currentPeriodEnd,omitempty"`
}

// BillingClient calls billing-core. G10: part of the Control Session aggregator.
type BillingClient struct {
	baseURL    string
	apiKey     string
	httpClient *http.Client
}

// NewBillingClient returns nil when baseURL is empty.
func NewBillingClient(baseURL, internalAPIKey string) *BillingClient {
	if baseURL == "" {
		return nil
	}
	return &BillingClient{
		baseURL:    baseURL,
		apiKey:     internalAPIKey,
		httpClient: &http.Client{Timeout: 5 * time.Second},
	}
}

// GetAccount returns the billing account snapshot for orgID. A missing
// account (404 — org has no billing record yet) is not an error; returns nil.
func (c *BillingClient) GetAccount(ctx context.Context, orgID string) (*BillingAccount, error) {
	url := fmt.Sprintf("%s/api/v1/billing/orgs/%s/account", c.baseURL, orgID)
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, url, nil)
	if err != nil {
		return nil, fmt.Errorf("billing-client: build request: %w", err)
	}
	if c.apiKey != "" {
		req.Header.Set("X-Internal-Api-Key", c.apiKey)
	}
	req.Header.Set("Accept", "application/json")

	resp, err := c.httpClient.Do(req)
	if err != nil {
		return nil, fmt.Errorf("billing-client: call billing-core: %w", err)
	}
	defer resp.Body.Close()

	if resp.StatusCode == http.StatusNotFound {
		return nil, nil
	}
	if resp.StatusCode != http.StatusOK {
		return nil, fmt.Errorf("billing-client: account returned %d", resp.StatusCode)
	}

	var out BillingAccount
	if err := json.NewDecoder(resp.Body).Decode(&out); err != nil {
		return nil, fmt.Errorf("billing-client: decode account: %w", err)
	}
	if out.OrgID == "" {
		out.OrgID = orgID
	}
	return &out, nil
}
