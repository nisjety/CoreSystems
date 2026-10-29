package controlplane

import (
	"bytes"
	"context"
	"crypto/rsa"
	"encoding/base64"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"math"
	"math/big"
	"net/http"
	"net/url"
	"slices"
	"strings"
	"sync"
	"time"

	"github.com/golang-jwt/jwt/v5"
	"github.com/google/uuid"

	"github.com/triodelab/integration-corev2/internal/auth"
	"github.com/triodelab/integration-corev2/internal/config"
)

const (
	defaultTimeout      = 5 * time.Second
	ingestionAuditPlane = "ingestion"
)

var errUnknownSigningKey = errors.New("unknown signing key")

type AuthClient struct {
	baseURL          string
	jwksURL          string
	issuer           string
	audience         string
	httpClient       *http.Client
	keysMu           sync.RWMutex
	keysRefreshMu    sync.Mutex
	keys             map[string]*rsa.PublicKey
	keysExpiry       time.Time
	keysGeneration   uint64
	discoveredIssuer string
}

type OrgClient struct {
	baseURL      string
	serviceToken string
	serviceID    string
	httpClient   *http.Client
}

type BillingClient struct {
	baseURL      string
	serviceToken string
	serviceID    string
	source       string
	httpClient   *http.Client
}

type AuditClient struct {
	baseURL      string
	serviceToken string
	serviceID    string
	source       string
	httpClient   *http.Client
}

type UsageEvent struct {
	Metric   string         `json:"metric"`
	Quantity float64        `json:"quantity"`
	Metadata map[string]any `json:"metadata,omitempty"`
}

type AuditEvent struct {
	EventID    string         `json:"event_id"`
	OccurredAt time.Time      `json:"occurred_at"`
	OrgID      string         `json:"org_id"`
	UserID     string         `json:"user_id,omitempty"`
	ActorRole  string         `json:"actor_role,omitempty"`
	Plane      string         `json:"plane"`
	Producer   string         `json:"producer"`
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
	baseURL := strings.TrimRight(cfg.AuthCoreURL, "/")
	jwksURL := strings.TrimSpace(cfg.AuthCoreJWKSURL)
	if jwksURL == "" && baseURL != "" {
		jwksURL = baseURL + "/api/convex-auth/jwks"
	}
	issuer := strings.TrimSpace(cfg.PlaneTokenIssuer)
	if issuer == "" && baseURL != "" {
		issuer = baseURL + "/api/convex-auth"
	}
	audience := strings.TrimSpace(cfg.IngestionAuthAudience)
	if audience == "" {
		audience = "ingestion"
	}
	return &AuthClient{
		baseURL: baseURL, jwksURL: jwksURL, issuer: issuer, audience: audience,
		httpClient: withDefaultClient(httpClient),
	}
}

func NewOrgClient(cfg config.Config, httpClient *http.Client) *OrgClient {
	return &OrgClient{
		baseURL:      strings.TrimRight(cfg.OrgCoreURL, "/"),
		serviceToken: strings.TrimSpace(cfg.OrgCoreServiceToken),
		serviceID:    cfg.ServiceName,
		httpClient:   withDefaultClient(httpClient),
	}
}

func NewBillingClient(cfg config.Config, httpClient *http.Client) *BillingClient {
	return &BillingClient{
		baseURL:      strings.TrimRight(cfg.BillingCoreURL, "/"),
		serviceToken: strings.TrimSpace(cfg.BillingCoreServiceToken),
		serviceID:    cfg.ServiceName,
		source:       cfg.ServiceName,
		httpClient:   withDefaultClient(httpClient),
	}
}

func NewAuditClient(cfg config.Config, httpClient *http.Client) *AuditClient {
	return &AuditClient{
		baseURL:      strings.TrimRight(cfg.AuditCoreURL, "/"),
		serviceToken: strings.TrimSpace(cfg.AuditCoreServiceToken),
		serviceID:    cfg.ServiceName,
		source:       ingestionAuditPlane,
		httpClient:   withDefaultClient(httpClient),
	}
}

func (c *AuthClient) VerifyToken(ctx context.Context, token string) (auth.Principal, error) {
	if c.baseURL == "" || c.jwksURL == "" || c.issuer == "" || c.audience == "" {
		return auth.Principal{}, auth.NewError(http.StatusServiceUnavailable, "auth_core_unconfigured", "AUTH_CORE_URL is not configured")
	}
	keys, issuer, generation, err := c.fetchJWKS(ctx, false, 0)
	if err != nil {
		return auth.Principal{}, auth.NewError(http.StatusServiceUnavailable, "auth_core_unreachable", "Unable to load auth-core verification keys")
	}
	claims, parsed, err := c.parseToken(token, keys, issuer)
	if errors.Is(err, errUnknownSigningKey) {
		keys, issuer, _, err = c.fetchJWKS(ctx, true, generation)
		if err != nil {
			return auth.Principal{}, auth.NewError(http.StatusServiceUnavailable, "auth_core_unreachable", "Unable to refresh auth-core verification keys")
		}
		claims, parsed, err = c.parseToken(token, keys, issuer)
	}
	if err != nil {
		return auth.Principal{}, auth.NewError(http.StatusUnauthorized, "unauthorized", "Token verification failed")
	}
	if !parsed.Valid || numericClaim(claims, "iat") <= 0 || numericClaim(claims, "nbf") <= 0 {
		return auth.Principal{}, auth.NewError(http.StatusUnauthorized, "unauthorized", "Token claims are invalid")
	}
	orgID := stringClaim(claims, "org_id")
	principalType := stringClaim(claims, "principal_type")
	userID := stringClaim(claims, "user_id")
	if principalType == "service" {
		userID = stringClaim(claims, "service_id")
	}
	if orgID == "" || userID == "" || stringClaim(claims, "sub") != userID || (principalType != "user" && principalType != "service") {
		return auth.Principal{}, auth.NewError(http.StatusUnauthorized, "unauthorized", "Token identity claims are invalid")
	}
	scopes := stringSliceClaim(claims, "scopes")
	role := "member"
	if slices.Contains(scopes, "admin") {
		role = "admin"
	}
	return auth.Principal{
		UserID: userID, OrganizationID: orgID, Role: role, Email: stringClaim(claims, "email"),
		PrincipalType: principalType, Scopes: scopes,
	}, nil
}

func (c *AuthClient) parseToken(token string, keys map[string]*rsa.PublicKey, issuer string) (jwt.MapClaims, *jwt.Token, error) {
	claims := jwt.MapClaims{}
	parsed, err := jwt.ParseWithClaims(token, claims, func(parsed *jwt.Token) (any, error) {
		if parsed.Method.Alg() != jwt.SigningMethodRS256.Alg() {
			return nil, fmt.Errorf("unexpected signing algorithm %q", parsed.Method.Alg())
		}
		keyID, _ := parsed.Header["kid"].(string)
		key := keys[keyID]
		if key == nil {
			return nil, errUnknownSigningKey
		}
		return key, nil
	},
		jwt.WithValidMethods([]string{jwt.SigningMethodRS256.Alg()}),
		jwt.WithAudience(c.audience),
		jwt.WithIssuer(issuer),
		jwt.WithExpirationRequired(),
		jwt.WithIssuedAt(),
		jwt.WithLeeway(30*time.Second),
	)
	return claims, parsed, err
}

func (c *AuthClient) fetchJWKS(ctx context.Context, force bool, observedGeneration uint64) (map[string]*rsa.PublicKey, string, uint64, error) {
	c.keysMu.RLock()
	if len(c.keys) > 0 && ((!force && time.Now().Before(c.keysExpiry)) || (force && c.keysGeneration != observedGeneration)) {
		keys := c.keys
		issuer := c.discoveredIssuer
		generation := c.keysGeneration
		c.keysMu.RUnlock()
		return keys, issuer, generation, nil
	}
	c.keysMu.RUnlock()

	c.keysRefreshMu.Lock()
	defer c.keysRefreshMu.Unlock()
	c.keysMu.RLock()
	if len(c.keys) > 0 && ((!force && time.Now().Before(c.keysExpiry)) || (force && c.keysGeneration != observedGeneration)) {
		keys := c.keys
		issuer := c.discoveredIssuer
		generation := c.keysGeneration
		c.keysMu.RUnlock()
		return keys, issuer, generation, nil
	}
	c.keysMu.RUnlock()

	req, err := http.NewRequestWithContext(ctx, http.MethodGet, c.jwksURL, nil)
	if err != nil {
		return nil, "", 0, err
	}
	resp, err := c.httpClient.Do(req)
	if err != nil {
		return nil, "", 0, err
	}
	defer resp.Body.Close()
	if resp.StatusCode != http.StatusOK {
		return nil, "", 0, fmt.Errorf("JWKS returned HTTP %d", resp.StatusCode)
	}
	var document struct {
		PlaneTokenIssuer string `json:"planeTokenIssuer"`
		Keys             []struct {
			KeyID string `json:"kid"`
			Type  string `json:"kty"`
			Use   string `json:"use"`
			Alg   string `json:"alg"`
			N     string `json:"n"`
			E     string `json:"e"`
		} `json:"keys"`
	}
	if err := json.NewDecoder(io.LimitReader(resp.Body, 1<<20)).Decode(&document); err != nil {
		return nil, "", 0, err
	}
	keys := make(map[string]*rsa.PublicKey, len(document.Keys))
	for _, item := range document.Keys {
		if item.KeyID == "" || item.Type != "RSA" || item.Alg != "RS256" || (item.Use != "" && item.Use != "sig") {
			continue
		}
		n, nErr := base64.RawURLEncoding.DecodeString(item.N)
		e, eErr := base64.RawURLEncoding.DecodeString(item.E)
		if nErr != nil || eErr != nil || len(n) == 0 || len(e) == 0 {
			continue
		}
		modulus := new(big.Int).SetBytes(n)
		exponentValue := new(big.Int).SetBytes(e)
		if modulus.BitLen() < 2048 || !exponentValue.IsInt64() {
			continue
		}
		exponent := exponentValue.Int64()
		if exponent >= 3 && exponent <= math.MaxInt32 && exponent%2 == 1 {
			keys[item.KeyID] = &rsa.PublicKey{N: modulus, E: int(exponent)}
		}
	}
	if len(keys) == 0 {
		return nil, "", 0, fmt.Errorf("JWKS contains no usable RS256 keys")
	}
	issuer := strings.TrimSpace(document.PlaneTokenIssuer)
	if issuer == "" {
		issuer = c.issuer
	}
	c.keysMu.Lock()
	c.keys = keys
	c.discoveredIssuer = issuer
	c.keysExpiry = time.Now().Add(5 * time.Minute)
	c.keysGeneration++
	generation := c.keysGeneration
	c.keysMu.Unlock()
	return keys, issuer, generation, nil
}

func stringClaim(claims jwt.MapClaims, key string) string {
	value, _ := claims[key].(string)
	return strings.TrimSpace(value)
}

func numericClaim(claims jwt.MapClaims, key string) int64 {
	switch value := claims[key].(type) {
	case float64:
		return int64(value)
	case json.Number:
		number, _ := value.Int64()
		return number
	default:
		return 0
	}
}

func stringSliceClaim(claims jwt.MapClaims, key string) []string {
	values, _ := claims[key].([]any)
	out := make([]string, 0, len(values))
	for _, value := range values {
		if text, ok := value.(string); ok && text != "" {
			out = append(out, text)
		}
	}
	return out
}

func (c *OrgClient) GetOrgPlan(ctx context.Context, orgID, _ string) (auth.OrgPlan, error) {
	if c.baseURL == "" {
		return auth.OrgPlan{}, auth.NewError(http.StatusServiceUnavailable, "org_core_unconfigured", "ORG_CORE_URL is not configured")
	}
	endpoint := c.baseURL + "/orgs/" + url.PathEscape(orgID)
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, endpoint, nil)
	if err != nil {
		return auth.OrgPlan{}, err
	}
	setServicePrincipal(req, c.serviceID, c.serviceToken)

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
	setServicePrincipal(req, c.serviceID, c.serviceToken)

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
	if c == nil || c.baseURL == "" {
		return fmt.Errorf("audit-core is not configured")
	}
	if strings.TrimSpace(event.EventID) == "" || event.OccurredAt.IsZero() ||
		strings.TrimSpace(event.OrgID) == "" || strings.TrimSpace(event.Event) == "" {
		return fmt.Errorf("audit event_id, occurred_at, org_id, and event are required")
	}
	event.Plane = c.source
	event.Producer = c.serviceID
	if strings.TrimSpace(event.Outcome) == "" {
		event.Outcome = "ok"
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
	setServicePrincipal(req, c.serviceID, c.serviceToken)

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
	if httpClient == nil {
		httpClient = &http.Client{Timeout: defaultTimeout}
	}
	client := *httpClient
	client.CheckRedirect = func(_ *http.Request, _ []*http.Request) error {
		return http.ErrUseLastResponse
	}
	return &client
}

func setServicePrincipal(req *http.Request, serviceID, token string) {
	req.Header.Set("X-Service-Id", strings.TrimSpace(serviceID))
	req.Header.Set("X-Service-Token", strings.TrimSpace(token))
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
