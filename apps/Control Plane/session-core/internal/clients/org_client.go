package clients

import (
	"context"
	"encoding/json"
	"fmt"
	"net/http"
	"time"
)

// OrgMember mirrors the org-core member record.
type OrgMember struct {
	ID     string `json:"id"`
	OrgID  string `json:"org_id"`
	UserID string `json:"user_id"`
	Role   string `json:"role"`
	Status string `json:"status"`
}

type orgMembersResponse struct {
	Members []OrgMember `json:"members"`
	Count   int         `json:"count"`
}

// OrgClient calls org-core to validate membership.
type OrgClient struct {
	baseURL    string
	httpClient *http.Client
}

// NewOrgClient returns nil if baseURL is empty (disabled).
func NewOrgClient(baseURL string) *OrgClient {
	if baseURL == "" {
		return nil
	}
	return &OrgClient{
		baseURL: baseURL,
		httpClient: &http.Client{
			Timeout: 5 * time.Second,
		},
	}
}

// ValidateMembership fetches the member list for orgID and returns the role
// of userID if their status is "active". Returns an error if not found or not active.
func (c *OrgClient) ValidateMembership(ctx context.Context, orgID, userID string) (string, error) {
	url := fmt.Sprintf("%s/orgs/%s/members", c.baseURL, orgID)

	req, err := http.NewRequestWithContext(ctx, http.MethodGet, url, nil)
	if err != nil {
		return "", fmt.Errorf("building org membership request: %w", err)
	}

	resp, err := c.httpClient.Do(req)
	if err != nil {
		return "", fmt.Errorf("calling org-core: %w", err)
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		return "", fmt.Errorf("org-core returned status %d for org %s", resp.StatusCode, orgID)
	}

	var body orgMembersResponse
	if err := json.NewDecoder(resp.Body).Decode(&body); err != nil {
		return "", fmt.Errorf("decoding org members response: %w", err)
	}

	for _, m := range body.Members {
		if m.UserID == userID && m.Status == "active" {
			return m.Role, nil
		}
	}

	return "", fmt.Errorf("user %s is not an active member of org %s", userID, orgID)
}

// Organization mirrors org-core's GET /orgs/{id} response (subset surfaced by
// the Control Session aggregator, G10).
type Organization struct {
	ID       string `json:"id"`
	Name     string `json:"name,omitempty"`
	Plan     string `json:"plan,omitempty"`
	TenantID string `json:"tenantId,omitempty"`
}

// GetOrganization returns org-core's organization record. A 404 returns
// (nil, nil) so the caller can decide whether to treat "no org yet" as an
// onboarding state vs an error.
func (c *OrgClient) GetOrganization(ctx context.Context, orgID string) (*Organization, error) {
	url := fmt.Sprintf("%s/orgs/%s", c.baseURL, orgID)
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, url, nil)
	if err != nil {
		return nil, fmt.Errorf("org-client: build organization request: %w", err)
	}
	resp, err := c.httpClient.Do(req)
	if err != nil {
		return nil, fmt.Errorf("org-client: call org-core: %w", err)
	}
	defer resp.Body.Close()

	if resp.StatusCode == http.StatusNotFound {
		return nil, nil
	}
	if resp.StatusCode != http.StatusOK {
		return nil, fmt.Errorf("org-client: organization returned %d", resp.StatusCode)
	}
	var out Organization
	if err := json.NewDecoder(resp.Body).Decode(&out); err != nil {
		return nil, fmt.Errorf("org-client: decode organization: %w", err)
	}
	return &out, nil
}

// Entitlement is one row from org-core's GET /orgs/{id}/entitlements list.
type Entitlement struct {
	Feature string `json:"feature"`
	Enabled bool   `json:"enabled"`
}

type entitlementsResponse struct {
	Entitlements []Entitlement `json:"entitlements"`
}

// GetEntitlements returns org-core's entitlement list for orgID. A 404 returns
// (nil, nil) — same convention as GetOrganization.
func (c *OrgClient) GetEntitlements(ctx context.Context, orgID string) ([]Entitlement, error) {
	url := fmt.Sprintf("%s/orgs/%s/entitlements", c.baseURL, orgID)
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, url, nil)
	if err != nil {
		return nil, fmt.Errorf("org-client: build entitlements request: %w", err)
	}
	resp, err := c.httpClient.Do(req)
	if err != nil {
		return nil, fmt.Errorf("org-client: call org-core entitlements: %w", err)
	}
	defer resp.Body.Close()

	if resp.StatusCode == http.StatusNotFound {
		return nil, nil
	}
	if resp.StatusCode != http.StatusOK {
		return nil, fmt.Errorf("org-client: entitlements returned %d", resp.StatusCode)
	}
	var body entitlementsResponse
	if err := json.NewDecoder(resp.Body).Decode(&body); err != nil {
		return nil, fmt.Errorf("org-client: decode entitlements: %w", err)
	}
	return body.Entitlements, nil
}
