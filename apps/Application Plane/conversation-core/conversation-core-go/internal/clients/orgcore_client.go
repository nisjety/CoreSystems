// Package clients hosts small internal service-to-service HTTP clients for
// conversation-core-go, distinct from the tenant-facing integration client in
// internal/integration.
package clients

import (
	"context"
	"encoding/json"
	"fmt"
	"net/http"
	"net/url"
	"strings"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/conversation-core/conversation-core-go/internal/conversation"
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

// NewOrgCoreClient returns nil when any piece is unset. The process may still
// start for read-only traffic, but any AI proposal or support-recurrence
// action that requires this client will fail closed at its durable boundary.
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

// SupportPolicy is the small, current Control Plane decision Conversation Core
// needs at its own durable boundaries. It deliberately excludes membership:
// the signed gateway delegation has already bound the caller role, while
// Org Core remains authoritative for organization retention and capabilities.
type SupportPolicy struct {
	ZDREnabled        bool
	AIReviewEnabled   bool
	RecurrenceAllowed bool
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

// SupportPolicy resolves exact organization policy and the effective capability
// set for the verified caller role. A failure is surfaced to the Conversation
// Core resource server so it can fail closed, never as a cached gateway hint.
func (c *OrgCoreClient) SupportPolicy(ctx context.Context, orgID, role string) (SupportPolicy, error) {
	if c == nil {
		return SupportPolicy{}, fmt.Errorf("org-core client is not configured")
	}
	orgID = strings.TrimSpace(orgID)
	role = strings.TrimSpace(strings.ToLower(role))
	if orgID == "" || role == "" {
		return SupportPolicy{}, fmt.Errorf("organization id and role are required")
	}

	organization, err := c.getOrganization(ctx, orgID)
	if err != nil {
		return SupportPolicy{}, err
	}
	capabilities, err := c.getEffectiveCapabilities(ctx, orgID, role)
	if err != nil {
		return SupportPolicy{}, err
	}
	return SupportPolicy{
		ZDREnabled:        isZDREnabled(organization.Metadata),
		AIReviewEnabled:   supportAIReviewEnabled(organization.Metadata),
		RecurrenceAllowed: containsCapability(capabilities, "support:recurrence:read"),
	}, nil
}

// AllowAIProposal implements conversation.AIProposalPolicy. Every failure is
// converted into a stable domain error so HTTP and asynchronous producers have
// identical fail-closed behavior without exposing Control Plane internals.
func (c *OrgCoreClient) AllowAIProposal(ctx context.Context, orgID string) error {
	if c == nil {
		return conversation.ErrPolicyUnavailable
	}
	organization, err := c.getOrganization(ctx, strings.TrimSpace(orgID))
	if err != nil {
		return fmt.Errorf("%w: %v", conversation.ErrPolicyUnavailable, err)
	}
	if isZDREnabled(organization.Metadata) {
		return conversation.ErrZDRAIProposalForbidden
	}
	if !supportAIReviewEnabled(organization.Metadata) {
		return conversation.ErrAIReviewModeRequired
	}
	return nil
}

func (c *OrgCoreClient) getOrganization(ctx context.Context, orgID string) (*orgCoreOrganization, error) {
	request, err := http.NewRequestWithContext(
		ctx,
		http.MethodGet,
		fmt.Sprintf("%s/internal/orgs/%s", c.baseURL, url.PathEscape(orgID)),
		nil,
	)
	if err != nil {
		return nil, fmt.Errorf("build org-core organization request: %w", err)
	}
	request.Header.Set("X-Service-Id", c.servicePrincipal)
	request.Header.Set("X-Service-Token", c.serviceToken)
	response, err := c.httpClient.Do(request)
	if err != nil {
		return nil, fmt.Errorf("call org-core organization: %w", err)
	}
	defer response.Body.Close()
	if response.StatusCode != http.StatusOK {
		return nil, fmt.Errorf("org-core organization returned status %d", response.StatusCode)
	}
	var organization orgCoreOrganization
	if err := json.NewDecoder(response.Body).Decode(&organization); err != nil {
		return nil, fmt.Errorf("decode org-core organization: %w", err)
	}
	if strings.TrimSpace(organization.ID) != orgID {
		return nil, fmt.Errorf("org-core organization response did not match requested organization")
	}
	return &organization, nil
}

func (c *OrgCoreClient) getEffectiveCapabilities(ctx context.Context, orgID, role string) ([]string, error) {
	request, err := http.NewRequestWithContext(
		ctx,
		http.MethodGet,
		fmt.Sprintf("%s/internal/orgs/%s/roles/%s/capabilities", c.baseURL, url.PathEscape(orgID), url.PathEscape(role)),
		nil,
	)
	if err != nil {
		return nil, fmt.Errorf("build org-core capability request: %w", err)
	}
	request.Header.Set("X-Service-Id", c.servicePrincipal)
	request.Header.Set("X-Service-Token", c.serviceToken)
	response, err := c.httpClient.Do(request)
	if err != nil {
		return nil, fmt.Errorf("call org-core capabilities: %w", err)
	}
	defer response.Body.Close()
	if response.StatusCode != http.StatusOK {
		return nil, fmt.Errorf("org-core capabilities returned status %d", response.StatusCode)
	}
	var payload struct {
		Capabilities []string `json:"capabilities"`
	}
	if err := json.NewDecoder(response.Body).Decode(&payload); err != nil {
		return nil, fmt.Errorf("decode org-core capabilities: %w", err)
	}
	return payload.Capabilities, nil
}

func supportAIReviewEnabled(metadata map[string]any) bool {
	supportAI, ok := metadata["supportAi"].(map[string]any)
	if !ok {
		return true
	}
	mode, ok := supportAI["mode"].(string)
	return !ok || strings.TrimSpace(strings.ToLower(mode)) == "review"
}

func containsCapability(capabilities []string, expected string) bool {
	for _, capability := range capabilities {
		if strings.TrimSpace(capability) == expected {
			return true
		}
	}
	return false
}
