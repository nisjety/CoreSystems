// Package platform provides clients for Velion control-plane services
// (auth-core, billing-core, org-core) used by Quarry for authentication,
// quota enforcement, and usage metering.
package platform

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"path"
	"strings"
	"sync"
	"time"

	grpcmiddleware "github.com/grpc-ecosystem/go-grpc-middleware"
	grpcretry "github.com/grpc-ecosystem/go-grpc-middleware/retry"
	authpb "github.com/triodelab/quarry/internal/platform/authpb/dataplane/auth/v1"
	"google.golang.org/grpc"
	"google.golang.org/grpc/backoff"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/credentials/insecure"
	"google.golang.org/grpc/metadata"
)

type BillingAccount struct {
	OrgID             string                 `json:"org_id"`
	Plan              string                 `json:"plan"`
	SubscriptionState string                 `json:"subscription_state"`
	Credits           int64                  `json:"credits"`
	Products          map[string]bool        `json:"products"`
	FeatureFlags      map[string]bool        `json:"feature_flags"`
	Entitlements      map[string]bool        `json:"entitlements"`
	QuotaLimits       map[string]float64     `json:"quota_limits"`
	Metadata          map[string]interface{} `json:"metadata"`
	UpdatedAt         time.Time              `json:"updated_at"`
	CreatedAt         time.Time              `json:"created_at"`
}

type OrgEntitlement struct {
	Key       string    `json:"key"`
	Enabled   bool      `json:"enabled"`
	UpdatedAt time.Time `json:"updated_at"`
}

type OrgEntitlements struct {
	OrganizationID string           `json:"organization_id"`
	Entitlements   []OrgEntitlement `json:"entitlements"`
}

type QuotaStatus struct {
	OrgID       string  `json:"org_id"`
	Metric      string  `json:"metric"`
	Limit       float64 `json:"limit"`
	Used        float64 `json:"used"`
	Remaining   float64 `json:"remaining"`
	IsExceeded  bool    `json:"is_exceeded"`
	Utilization float64 `json:"utilization"`
}

type cacheEntry struct {
	value     interface{}
	expiresAt time.Time
}

type ttlCache struct {
	mu    sync.RWMutex
	items map[string]cacheEntry
}

func newTTLCache() *ttlCache {
	return &ttlCache{items: make(map[string]cacheEntry)}
}

func (c *ttlCache) Get(key string) (interface{}, bool) {
	if c == nil {
		return nil, false
	}
	now := time.Now()
	c.mu.RLock()
	entry, ok := c.items[key]
	c.mu.RUnlock()
	if !ok || now.After(entry.expiresAt) {
		if ok {
			c.mu.Lock()
			delete(c.items, key)
			c.mu.Unlock()
		}
		return nil, false
	}
	return entry.value, true
}

func (c *ttlCache) Set(key string, value interface{}, ttl time.Duration) {
	if c == nil || strings.TrimSpace(key) == "" || ttl <= 0 {
		return
	}
	c.mu.Lock()
	c.items[key] = cacheEntry{value: value, expiresAt: time.Now().Add(ttl)}
	c.mu.Unlock()
}

func (c *ttlCache) Delete(key string) {
	if c == nil || strings.TrimSpace(key) == "" {
		return
	}
	c.mu.Lock()
	delete(c.items, key)
	c.mu.Unlock()
}

// ─── AuthClient ───────────────────────────────────────────────────────────────

// AuthClient verifies Bearer tokens against the control-plane auth-core service.
// It prefers gRPC when a connection is available and falls back to HTTP.
type AuthClient struct {
	baseURL        string
	internalAPIKey string
	httpClient     *http.Client
	cache          *ttlCache
	cacheTTL       time.Duration

	// gRPC fields — set when an AuthCoreGRPCAddr is configured.
	grpcConn    *grpc.ClientConn
	tokenClient authpb.TokenValidationServiceClient
}

// NewAuthClient creates a client that talks to auth-core over HTTP.
func NewAuthClient(baseURL, internalAPIKey string) *AuthClient {
	return NewAuthClientWithCache(baseURL, internalAPIKey, 0)
}

// NewAuthClientWithCache creates a client that talks to auth-core over HTTP
// and caches successful token resolutions for a short TTL.
func NewAuthClientWithCache(baseURL, internalAPIKey string, cacheTTL time.Duration) *AuthClient {
	return &AuthClient{
		baseURL:        strings.TrimRight(baseURL, "/"),
		internalAPIKey: internalAPIKey,
		httpClient: &http.Client{
			Timeout: 5 * time.Second,
		},
		cache:    newTTLCache(),
		cacheTTL: cacheTTL,
	}
}

// ConnectGRPC dials the auth-core gRPC server and stores the connection for
// use in VerifyToken. Call once at startup; the caller owns closing via Close().
func (c *AuthClient) ConnectGRPC(ctx context.Context, addr string) error {
	if addr == "" {
		return nil
	}
	retryOpts := []grpcretry.CallOption{
		grpcretry.WithBackoff(grpcretry.BackoffExponential(100 * time.Millisecond)),
		grpcretry.WithMax(3),
		grpcretry.WithCodes(codes.Unavailable, codes.DeadlineExceeded),
	}
	conn, err := grpc.DialContext(
		ctx,
		addr,
		grpc.WithTransportCredentials(insecure.NewCredentials()),
		grpc.WithConnectParams(grpc.ConnectParams{
			Backoff:           backoff.Config{BaseDelay: 200 * time.Millisecond, Multiplier: 1.6, MaxDelay: 5 * time.Second},
			MinConnectTimeout: 5 * time.Second,
		}),
		grpc.WithUnaryInterceptor(grpcmiddleware.ChainUnaryClient(
			grpcretry.UnaryClientInterceptor(retryOpts...),
		)),
	)
	if err != nil {
		return fmt.Errorf("dial auth-core grpc %s: %w", addr, err)
	}
	c.grpcConn = conn
	c.tokenClient = authpb.NewTokenValidationServiceClient(conn)
	return nil
}

// Close releases the underlying gRPC connection if one was established.
func (c *AuthClient) Close() error {
	if c == nil || c.grpcConn == nil {
		return nil
	}
	return c.grpcConn.Close()
}

type authCoreVerifyResponse struct {
	Success bool `json:"success"`
	Data    struct {
		UserID         string `json:"userId"`
		OrganizationID string `json:"organizationId"`
		WorkspaceID    string `json:"workspaceId"`
		Role           string `json:"role"`
		Email          string `json:"email"`
		Tier           string `json:"tier"`
	} `json:"data"`
	Error *struct {
		Code    string `json:"code"`
		Message string `json:"message"`
	} `json:"error"`
}

// VerifyToken validates a Bearer token against auth-core.
// Uses gRPC when a connection is available (lower latency, TLS metadata),
// falls back to HTTP /internal/sessions/verify otherwise.
func (c *AuthClient) VerifyToken(ctx context.Context, token string) (*Principal, error) {
	if c == nil {
		return nil, fmt.Errorf("auth-core not configured")
	}
	if strings.TrimSpace(token) == "" {
		return nil, fmt.Errorf("token is empty")
	}
	if c.cache != nil && c.cacheTTL > 0 {
		if cached, ok := c.cache.Get(authTokenCacheKey(token)); ok {
			if principal, ok := cached.(*Principal); ok && principal != nil {
				return principal.Clone(), nil
			}
		}
	}

	var principal *Principal
	var err error
	if c.tokenClient != nil {
		principal, err = c.verifyTokenGRPC(ctx, token)
	} else {
		principal, err = c.verifyTokenHTTP(ctx, token)
	}
	if err != nil {
		return nil, err
	}

	if c.cache != nil && c.cacheTTL > 0 {
		c.cache.Set(authTokenCacheKey(token), principal.Clone(), c.cacheTTL)
	}
	return principal, nil
}

// verifyTokenGRPC calls auth-core TokenValidationService.ValidateToken over gRPC.
func (c *AuthClient) verifyTokenGRPC(ctx context.Context, token string) (*Principal, error) {
	md := metadata.Pairs("x-service-auth", c.internalAPIKey)
	ctx = metadata.NewOutgoingContext(ctx, md)

	resp, err := c.tokenClient.ValidateToken(ctx, &authpb.ValidateTokenRequest{Token: token})
	if err != nil {
		return nil, fmt.Errorf("auth-core grpc: %w", err)
	}
	if !resp.Valid {
		return nil, fmt.Errorf("unauthorized: token validation failed")
	}
	if resp.UserId == "" || resp.OrgId == "" {
		return nil, fmt.Errorf("auth-core grpc: incomplete principal (userId or orgId empty)")
	}
	return &Principal{
		UserID:         resp.UserId,
		OrganizationID: resp.OrgId,
		Email:          resp.Email,
		Role:           resp.Role,
		Tier:           "free", // gRPC response doesn't carry tier; billing-core owns it
	}, nil
}

// verifyTokenHTTP calls auth-core POST /internal/sessions/verify (HTTP fallback).
func (c *AuthClient) verifyTokenHTTP(ctx context.Context, token string) (*Principal, error) {
	body, err := json.Marshal(map[string]string{"token": token})
	if err != nil {
		return nil, fmt.Errorf("marshal verify request: %w", err)
	}

	reqURL := c.baseURL + "/internal/sessions/verify"
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, reqURL, bytes.NewReader(body))
	if err != nil {
		return nil, fmt.Errorf("create verify request: %w", err)
	}
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("x-internal-api-key", c.internalAPIKey)

	resp, err := c.httpClient.Do(req)
	if err != nil {
		return nil, fmt.Errorf("auth-core unreachable: %w", err)
	}
	defer resp.Body.Close()

	respBody, err := io.ReadAll(io.LimitReader(resp.Body, 1<<20))
	if err != nil {
		return nil, fmt.Errorf("read auth-core response: %w", err)
	}

	var result authCoreVerifyResponse
	if err := json.Unmarshal(respBody, &result); err != nil {
		return nil, fmt.Errorf("parse auth-core response: %w", err)
	}

	if resp.StatusCode == http.StatusUnauthorized || resp.StatusCode == http.StatusForbidden || !result.Success {
		msg := "token verification failed"
		if result.Error != nil {
			msg = result.Error.Message
		}
		return nil, fmt.Errorf("unauthorized: %s", msg)
	}

	if result.Data.UserID == "" || result.Data.OrganizationID == "" {
		return nil, fmt.Errorf("auth-core returned incomplete principal")
	}

	tier := result.Data.Tier
	if tier == "" {
		tier = "free"
	}

	return &Principal{
		UserID:         result.Data.UserID,
		OrganizationID: result.Data.OrganizationID,
		WorkspaceID:    result.Data.WorkspaceID,
		Role:           result.Data.Role,
		Email:          result.Data.Email,
		Tier:           tier,
	}, nil
}

// SandboxAccountResult holds the credentials returned by auth-core after a
// successful sandbox account creation.
type SandboxAccountResult struct {
	OrgID  string `json:"orgId"`
	UserID string `json:"userId"`
	APIKey string `json:"apiKey"`
}

// CreateSandboxAccount calls auth-core POST /internal/agent-signup to create a
// free-tier organisation, user and API key for the supplied email address.
func (c *AuthClient) CreateSandboxAccount(ctx context.Context, email string) (*SandboxAccountResult, error) {
	if c == nil {
		return nil, fmt.Errorf("auth-core not configured")
	}
	body, err := json.Marshal(map[string]string{"email": email})
	if err != nil {
		return nil, fmt.Errorf("auth-core: marshal sandbox request: %w", err)
	}

	url := c.baseURL + "/internal/agent-signup"
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, url, bytes.NewReader(body))
	if err != nil {
		return nil, fmt.Errorf("auth-core: build sandbox request: %w", err)
	}
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("x-internal-api-key", c.internalAPIKey)

	resp, err := c.httpClient.Do(req)
	if err != nil {
		return nil, fmt.Errorf("auth-core: sandbox request: %w", err)
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusCreated && resp.StatusCode != http.StatusOK {
		b, _ := io.ReadAll(io.LimitReader(resp.Body, 512))
		return nil, fmt.Errorf("auth-core: sandbox response %d: %s", resp.StatusCode, strings.TrimSpace(string(b)))
	}

	var result struct {
		Data *SandboxAccountResult `json:"data"`
	}
	if err := json.NewDecoder(resp.Body).Decode(&result); err != nil {
		return nil, fmt.Errorf("auth-core: decode sandbox response: %w", err)
	}
	if result.Data == nil || result.Data.APIKey == "" {
		return nil, fmt.Errorf("auth-core: sandbox response missing apiKey")
	}
	return result.Data, nil
}

// ─── BillingClient ────────────────────────────────────────────────────────────
type BillingClient struct {
	baseURL        string
	internalAPIKey string
	httpClient     *http.Client
}

// NewBillingClient creates a client that talks to billing-core.
func NewBillingClient(baseURL, internalAPIKey string) *BillingClient {
	return &BillingClient{
		baseURL:        normalizeBillingBaseURL(baseURL),
		internalAPIKey: internalAPIKey,
		httpClient: &http.Client{
			Timeout: 5 * time.Second,
		},
	}
}

type QuotaResult struct {
	Allowed   bool   `json:"allowed"`
	Remaining int64  `json:"remaining"`
	Limit     int64  `json:"limit"`
	Metric    string `json:"metric"`
	ResetAt   string `json:"resetAt,omitempty"`
}

type billingQuotaEnvelope struct {
	Quota QuotaStatus `json:"quota"`
}

type OrgClient struct {
	baseURL        string
	internalAPIKey string
	httpClient     *http.Client
}

func NewOrgClient(baseURL, internalAPIKey string) *OrgClient {
	return &OrgClient{
		baseURL:        normalizeOrgBaseURL(baseURL),
		internalAPIKey: internalAPIKey,
		httpClient: &http.Client{
			Timeout: 5 * time.Second,
		},
	}
}

type ControlPlaneConfig struct {
	BillingBaseURL string
	OrgBaseURL     string
	UserBaseURL    string
	InternalAPIKey string
	CacheTTL       time.Duration
}

type ControlPlaneService struct {
	billing *BillingClient
	org     *OrgClient
	user    *UserClient
	cache   *ttlCache
	ttl     time.Duration
}

func NewControlPlaneService(cfg ControlPlaneConfig) *ControlPlaneService {
	ttl := cfg.CacheTTL
	if ttl <= 0 {
		ttl = time.Minute
	}
	service := &ControlPlaneService{
		cache: newTTLCache(),
		ttl:   ttl,
	}
	if strings.TrimSpace(cfg.BillingBaseURL) != "" {
		service.billing = NewBillingClient(cfg.BillingBaseURL, cfg.InternalAPIKey)
	}
	if strings.TrimSpace(cfg.OrgBaseURL) != "" {
		service.org = NewOrgClient(cfg.OrgBaseURL, cfg.InternalAPIKey)
	}
	if strings.TrimSpace(cfg.UserBaseURL) != "" {
		service.user = NewUserClient(cfg.UserBaseURL, cfg.InternalAPIKey)
	}
	return service
}

func normalizeBillingBaseURL(base string) string {
	base = strings.TrimRight(strings.TrimSpace(base), "/")
	if base == "" {
		return ""
	}
	if strings.Contains(base, "/api/v1/billing") {
		return base
	}
	return base + "/api/v1/billing"
}

func normalizeOrgBaseURL(base string) string {
	return strings.TrimRight(strings.TrimSpace(base), "/")
}

func appendURLPath(baseURL string, segments ...string) string {
	parsed, err := url.Parse(baseURL)
	if err != nil {
		return strings.TrimRight(baseURL, "/")
	}
	parts := []string{strings.TrimRight(parsed.Path, "/")}
	for _, segment := range segments {
		if trimmed := strings.Trim(segment, "/"); trimmed != "" {
			parts = append(parts, trimmed)
		}
	}
	parsed.Path = path.Join(parts...)
	return parsed.String()
}

type billingCheckResponse struct {
	Success bool         `json:"success"`
	Data    *QuotaResult `json:"data"`
	Error   *struct {
		Code    string `json:"code"`
		Message string `json:"message"`
	} `json:"error"`
}

// CheckQuota asks billing-core whether orgID has remaining quota for the given metric.
// Returns (result, nil) on success, or (nil, err) on network/parse errors.
// A non-nil result with Allowed=false means quota is exhausted — NOT an error.
func (c *BillingClient) CheckQuota(ctx context.Context, orgID, metric string, requestedUnits int64) (*QuotaResult, error) {
	if c == nil || c.baseURL == "" {
		// Billing not configured — allow by default (open-core mode).
		return &QuotaResult{Allowed: true, Remaining: -1, Limit: -1, Metric: metric}, nil
	}
	status, err := c.GetQuotaStatus(ctx, orgID, metric)
	if err != nil {
		return &QuotaResult{Allowed: true, Remaining: -1, Limit: -1, Metric: metric}, err
	}
	remaining := int64(status.Remaining)
	limit := int64(status.Limit)
	allowed := !status.IsExceeded && (status.Remaining < 0 || status.Remaining >= float64(requestedUnits))
	return &QuotaResult{
		Allowed:   allowed,
		Remaining: remaining,
		Limit:     limit,
		Metric:    metric,
	}, nil
}

// RecordUsage tells billing-core to debit units from an org's quota.
// Fire-and-forget: errors are returned but should not block the caller.
func (c *BillingClient) RecordUsage(ctx context.Context, orgID, metric string, units int64, metadata map[string]interface{}) error {
	if c == nil || c.baseURL == "" {
		return nil // billing not configured
	}

	body, err := json.Marshal(map[string]interface{}{
		"event_id":    buildUsageEventID(orgID, metric, metadata),
		"metric":      metric,
		"quantity":    units,
		"source":      "quarry",
		"occurred_at": time.Now().UTC().Format(time.RFC3339),
		"metadata":    metadata,
	})
	if err != nil {
		return fmt.Errorf("marshal usage request: %w", err)
	}

	url := appendURLPath(c.baseURL, "orgs", orgID, "usage")
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, url, bytes.NewReader(body))
	if err != nil {
		return fmt.Errorf("create usage request: %w", err)
	}
	req.Header.Set("Content-Type", "application/json")
	if strings.TrimSpace(c.internalAPIKey) != "" {
		req.Header.Set("x-internal-api-key", c.internalAPIKey)
	}

	resp, err := c.httpClient.Do(req)
	if err != nil {
		return fmt.Errorf("billing-core unreachable: %w", err)
	}
	defer resp.Body.Close()

	if resp.StatusCode >= 400 {
		return fmt.Errorf("billing-core returned %d recording usage", resp.StatusCode)
	}

	return nil
}

func (c *BillingClient) GetAccount(ctx context.Context, orgID string) (*BillingAccount, error) {
	if c == nil || c.baseURL == "" {
		return nil, fmt.Errorf("billing-core not configured")
	}
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, appendURLPath(c.baseURL, "orgs", orgID, "account"), nil)
	if err != nil {
		return nil, fmt.Errorf("create get account request: %w", err)
	}
	if strings.TrimSpace(c.internalAPIKey) != "" {
		req.Header.Set("x-internal-api-key", c.internalAPIKey)
	}
	resp, err := c.httpClient.Do(req)
	if err != nil {
		return nil, fmt.Errorf("billing-core unreachable: %w", err)
	}
	defer resp.Body.Close()
	if resp.StatusCode >= 400 {
		body, _ := io.ReadAll(io.LimitReader(resp.Body, 4096))
		return nil, fmt.Errorf("billing-core returned %d: %s", resp.StatusCode, strings.TrimSpace(string(body)))
	}
	var account BillingAccount
	if err := json.NewDecoder(io.LimitReader(resp.Body, 1<<20)).Decode(&account); err != nil {
		return nil, fmt.Errorf("decode billing account: %w", err)
	}
	return &account, nil
}

func (c *BillingClient) GetQuotaStatus(ctx context.Context, orgID, metric string) (*QuotaStatus, error) {
	if c == nil || c.baseURL == "" {
		return nil, fmt.Errorf("billing-core not configured")
	}
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, appendURLPath(c.baseURL, "orgs", orgID, "quotas", metric), nil)
	if err != nil {
		return nil, fmt.Errorf("create quota status request: %w", err)
	}
	if strings.TrimSpace(c.internalAPIKey) != "" {
		req.Header.Set("x-internal-api-key", c.internalAPIKey)
	}
	resp, err := c.httpClient.Do(req)
	if err != nil {
		return nil, fmt.Errorf("billing-core unreachable: %w", err)
	}
	defer resp.Body.Close()
	if resp.StatusCode >= 400 {
		body, _ := io.ReadAll(io.LimitReader(resp.Body, 4096))
		return nil, fmt.Errorf("billing-core returned %d: %s", resp.StatusCode, strings.TrimSpace(string(body)))
	}
	var payload billingQuotaEnvelope
	if err := json.NewDecoder(io.LimitReader(resp.Body, 1<<20)).Decode(&payload); err != nil {
		return nil, fmt.Errorf("decode quota status: %w", err)
	}
	return &payload.Quota, nil
}

func (c *OrgClient) GetEntitlements(ctx context.Context, orgID string) (*OrgEntitlements, error) {
	if c == nil || c.baseURL == "" {
		return nil, fmt.Errorf("org-core not configured")
	}
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, appendURLPath(c.baseURL, "orgs", orgID, "entitlements"), nil)
	if err != nil {
		return nil, fmt.Errorf("create get entitlements request: %w", err)
	}
	if strings.TrimSpace(c.internalAPIKey) != "" {
		req.Header.Set("x-internal-api-key", c.internalAPIKey)
	}
	resp, err := c.httpClient.Do(req)
	if err != nil {
		return nil, fmt.Errorf("org-core unreachable: %w", err)
	}
	defer resp.Body.Close()
	if resp.StatusCode >= 400 {
		body, _ := io.ReadAll(io.LimitReader(resp.Body, 4096))
		return nil, fmt.Errorf("org-core returned %d: %s", resp.StatusCode, strings.TrimSpace(string(body)))
	}
	var payload OrgEntitlements
	if err := json.NewDecoder(io.LimitReader(resp.Body, 1<<20)).Decode(&payload); err != nil {
		return nil, fmt.Errorf("decode entitlements: %w", err)
	}
	return &payload, nil
}

func (s *ControlPlaneService) GetAccount(ctx context.Context, orgID string) (*BillingAccount, error) {
	if s == nil || strings.TrimSpace(orgID) == "" {
		return nil, fmt.Errorf("control plane account lookup is not configured")
	}
	cacheKey := "account:" + orgID
	if cached, ok := s.cache.Get(cacheKey); ok {
		if account, ok := cached.(*BillingAccount); ok && account != nil {
			return cloneBillingAccount(account), nil
		}
	}
	if s.billing == nil {
		return nil, fmt.Errorf("billing client is not configured")
	}
	account, err := s.billing.GetAccount(ctx, orgID)
	if err != nil {
		return nil, err
	}
	s.cache.Set(cacheKey, cloneBillingAccount(account), s.ttl)
	return cloneBillingAccount(account), nil
}

func (s *ControlPlaneService) GetQuotaStatus(ctx context.Context, orgID, metric string) (*QuotaStatus, error) {
	if s == nil || strings.TrimSpace(orgID) == "" || strings.TrimSpace(metric) == "" {
		return nil, fmt.Errorf("control plane quota lookup is not configured")
	}
	cacheKey := "quota:" + orgID + ":" + metric
	if cached, ok := s.cache.Get(cacheKey); ok {
		if status, ok := cached.(*QuotaStatus); ok && status != nil {
			clone := *status
			return &clone, nil
		}
	}
	if s.billing == nil {
		return nil, fmt.Errorf("billing client is not configured")
	}
	status, err := s.billing.GetQuotaStatus(ctx, orgID, metric)
	if err != nil {
		return nil, err
	}
	clone := *status
	s.cache.Set(cacheKey, &clone, s.ttl)
	return &clone, nil
}

func (s *ControlPlaneService) GetFirstQuotaStatus(ctx context.Context, orgID string, metrics ...string) (*QuotaStatus, string, error) {
	var lastErr error
	for _, metric := range metrics {
		metric = strings.TrimSpace(metric)
		if metric == "" {
			continue
		}
		status, err := s.GetQuotaStatus(ctx, orgID, metric)
		if err == nil && status != nil {
			return status, metric, nil
		}
		if err != nil {
			lastErr = err
		}
	}
	if lastErr != nil {
		return nil, "", lastErr
	}
	return nil, "", nil
}

func (s *ControlPlaneService) GetEntitlements(ctx context.Context, orgID string) (*OrgEntitlements, error) {
	if s == nil || strings.TrimSpace(orgID) == "" {
		return nil, fmt.Errorf("control plane entitlement lookup is not configured")
	}
	cacheKey := "entitlements:" + orgID
	if cached, ok := s.cache.Get(cacheKey); ok {
		if entitlements, ok := cached.(*OrgEntitlements); ok && entitlements != nil {
			return cloneOrgEntitlements(entitlements), nil
		}
	}
	if s.org == nil {
		return nil, fmt.Errorf("org client is not configured")
	}
	entitlements, err := s.org.GetEntitlements(ctx, orgID)
	if err != nil {
		return nil, err
	}
	s.cache.Set(cacheKey, cloneOrgEntitlements(entitlements), s.ttl)
	return cloneOrgEntitlements(entitlements), nil
}

func cloneBillingAccount(account *BillingAccount) *BillingAccount {
	if account == nil {
		return nil
	}
	cloned := *account
	cloned.Products = cloneBoolMap(account.Products)
	cloned.FeatureFlags = cloneBoolMap(account.FeatureFlags)
	cloned.Entitlements = cloneBoolMap(account.Entitlements)
	cloned.QuotaLimits = cloneFloatMap(account.QuotaLimits)
	cloned.Metadata = cloneAnyMap(account.Metadata)
	return &cloned
}

func cloneOrgEntitlements(input *OrgEntitlements) *OrgEntitlements {
	if input == nil {
		return nil
	}
	cloned := &OrgEntitlements{
		OrganizationID: input.OrganizationID,
		Entitlements:   make([]OrgEntitlement, len(input.Entitlements)),
	}
	copy(cloned.Entitlements, input.Entitlements)
	return cloned
}

func cloneBoolMap(input map[string]bool) map[string]bool {
	if input == nil {
		return nil
	}
	output := make(map[string]bool, len(input))
	for key, value := range input {
		output[key] = value
	}
	return output
}

func cloneFloatMap(input map[string]float64) map[string]float64 {
	if input == nil {
		return nil
	}
	output := make(map[string]float64, len(input))
	for key, value := range input {
		output[key] = value
	}
	return output
}

func cloneAnyMap(input map[string]interface{}) map[string]interface{} {
	if input == nil {
		return nil
	}
	output := make(map[string]interface{}, len(input))
	for key, value := range input {
		output[key] = value
	}
	return output
}

func buildUsageEventID(orgID, metric string, metadata map[string]interface{}) string {
	if requestID, ok := metadata["requestId"].(string); ok && strings.TrimSpace(requestID) != "" {
		return orgID + ":" + metric + ":" + requestID
	}
	return orgID + ":" + metric + ":" + time.Now().UTC().Format(time.RFC3339Nano)
}

func authTokenCacheKey(token string) string {
	sum := sha256.Sum256([]byte(token))
	return fmt.Sprintf("auth:%x", sum[:])
}
