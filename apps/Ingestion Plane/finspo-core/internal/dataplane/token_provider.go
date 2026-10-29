package dataplane

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"strings"
	"sync"
	"time"
)

const (
	dataPlaneAudience        = "data-plane"
	dataPlaneWriteScope      = "documents:write"
	defaultTokenRefreshSkew  = 30 * time.Second
	maximumTokenLifetime     = 10 * time.Minute
	maximumTokenCacheEntries = 10_000
)

// OrgTokenProvider provides short-lived Data Plane credentials bound to a
// verified organization. Invalidate is conditional on the rejected token so a
// stale 401 cannot evict a newer token minted by a concurrent request.
type OrgTokenProvider interface {
	Configured() bool
	Token(context.Context, string) (string, error)
	Invalidate(orgID, rejectedToken string)
}

type AuthCoreTokenConfig struct {
	AuthCoreURL   string
	ServiceID     string
	ServiceAPIKey string
	HTTPClient    *http.Client
	Now           func() time.Time
}

type AuthCoreTokenProvider struct {
	authCoreURL     string
	serviceID       string
	serviceAPIKey   string
	httpClient      *http.Client
	now             func() time.Time
	mu              sync.Mutex
	cache           map[string]cachedToken
	inFlight        map[string]*tokenMintFlight
	maxCacheEntries int
	accessSequence  uint64
}

type cachedToken struct {
	value      string
	refreshAt  time.Time
	lastAccess uint64
}

type tokenMintFlight struct {
	done  chan struct{}
	token string
	err   error
}

type tokenBundle struct {
	Token            string `json:"token"`
	ExpiresAt        string `json:"expiresAt"`
	ExpiresInSeconds int    `json:"expiresInSeconds"`
	Issuer           string `json:"issuer"`
	Audience         string `json:"audience"`
}

func NewAuthCoreTokenProvider(cfg AuthCoreTokenConfig) *AuthCoreTokenProvider {
	httpClient := cfg.HTTPClient
	if httpClient == nil {
		httpClient = &http.Client{Timeout: 10 * time.Second}
	}
	issuerClient := &http.Client{
		Transport: httpClient.Transport,
		Timeout:   httpClient.Timeout,
		CheckRedirect: func(_ *http.Request, _ []*http.Request) error {
			return http.ErrUseLastResponse
		},
	}
	now := cfg.Now
	if now == nil {
		now = time.Now
	}
	return &AuthCoreTokenProvider{
		authCoreURL: strings.TrimRight(strings.TrimSpace(cfg.AuthCoreURL), "/"),
		serviceID:   strings.TrimSpace(cfg.ServiceID), serviceAPIKey: strings.TrimSpace(cfg.ServiceAPIKey),
		httpClient: issuerClient, now: now, cache: make(map[string]cachedToken),
		inFlight: make(map[string]*tokenMintFlight), maxCacheEntries: maximumTokenCacheEntries,
	}
}

func (p *AuthCoreTokenProvider) Configured() bool {
	return p != nil && p.authCoreURL != "" && p.serviceID != "" && p.serviceAPIKey != ""
}

func (p *AuthCoreTokenProvider) Token(ctx context.Context, orgID string) (string, error) {
	orgID = strings.TrimSpace(orgID)
	if orgID == "" {
		return "", fmt.Errorf("data plane token: verified organization is required")
	}
	if !p.Configured() {
		return "", fmt.Errorf("data plane token: Auth Core service principal is not configured")
	}
	p.mu.Lock()
	if cached, ok := p.cache[orgID]; ok && p.now().Before(cached.refreshAt) {
		p.accessSequence++
		cached.lastAccess = p.accessSequence
		p.cache[orgID] = cached
		p.mu.Unlock()
		return cached.value, nil
	}
	if flight, ok := p.inFlight[orgID]; ok {
		p.mu.Unlock()
		select {
		case <-ctx.Done():
			return "", fmt.Errorf("data plane token: wait for issuer: %w", ctx.Err())
		case <-flight.done:
			return flight.token, flight.err
		}
	}
	flight := &tokenMintFlight{done: make(chan struct{})}
	p.inFlight[orgID] = flight
	p.mu.Unlock()

	token, refreshAt, err := p.mint(ctx, orgID)
	p.mu.Lock()
	if err == nil {
		p.storeTokenLocked(orgID, token, refreshAt)
	}
	flight.token = token
	flight.err = err
	delete(p.inFlight, orgID)
	close(flight.done)
	p.mu.Unlock()
	return token, err
}

func (p *AuthCoreTokenProvider) mint(ctx context.Context, orgID string) (string, time.Time, error) {
	body, err := json.Marshal(map[string]any{
		"orgId": orgID, "scopes": []string{dataPlaneWriteScope},
		"reason": "finspo-core durable SharePoint ingestion",
	})
	if err != nil {
		return "", time.Time{}, fmt.Errorf("data plane token: encode request: %w", err)
	}
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, p.authCoreURL+"/api/data-plane/internal-token", bytes.NewReader(body))
	if err != nil {
		return "", time.Time{}, fmt.Errorf("data plane token: build request: %w", err)
	}
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("X-Service-ID", p.serviceID)
	req.Header.Set("X-Service-API-Key", p.serviceAPIKey)
	resp, err := p.httpClient.Do(req)
	if err != nil {
		return "", time.Time{}, fmt.Errorf("data plane token: issuer unavailable: %w", err)
	}
	defer resp.Body.Close()
	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		return "", time.Time{}, fmt.Errorf("data plane token: issuer returned HTTP %d", resp.StatusCode)
	}
	var bundle tokenBundle
	if err := json.NewDecoder(io.LimitReader(resp.Body, 1<<20)).Decode(&bundle); err != nil {
		return "", time.Time{}, fmt.Errorf("data plane token: issuer returned invalid JSON")
	}
	refreshAt, err := validateBundle(bundle, p.now())
	if err != nil {
		return "", time.Time{}, err
	}
	return bundle.Token, refreshAt, nil
}

func (p *AuthCoreTokenProvider) storeTokenLocked(orgID, token string, refreshAt time.Time) {
	capacity := p.maxCacheEntries
	if capacity <= 0 {
		capacity = maximumTokenCacheEntries
	}
	if _, exists := p.cache[orgID]; !exists && len(p.cache) >= capacity {
		var evictOrg string
		var oldestAccess uint64
		for candidate, cached := range p.cache {
			if evictOrg == "" || cached.lastAccess < oldestAccess {
				evictOrg = candidate
				oldestAccess = cached.lastAccess
			}
		}
		delete(p.cache, evictOrg)
	}
	p.accessSequence++
	p.cache[orgID] = cachedToken{value: token, refreshAt: refreshAt, lastAccess: p.accessSequence}
}

func validateBundle(bundle tokenBundle, now time.Time) (time.Time, error) {
	if strings.TrimSpace(bundle.Token) == "" || strings.TrimSpace(bundle.Issuer) == "" || bundle.Audience != dataPlaneAudience {
		return time.Time{}, fmt.Errorf("data plane token: issuer response violated the audience contract")
	}
	claimedLifetime := time.Duration(bundle.ExpiresInSeconds) * time.Second
	if bundle.ExpiresInSeconds <= 0 || claimedLifetime > maximumTokenLifetime {
		return time.Time{}, fmt.Errorf("data plane token: issuer response violated the lifetime contract")
	}
	expiresAt, err := time.Parse(time.RFC3339, bundle.ExpiresAt)
	if err != nil {
		return time.Time{}, fmt.Errorf("data plane token: issuer returned an invalid expiry")
	}
	remaining := expiresAt.Sub(now)
	if remaining <= 0 || remaining > maximumTokenLifetime || absoluteDuration(remaining-claimedLifetime) > 10*time.Second {
		return time.Time{}, fmt.Errorf("data plane token: issuer response violated the expiry contract")
	}
	skew := defaultTokenRefreshSkew
	if skew >= remaining {
		skew = remaining / 5
	}
	return expiresAt.Add(-skew), nil
}

func absoluteDuration(value time.Duration) time.Duration {
	if value < 0 {
		return -value
	}
	return value
}

func (p *AuthCoreTokenProvider) Invalidate(orgID, rejectedToken string) {
	if p == nil {
		return
	}
	orgID = strings.TrimSpace(orgID)
	p.mu.Lock()
	defer p.mu.Unlock()
	if cached, ok := p.cache[orgID]; ok && cached.value == rejectedToken {
		delete(p.cache, orgID)
	}
}
