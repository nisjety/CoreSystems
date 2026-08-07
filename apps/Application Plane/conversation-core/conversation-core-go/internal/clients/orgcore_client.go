// Package clients hosts small internal service-to-service HTTP clients for
// conversation-core-go, distinct from the tenant-facing integration client in
// internal/integration.
package clients

import (
	"context"
	"encoding/json"
	"fmt"
	"net/http"
	"strings"
	"time"
)

// OrgCoreClient calls org-core's machine-to-machine /internal/orgs surface.
// GET /internal/orgs requires only X-Service-Id/X-Service-Token (its scope,
// org:read:any, is not ":self"-suffixed, so org-core's service-auth
// middleware does not require signed delegation headers here) — mirrors
// user-core's internal/clients/orgcore_client.go PromoteMemberSuccession.
type OrgCoreClient struct {
	baseURL          string
	servicePrincipal string
	serviceToken     string
	httpClient       *http.Client
}

// NewOrgCoreClient returns nil when any piece is unset, so a caller can fail
// open (feature disabled) rather than fail startup — this client is only
// used by the optional support-recurrence corpus builder.
func NewOrgCoreClient(baseURL, servicePrincipal, serviceToken string) *OrgCoreClient {
	baseURL = strings.TrimRight(strings.TrimSpace(baseURL), "/")
	servicePrincipal = strings.TrimSpace(servicePrincipal)
	serviceToken = strings.TrimSpace(serviceToken)
	if baseURL == "" || servicePrincipal == "" || serviceToken == "" {
		return nil
	}
	return &OrgCoreClient{
		baseURL:          baseURL,
		servicePrincipal: servicePrincipal,
		serviceToken:     serviceToken,
		httpClient:       &http.Client{Timeout: 10 * time.Second},
	}
}

type orgCoreOrganization struct {
	ID       string         `json:"id"`
	Metadata map[string]any `json:"metadata"`
}

type orgCoreListOrganizationsResponse struct {
	Organizations []orgCoreOrganization `json:"organizations"`
	HasMore       bool                  `json:"hasMore"`
}

// maxOrgListPages bounds the enumeration below so a bug in org-core's
// pagination (or an unexpectedly large deployment) can never loop forever.
const maxOrgListPages = 20

// ZDREnabledOrgIDs returns the set of organization IDs currently reporting
// interactiveRetention.zdr = true. Live-read on every call, matching every
// other ZDR check in this codebase (the gateway's require_support_ai_review,
// require_draft_persistence) rather than a cached/event-derived value —
// there is no "ZDR disabled" event anywhere to invalidate a cache with, and
// this feature must reflect an org's CURRENT posture, not its last-known one.
func (c *OrgCoreClient) ZDREnabledOrgIDs(ctx context.Context) (map[string]bool, error) {
	if c == nil {
		return nil, fmt.Errorf("org-core client is not configured")
	}
	zdrOrgIDs := map[string]bool{}
	offset := 0
	const limit = 500
	for range maxOrgListPages {
		url := fmt.Sprintf("%s/internal/orgs?limit=%d&offset=%d", c.baseURL, limit, offset)
		req, err := http.NewRequestWithContext(ctx, http.MethodGet, url, nil)
		if err != nil {
			return nil, fmt.Errorf("build org list request: %w", err)
		}
		req.Header.Set("X-Service-Id", c.servicePrincipal)
		req.Header.Set("X-Service-Token", c.serviceToken)

		resp, err := c.httpClient.Do(req)
		if err != nil {
			return nil, fmt.Errorf("call org-core internal org list: %w", err)
		}
		var decoded orgCoreListOrganizationsResponse
		decodeErr := json.NewDecoder(resp.Body).Decode(&decoded)
		resp.Body.Close()
		if resp.StatusCode != http.StatusOK {
			return nil, fmt.Errorf("org-core internal org list returned status %d", resp.StatusCode)
		}
		if decodeErr != nil {
			return nil, fmt.Errorf("decode org-core internal org list: %w", decodeErr)
		}

		for _, org := range decoded.Organizations {
			if isZDREnabled(org.Metadata) {
				zdrOrgIDs[org.ID] = true
			}
		}
		if !decoded.HasMore {
			break
		}
		offset += limit
	}
	return zdrOrgIDs, nil
}

func isZDREnabled(metadata map[string]any) bool {
	retention, ok := metadata["interactiveRetention"].(map[string]any)
	if !ok {
		return false
	}
	zdr, _ := retention["zdr"].(bool)
	return zdr
}
