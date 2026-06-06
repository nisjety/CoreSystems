package controlplane

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"net/http"
	"net/url"
	"strings"
	"time"

	"github.com/google/uuid"

	"github.com/triodelab/integration-corev2/internal/auth"
	"github.com/triodelab/integration-corev2/internal/config"
)

const defaultTimeout = 5 * time.Second

type AuthClient struct {
	baseURL        string
	internalAPIKey string
	httpClient     *http.Client
}

type OrgClient struct {
	baseURL        string
	internalAPIKey string
	httpClient     *http.Client
}

type BillingClient struct {
	baseURL        string
	internalAPIKey string
	source         string
	httpClient     *http.Client
}

type AuditClient struct {
	baseURL        string
	internalAPIKey string
	source         string
	httpClient     *http.Client
}

type UsageEvent struct {
	Metric   string         `json:"metric"`
	Quantity float64        `json:"quantity"`
	Metadata map[string]any `json:"metadata,omitempty"`
}

type AuditEvent struct {
	OccurredAt time.Time      `json:"occurred_at"`
	OrgID      string         `json:"org_id"`
	UserID     string         `json:"user_id,omitempty"`
	ActorRole  string         `json:"actor_role,omitempty"`
	Plane      string         `json:"plane"`
	Event      string         `json:"event"`
	Subject    string         `json:"subject,omitempty"`
	ResourceID string         `json:"resource_id,omitempty"`
	Outcome    string         `json:"outcome,omitempty"`
	Details    map[string]any `json:"details,omitempty"`
	RequestID  string         `json:"request_id,omitempty"`
	IPAddress  string         `json:"ip_address,omitempty"`
	UserAgent  string         `json:"user_agent,omitempty"`
}

func NewAuthClient(cfg config.Config, httpClient *http.Client) *AuthClient {
	return &AuthClient{
		baseURL:        strings.TrimRight(cfg.AuthCoreURL, "/"),
		internalAPIKey: cfg.ControlPlaneInternalAPIKey(),
		httpClient:     withDefaultClient(httpClient),
	}
}

func NewOrgClient(cfg config.Config, httpClient *http.Client) *OrgClient {
	return &OrgClient{
		baseURL:        strings.TrimRight(cfg.OrgCoreURL, "/"),
		internalAPIKey: cfg.ControlPlaneInternalAPIKey(),
		httpClient:     withDefaultClient(httpClient),
	}
}

func NewBillingClient(cfg config.Config, httpClient *http.Client) *BillingClient {
	return &BillingClient{
		baseURL:        strings.TrimRight(cfg.BillingCoreURL, "/"),
		internalAPIKey: cfg.ControlPlaneInternalAPIKey(),
		source:         cfg.ServiceName,
		httpClient:     withDefaultClient(httpClient),
	}
}

func NewAuditClient(cfg config.Config, httpClient *http.Client) *AuditClient {
	return &AuditClient{
		baseURL:        strings.TrimRight(cfg.AuditCoreURL, "/"),
		internalAPIKey: cfg.ControlPlaneInternalAPIKey(),
		source:         cfg.ServiceName,
		httpClient:     withDefaultClient(httpClient),
	}
}

func (c *AuthClient) VerifyToken(ctx context.Context, token string) (auth.Principal, error) {
	if c.baseURL == "" {
		return auth.Principal{}, auth.NewError(http.StatusServiceUnavailable, "auth_core_unconfigured", "AUTH_CORE_URL is not configured")
	}
	body, err := json.Marshal(map[string]string{"token": token})
	if err != nil {
		return auth.Principal{}, err
	}
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, c.baseURL+"/internal/sessions/verify", bytes.NewReader(body))
	if err != nil {
		return auth.Principal{}, err
	}
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("X-Internal-Api-Key", c.internalAPIKey)

	resp, err := c.httpClient.Do(req)
	if err != nil {
		return auth.Principal{}, auth.NewError(http.StatusServiceUnavailable, "auth_core_unreachable", "Unable to reach auth-core for token verification")
	}
	defer resp.Body.Close()

	var decoded map[string]any
	if err := json.NewDecoder(resp.Body).Decode(&decoded); err != nil {
		return auth.Principal{}, auth.NewError(http.StatusServiceUnavailable, "auth_core_bad_response", "auth-core returned a non-JSON response")
	}
	if resp.StatusCode == http.StatusUnauthorized || resp.StatusCode == http.StatusForbidden || resp.StatusCode < 200 || resp.StatusCode >= 300 {
		return auth.Principal{}, auth.NewError(http.StatusUnauthorized, "unauthorized", errorMessage(decoded, "Token verification failed"))
	}
	payload := unwrapData(decoded)
	principal := auth.Principal{
		UserID:         stringField(payload, "userId", "user_id", "id", "sub"),
		OrganizationID: stringField(payload, "organizationId", "organization_id", "orgId", "org_id"),
		WorkspaceID:    stringField(payload, "workspaceId", "workspace_id"),
		Role:           stringField(payload, "role"),
		Email:          stringField(payload, "email"),
	}
	if principal.Role == "" {
		principal.Role = "member"
	}
	return principal, nil
}

func (c *OrgClient) GetOrgPlan(ctx context.Context, orgID, userID string) (auth.OrgPlan, error) {
	if c.baseURL == "" {
		return auth.OrgPlan{}, auth.NewError(http.StatusServiceUnavailable, "org_core_unconfigured", "ORG_CORE_URL is not configured")
	}
	endpoint := c.baseURL + "/orgs/" + url.PathEscape(orgID)
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, endpoint, nil)
	if err != nil {
		return auth.OrgPlan{}, err
	}
	req.Header.Set("X-Internal-Api-Key", c.internalAPIKey)
	if userID != "" {
		req.Header.Set("X-User-ID", userID)
	}

	resp, err := c.httpClient.Do(req)
	if err != nil {
		return auth.OrgPlan{}, auth.NewError(http.StatusServiceUnavailable, "org_core_unreachable", "Unable to reach org-core for plan lookup")
	}
	defer resp.Body.Close()

	var decoded map[string]any
	if err := json.NewDecoder(resp.Body).Decode(&decoded); err != nil {
		return auth.OrgPlan{}, auth.NewError(http.StatusBadGateway, "org_core_bad_response", "org-core returned non-JSON response")
	}
	if resp.StatusCode == http.StatusNotFound {
		return auth.OrgPlan{}, auth.NewError(http.StatusNotFound, "org_not_found", fmt.Sprintf("Organization %s not found in org-core", orgID))
	}
	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		return auth.OrgPlan{}, auth.NewError(http.StatusBadGateway, "org_core_error", fmt.Sprintf("org-core returned HTTP %d", resp.StatusCode))
	}
	payload := unwrapData(decoded)
	return auth.OrgPlan{
		Plan:         normalizePlan(stringField(payload, "plan", "tier")),
		Quotas:       parseQuotas(payload["quotas"]),
		Entitlements: parseEntitlements(payload["entitlements"]),
		Raw:          payload,
	}, nil
}

func (c *BillingClient) RecordUsage(ctx context.Context, orgID string, event UsageEvent) error {
	if c == nil || c.baseURL == "" || strings.TrimSpace(orgID) == "" || strings.TrimSpace(event.Metric) == "" || event.Quantity <= 0 {
		return nil
	}
	payload := map[string]any{
		"event_id":    "usage_" + uuid.NewString(),
		"metric":      event.Metric,
		"quantity":    event.Quantity,
		"source":      c.source,
		"occurred_at": time.Now().UTC().Format(time.RFC3339),
	}
	if len(event.Metadata) > 0 {
		payload["metadata"] = event.Metadata
	}
	body, err := json.Marshal(payload)
	if err != nil {
		return err
	}
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, c.baseURL+"/api/v1/billing/orgs/"+url.PathEscape(orgID)+"/usage", bytes.NewReader(body))
	if err != nil {
		return err
	}
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("X-Internal-Api-Key", c.internalAPIKey)

	resp, err := c.httpClient.Do(req)
	if err != nil {
		return err
	}
	defer resp.Body.Close()
	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		return fmt.Errorf("billing-core returned HTTP %d", resp.StatusCode)
	}
	return nil
}

func (c *AuditClient) RecordAudit(ctx context.Context, event AuditEvent) error {
	if c == nil || c.baseURL == "" || strings.TrimSpace(event.OrgID) == "" || strings.TrimSpace(event.Event) == "" {
		return nil
	}
	if strings.TrimSpace(event.Plane) == "" {
		event.Plane = c.source
	}
	if strings.TrimSpace(event.Outcome) == "" {
		event.Outcome = "ok"
	}
	if event.OccurredAt.IsZero() {
		event.OccurredAt = time.Now().UTC()
	}
	body, err := json.Marshal(event)
	if err != nil {
		return err
	}
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, c.baseURL+"/v1/audit", bytes.NewReader(body))
	if err != nil {
		return err
	}
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("X-Internal-Api-Key", c.internalAPIKey)

	resp, err := c.httpClient.Do(req)
	if err != nil {
		return err
	}
	defer resp.Body.Close()
	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		return fmt.Errorf("audit-core returned HTTP %d", resp.StatusCode)
	}
	return nil
}

func withDefaultClient(httpClient *http.Client) *http.Client {
	if httpClient != nil {
		return httpClient
	}
	return &http.Client{Timeout: defaultTimeout}
}

func unwrapData(input map[string]any) map[string]any {
	if data, ok := input["data"].(map[string]any); ok {
		return data
	}
	return input
}

func errorMessage(input map[string]any, fallback string) string {
	if errObj, ok := input["error"].(map[string]any); ok {
		if message, ok := errObj["message"].(string); ok && strings.TrimSpace(message) != "" {
			return strings.TrimSpace(message)
		}
	}
	if message, ok := input["message"].(string); ok && strings.TrimSpace(message) != "" {
		return strings.TrimSpace(message)
	}
	return fallback
}

func stringField(input map[string]any, keys ...string) string {
	for _, key := range keys {
		if value, ok := input[key].(string); ok && strings.TrimSpace(value) != "" {
			return strings.TrimSpace(value)
		}
	}
	return ""
}

func normalizePlan(input string) string {
	switch strings.ToLower(strings.TrimSpace(input)) {
	case "starter", "essential":
		return "starter"
	case "pro", "advanced":
		return "pro"
	case "enterprise", "expert", "custom":
		return "enterprise"
	default:
		return "free"
	}
}

func parseQuotas(value any) map[string]auth.Quota {
	out := map[string]auth.Quota{}
	rows, ok := value.([]any)
	if !ok {
		return out
	}
	for _, row := range rows {
		item, ok := row.(map[string]any)
		if !ok {
			continue
		}
		key := stringField(item, "key")
		if key == "" {
			continue
		}
		out[key] = auth.Quota{
			Key:         key,
			Value:       int64Field(item, "value"),
			Limit:       int64Field(item, "limit"),
			ResetPeriod: stringField(item, "resetPeriod", "reset_period"),
		}
	}
	return out
}

func parseEntitlements(value any) map[string]bool {
	out := map[string]bool{}
	switch typed := value.(type) {
	case []any:
		for _, row := range typed {
			item, ok := row.(map[string]any)
			if !ok {
				continue
			}
			key := stringField(item, "key")
			if key == "" {
				continue
			}
			out[key] = boolField(item, "enabled")
		}
	case map[string]any:
		for key, raw := range typed {
			if enabled, ok := raw.(bool); ok {
				out[key] = enabled
			}
		}
	}
	return out
}

func int64Field(input map[string]any, key string) int64 {
	switch value := input[key].(type) {
	case float64:
		return int64(value)
	case int64:
		return value
	case int:
		return int64(value)
	default:
		return 0
	}
}

func boolField(input map[string]any, key string) bool {
	value, _ := input[key].(bool)
	return value
}
