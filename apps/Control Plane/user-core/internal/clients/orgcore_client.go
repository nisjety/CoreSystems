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

// OrgCoreClient calls org-core's internal, service-principal-gated surface.
// Only the admin-succession promotion is wired today (see
// internal/users/gdpr_succession.go); extend this client rather than adding a
// second org-core caller elsewhere in user-core.
//
// It satisfies users.OrgCoreSuccessionClient structurally (no import needed
// in either direction — Go interfaces are satisfied by method set, and
// clients must never import the users package, which would create a cycle
// since users package callers construct this client).
type OrgCoreClient struct {
	baseURL          string
	servicePrincipal string
	serviceToken     string
	httpClient       *http.Client
}

// NewOrgCoreClient returns nil when any of baseURL/servicePrincipal/token is
// blank, so callers can treat a nil *OrgCoreClient as "succession promotion is
// not configured on this deployment" — the same fail-closed shape as
// s.userService.ErasureAvailable() elsewhere in user-core's GDPR surface —
// instead of silently no-oping. Callers MUST check for a nil result BEFORE
// assigning it to an interface-typed field (a typed nil boxed into an
// interface is non-nil); see cmd/server/main.go's wiring.
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

// PromoteMemberSuccession calls
// POST /internal/orgs/:orgId/members/:userId/succession. The scope this
// endpoint requires (org:membership:succession:any) is NOT a ":self"-suffixed
// scope, so org-core's service-auth middleware does not require the signed
// delegation headers — only X-Service-Id / X-Service-Token, matching a
// principal registered in org-core's ORG_CORE_SERVICE_CREDENTIALS.
func (c *OrgCoreClient) PromoteMemberSuccession(ctx context.Context, orgID, successorUserID, role string) error {
	if c == nil {
		return fmt.Errorf("org-core client is not configured")
	}
	orgID = strings.TrimSpace(orgID)
	successorUserID = strings.TrimSpace(successorUserID)
	if orgID == "" || successorUserID == "" {
		return fmt.Errorf("organization id and successor user id are required")
	}

	body, err := json.Marshal(map[string]string{"role": role})
	if err != nil {
		return fmt.Errorf("encode succession request: %w", err)
	}
	url := fmt.Sprintf("%s/internal/orgs/%s/members/%s/succession", c.baseURL, orgID, successorUserID)
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, url, bytes.NewReader(body))
	if err != nil {
		return fmt.Errorf("build succession request: %w", err)
	}
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("X-Service-Id", c.servicePrincipal)
	req.Header.Set("X-Service-Token", c.serviceToken)

	resp, err := c.httpClient.Do(req)
	if err != nil {
		return fmt.Errorf("call org-core succession endpoint: %w", err)
	}
	defer func() { _ = resp.Body.Close() }()
	if resp.StatusCode != http.StatusOK {
		return fmt.Errorf("org-core succession endpoint returned status %d", resp.StatusCode)
	}
	return nil
}
