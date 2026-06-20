// Package userauthz is documents-api's thin client for user-core's per-user
// authorization facade. It resolves the EXPLICIT resource grants a viewer holds
// (resource_grants), which documents-api unions with owner_id/visibility to
// enforce ownership at the source.
//
// Cross-plane rule: the grant store lives in user-core (Control Plane);
// documents-api (Data Plane) NEVER queries it directly — it asks over this HTTP
// facade, authenticated with the shared internal key.
package userauthz

import (
	"context"
	"encoding/json"
	"fmt"
	"net/http"
	"net/url"
	"strings"
	"time"
)

// Client calls user-core's internal authz facade.
type Client struct {
	baseURL     string
	internalKey string
	http        *http.Client
}

// New constructs a Client. baseURL is user-core's root (e.g. http://user-core:8080).
func New(baseURL, internalKey string) *Client {
	return &Client{
		baseURL:     strings.TrimRight(strings.TrimSpace(baseURL), "/"),
		internalKey: strings.TrimSpace(internalKey),
		http:        &http.Client{Timeout: 3 * time.Second},
	}
}

type visibleResponse struct {
	IDs    []string `json:"ids"`
	AllOrg bool     `json:"all_org"`
}

// ListVisibleDocuments returns the document ids explicitly granted to the viewer
// in the org. The caller decides how to handle an error (documents-api fails
// open to owner + org/shared visibility — never leaking, only possibly hiding a
// shared doc until the facade recovers).
func (c *Client) ListVisibleDocuments(ctx context.Context, orgID, userID string) ([]string, error) {
	if c == nil || c.baseURL == "" {
		return nil, fmt.Errorf("userauthz: client not configured")
	}
	q := url.Values{}
	q.Set("org_id", orgID)
	q.Set("subject_id", userID)
	q.Set("resource_type", "document")
	reqURL := c.baseURL + "/api/v1/internal/authz/visible?" + q.Encode()

	req, err := http.NewRequestWithContext(ctx, http.MethodGet, reqURL, nil)
	if err != nil {
		return nil, fmt.Errorf("userauthz: build request: %w", err)
	}
	if c.internalKey != "" {
		req.Header.Set("X-Internal-Api-Key", c.internalKey)
	}

	resp, err := c.http.Do(req)
	if err != nil {
		return nil, fmt.Errorf("userauthz: request failed: %w", err)
	}
	defer resp.Body.Close()
	if resp.StatusCode != http.StatusOK {
		return nil, fmt.Errorf("userauthz: unexpected status %d", resp.StatusCode)
	}

	var out visibleResponse
	if err := json.NewDecoder(resp.Body).Decode(&out); err != nil {
		return nil, fmt.Errorf("userauthz: decode response: %w", err)
	}
	// NOTE(PR-5): out.AllOrg (admin super-visibility / org:data:read_all) is not
	// consumed yet — the MVP facade always returns false. When the admin bypass
	// lands, AllOrg=true must short-circuit the ownership predicate. It is
	// intentionally NOT part of default agent grounding.
	return out.IDs, nil
}
