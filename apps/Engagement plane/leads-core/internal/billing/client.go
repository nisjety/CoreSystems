// Package billing contains the narrow Billing Core contract Leads Core needs to
// enforce the `leads` add-on at the resource server. It never accepts a caller
// supplied billing decision: the service authenticates directly to Billing Core.
package billing

import (
	"context"
	"encoding/json"
	"fmt"
	"net/http"
	"strings"
	"time"
)

const leadsFeature = "leads"

// EntitlementChecker is intentionally small so HTTP handlers can be tested
// without a network dependency and no billing response leaks into lead data.
type EntitlementChecker interface {
	Allowed(ctx context.Context, orgID, feature string) (bool, error)
}

type Client struct {
	baseURL      string
	serviceID    string
	serviceToken string
	httpClient   *http.Client
}

func NewClient(baseURL, serviceID, serviceToken string) *Client {
	baseURL = strings.TrimRight(strings.TrimSpace(baseURL), "/")
	serviceID = strings.TrimSpace(serviceID)
	serviceToken = strings.TrimSpace(serviceToken)
	if baseURL == "" || serviceID == "" || serviceToken == "" {
		return nil
	}
	return &Client{
		baseURL:      baseURL,
		serviceID:    serviceID,
		serviceToken: serviceToken,
		httpClient:   &http.Client{Timeout: 5 * time.Second},
	}
}

func (c *Client) Allowed(ctx context.Context, orgID, feature string) (bool, error) {
	if c == nil {
		return false, fmt.Errorf("billing client is not configured")
	}
	orgID = strings.TrimSpace(orgID)
	feature = strings.TrimSpace(feature)
	if orgID == "" || feature == "" {
		return false, fmt.Errorf("organization id and feature are required")
	}

	request, err := http.NewRequestWithContext(
		ctx,
		http.MethodGet,
		fmt.Sprintf("%s/api/v1/billing/orgs/%s/entitlements/%s", c.baseURL, orgID, feature),
		nil,
	)
	if err != nil {
		return false, fmt.Errorf("build billing entitlement request: %w", err)
	}
	request.Header.Set("X-Service-Id", c.serviceID)
	request.Header.Set("X-Service-Token", c.serviceToken)

	response, err := c.httpClient.Do(request)
	if err != nil {
		return false, fmt.Errorf("call billing entitlement: %w", err)
	}
	defer response.Body.Close()

	var payload struct {
		Allowed *bool `json:"allowed"`
	}
	if err := json.NewDecoder(response.Body).Decode(&payload); err != nil {
		return false, fmt.Errorf("decode billing entitlement response: %w", err)
	}
	if response.StatusCode == http.StatusPaymentRequired {
		return false, nil
	}
	if response.StatusCode < http.StatusOK || response.StatusCode >= http.StatusMultipleChoices {
		return false, fmt.Errorf("billing entitlement returned status %d", response.StatusCode)
	}
	return payload.Allowed != nil && *payload.Allowed, nil
}

func LeadsFeature() string { return leadsFeature }
