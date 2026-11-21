package clients

import (
	"context"
	"errors"
	"fmt"
	"net/http"
	"net/url"
	"strings"
)

// EraseBrowserSiteChoices calls
// DELETE /internal/orgs/:orgId/members/:userId/browser-sites on org-core. One
// member's remembered agent-browser site choices and their history rows in one
// organisation are erased, whether or not the member is still active (F-f02-16).
// The endpoint's scope (org:browser-site:erase:any) is pinned to the user-core
// principal in org-core's ORG_CORE_SERVICE_CREDENTIALS, so the X-Service-Id and
// X-Service-Token pair that the succession call sends is enough. The call is
// idempotent: a repeat erases nothing more and answers the same way.
func (c *OrgCoreClient) EraseBrowserSiteChoices(ctx context.Context, orgID, userID string) error {
	if c == nil {
		return fmt.Errorf("org-core client is not configured")
	}
	orgID = strings.TrimSpace(orgID)
	userID = strings.TrimSpace(userID)
	if orgID == "" || userID == "" {
		return fmt.Errorf("organization id and user id are required")
	}

	endpoint := fmt.Sprintf("%s/internal/orgs/%s/members/%s/browser-sites",
		c.baseURL, url.PathEscape(orgID), url.PathEscape(userID))
	req, err := http.NewRequestWithContext(ctx, http.MethodDelete, endpoint, nil)
	if err != nil {
		return fmt.Errorf("build browser-site erasure request: %w", err)
	}
	req.Header.Set("X-Service-Id", c.servicePrincipal)
	req.Header.Set("X-Service-Token", c.serviceToken)

	resp, err := c.httpClient.Do(req)
	if err != nil {
		// The transport error names the request URL, and that URL carries the
		// organisation and user ids. Only its cause is kept, so no id reaches the
		// fan-out's retry record.
		var transport *url.Error
		if errors.As(err, &transport) {
			err = transport.Err
		}
		return fmt.Errorf("call org-core browser-site erasure endpoint: %w", err)
	}
	defer func() { _ = resp.Body.Close() }()
	if resp.StatusCode != http.StatusOK {
		return fmt.Errorf("org-core browser-site erasure endpoint returned status %d", resp.StatusCode)
	}
	return nil
}
