package handoff

import (
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"strings"
	"sync"
	"time"

	"github.com/triodelab/integration-corev2/internal/config"
)

const (
	dataPlaneAudience        = "data-plane"
	documentsWriteScope      = "documents:write"
	tokenRefreshSkew         = 30 * time.Second
	maximumTokenLifetime     = 10 * time.Minute
	maximumTokenCacheEntries = 10_000
)

type DataPlaneDocumentsConfig struct {
	BaseURL       string
	AuthCoreURL   string
	ServiceID     string
	ServiceAPIKey string
	HTTPClient    *http.Client
	now           func() time.Time
}

type DataPlaneDocumentsClient struct {
	baseURL    string
	tokens     *dataPlaneTokenProvider
	httpClient httpClient
}

type DataPlaneDocumentRequest struct {
	OrgID     string         `json:"org_id"`
	Source    string         `json:"source"`
	Type      string         `json:"type"`
	Title     string         `json:"title"`
	Content   string         `json:"content,omitempty"`
	Metadata  map[string]any `json:"metadata,omitempty"`
	CreatedBy string         `json:"created_by,omitempty"`
}

type DataPlaneDocument struct {
	ID         string         `json:"id,omitempty"`
	DocumentID string         `json:"document_id,omitempty"`
	OrgID      string         `json:"org_id,omitempty"`
	Source     string         `json:"source,omitempty"`
	Type       string         `json:"type,omitempty"`
	Title      string         `json:"title,omitempty"`
	Metadata   map[string]any `json:"metadata,omitempty"`
}

type dataPlaneTokenProvider struct {
	authCoreURL     string
	serviceID       string
	serviceAPIKey   string
	httpClient      httpClient
	now             func() time.Time
	mu              sync.Mutex
	cache           map[string]cachedDataPlaneToken
	inFlight        map[string]*dataPlaneTokenFlight
	maxCacheEntries int
	accessSequence  uint64
}

type cachedDataPlaneToken struct {
	value      string
	refreshAt  time.Time
	lastAccess uint64
}

type dataPlaneTokenFlight struct {
	done  chan struct{}
	token string
	err   error
}

type dataPlaneTokenBundle struct {
	Token            string `json:"token"`
	ExpiresAt        string `json:"expiresAt"`
	ExpiresInSeconds int    `json:"expiresInSeconds"`
	Issuer           string `json:"issuer"`
	Audience         string `json:"audience"`
}

func NewDataPlaneDocumentsClient(cfg DataPlaneDocumentsConfig) *DataPlaneDocumentsClient {
	downstreamClient := cfg.HTTPClient
	if downstreamClient == nil {
		downstreamClient = &http.Client{Timeout: defaultTimeout}
	}
	downstreamClient = &http.Client{
		Transport: downstreamClient.Transport,
		Timeout:   downstreamClient.Timeout,
		CheckRedirect: func(_ *http.Request, _ []*http.Request) error {
			return http.ErrUseLastResponse
		},
	}
	issuerClient := &http.Client{
		Transport: downstreamClient.Transport,
		Timeout:   downstreamClient.Timeout,
		CheckRedirect: func(_ *http.Request, _ []*http.Request) error {
			return http.ErrUseLastResponse
		},
	}
	now := cfg.now
	if now == nil {
		now = time.Now
	}
	return &DataPlaneDocumentsClient{
		baseURL: normalizeBaseURL(cfg.BaseURL),
		tokens: &dataPlaneTokenProvider{
			authCoreURL: normalizeBaseURL(cfg.AuthCoreURL), serviceID: strings.TrimSpace(cfg.ServiceID),
			serviceAPIKey: strings.TrimSpace(cfg.ServiceAPIKey), httpClient: issuerClient, now: now,
			cache: make(map[string]cachedDataPlaneToken), inFlight: make(map[string]*dataPlaneTokenFlight),
			maxCacheEntries: maximumTokenCacheEntries,
		},
		httpClient: downstreamClient,
	}
}

func NewDataPlaneDocumentsClientFromConfig(cfg config.Config, httpClient *http.Client) *DataPlaneDocumentsClient {
	return NewDataPlaneDocumentsClient(DataPlaneDocumentsConfig{
		BaseURL: cfg.DataPlaneDocumentsURL, AuthCoreURL: cfg.AuthCoreURL,
		ServiceID: cfg.IntegrationServiceID, ServiceAPIKey: cfg.IntegrationServiceAPIKey,
		HTTPClient: httpClient,
	})
}

func (p *dataPlaneTokenProvider) configured() bool {
	return p != nil && p.authCoreURL != "" && p.serviceID != "" && p.serviceAPIKey != ""
}

func (p *dataPlaneTokenProvider) token(ctx context.Context, orgID string) (string, error) {
	orgID = strings.TrimSpace(orgID)
	if orgID == "" {
		return "", fmt.Errorf("data-plane token: verified organization is required")
	}
	if !p.configured() {
		return "", ErrNotConfigured
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
			return "", fmt.Errorf("data-plane token: wait for issuer: %w", ctx.Err())
		case <-flight.done:
			return flight.token, flight.err
		}
	}
	flight := &dataPlaneTokenFlight{done: make(chan struct{})}
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

func (p *dataPlaneTokenProvider) mint(ctx context.Context, orgID string) (string, time.Time, error) {
	req, err := jsonRequest(ctx, http.MethodPost, p.authCoreURL+"/api/data-plane/internal-token", map[string]any{
		"orgId": orgID, "scopes": []string{documentsWriteScope},
		"reason": "integration-corev2 durable ingestion",
	})
	if err != nil {
		return "", time.Time{}, fmt.Errorf("data-plane token: build request: %w", err)
	}
	req.Header.Set("X-Service-ID", p.serviceID)
	req.Header.Set("X-Service-API-Key", p.serviceAPIKey)
	resp, err := p.httpClient.Do(req)
	if err != nil {
		return "", time.Time{}, fmt.Errorf("data-plane token: issuer unavailable: %w", err)
	}
	defer resp.Body.Close()
	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		return "", time.Time{}, fmt.Errorf("data-plane token: issuer returned HTTP %d", resp.StatusCode)
	}
	var bundle dataPlaneTokenBundle
	if err := json.NewDecoder(io.LimitReader(resp.Body, 1<<20)).Decode(&bundle); err != nil {
		return "", time.Time{}, fmt.Errorf("data-plane token: issuer returned invalid JSON")
	}
	refreshAt, err := validateTokenBundle(bundle, p.now())
	if err != nil {
		return "", time.Time{}, err
	}
	return bundle.Token, refreshAt, nil
}

func (p *dataPlaneTokenProvider) storeTokenLocked(orgID, token string, refreshAt time.Time) {
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
	p.cache[orgID] = cachedDataPlaneToken{value: token, refreshAt: refreshAt, lastAccess: p.accessSequence}
}

func validateTokenBundle(bundle dataPlaneTokenBundle, now time.Time) (time.Time, error) {
	if strings.TrimSpace(bundle.Token) == "" || strings.TrimSpace(bundle.Issuer) == "" || bundle.Audience != dataPlaneAudience {
		return time.Time{}, fmt.Errorf("data-plane token: issuer response violated the audience contract")
	}
	if bundle.ExpiresInSeconds <= 0 || time.Duration(bundle.ExpiresInSeconds)*time.Second > maximumTokenLifetime {
		return time.Time{}, fmt.Errorf("data-plane token: issuer response violated the lifetime contract")
	}
	expiresAt, err := time.Parse(time.RFC3339, bundle.ExpiresAt)
	if err != nil {
		return time.Time{}, fmt.Errorf("data-plane token: issuer returned an invalid expiry")
	}
	remaining := expiresAt.Sub(now)
	if remaining <= 0 || remaining > maximumTokenLifetime || absDuration(remaining-time.Duration(bundle.ExpiresInSeconds)*time.Second) > 10*time.Second {
		return time.Time{}, fmt.Errorf("data-plane token: issuer response violated the expiry contract")
	}
	skew := tokenRefreshSkew
	if skew >= remaining {
		skew = remaining / 5
	}
	return expiresAt.Add(-skew), nil
}

func absDuration(value time.Duration) time.Duration {
	if value < 0 {
		return -value
	}
	return value
}

func (p *dataPlaneTokenProvider) invalidate(orgID, token string) {
	if p == nil {
		return
	}
	orgID = strings.TrimSpace(orgID)
	p.mu.Lock()
	defer p.mu.Unlock()
	if cached, ok := p.cache[orgID]; ok && cached.value == token {
		delete(p.cache, orgID)
	}
}

// CreateDocument uses an Auth Core minted, short-lived token bound to the
// verified organization in the durable handoff record. No caller-selected
// identity headers or durable service credentials cross into Data Plane.
func (c *DataPlaneDocumentsClient) CreateDocument(ctx context.Context, input DataPlaneDocumentRequest) (DataPlaneDocument, error) {
	if !c.Configured() {
		return DataPlaneDocument{}, ErrNotConfigured
	}
	orgID := strings.TrimSpace(input.OrgID)
	if orgID == "" {
		return DataPlaneDocument{}, fmt.Errorf("data-plane documents: verified organization is required")
	}
	input.OrgID = orgID
	for attempt := 0; attempt < 2; attempt++ {
		token, err := c.tokens.token(ctx, orgID)
		if err != nil {
			return DataPlaneDocument{}, err
		}
		req, err := jsonRequest(ctx, http.MethodPost, c.baseURL+"/v1/documents", input)
		if err != nil {
			return DataPlaneDocument{}, err
		}
		req.Header.Set("Authorization", "Bearer "+token)
		resp, err := c.httpClient.Do(req)
		if err != nil {
			return DataPlaneDocument{}, fmt.Errorf("data-plane documents request failed: %w", err)
		}
		if resp.StatusCode == http.StatusUnauthorized && attempt == 0 {
			_, _ = io.Copy(io.Discard, io.LimitReader(resp.Body, 4096))
			_ = resp.Body.Close()
			c.tokens.invalidate(orgID, token)
			continue
		}
		return decodeJSONOrEnvelope[DataPlaneDocument]("data-plane documents", resp)
	}
	return DataPlaneDocument{}, fmt.Errorf("data-plane documents authentication failed after refresh")
}

func (c *DataPlaneDocumentsClient) Configured() bool {
	return c != nil && c.baseURL != "" && c.tokens.configured()
}
