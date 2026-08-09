package sharepoint

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"net/http"
	"strings"
	"sync"
	"time"
)

const accessTokenRefreshSkew = time.Minute

type cachedAccessToken struct {
	value     string
	refreshAt time.Time
}

// HttpAccessTokenProvider fetches access tokens from the integration-core token broker.
type HttpAccessTokenProvider struct {
	integrationCoreURL string
	internalAPIKey     string
	httpClient         *http.Client
	mu                 sync.Mutex
	tokens             map[string]cachedAccessToken
	now                func() time.Time
}

// NewHttpAccessTokenProvider creates an HttpAccessTokenProvider using the given
// integration-core base URL and internal API key.
func NewHttpAccessTokenProvider(integrationCoreURL, internalAPIKey string) *HttpAccessTokenProvider {
	return &HttpAccessTokenProvider{
		integrationCoreURL: integrationCoreURL,
		internalAPIKey:     internalAPIKey,
		httpClient:         &http.Client{},
		tokens:             make(map[string]cachedAccessToken),
		now:                time.Now,
	}
}

// AccessToken returns a per-organization bearer token. Unexpired broker leases are
// held only in process memory so a large page sync does not request one lease per page.
func (p *HttpAccessTokenProvider) AccessToken(ctx context.Context, organizationID string) (string, error) {
	organizationID = strings.TrimSpace(organizationID)
	if organizationID == "" {
		return "", fmt.Errorf("organization ID is required")
	}

	p.mu.Lock()
	defer p.mu.Unlock()

	now := p.now().UTC()
	if cached, ok := p.tokens[organizationID]; ok && now.Before(cached.refreshAt) {
		return cached.value, nil
	}

	token, expiresAt, err := p.fetchAccessToken(ctx, organizationID)
	if err != nil {
		return "", err
	}

	refreshAt := expiresAt.Add(-accessTokenRefreshSkew)
	if refreshAt.After(now) {
		p.tokens[organizationID] = cachedAccessToken{value: token, refreshAt: refreshAt}
	} else {
		delete(p.tokens, organizationID)
	}
	return token, nil
}

func (p *HttpAccessTokenProvider) fetchAccessToken(ctx context.Context, organizationID string) (string, time.Time, error) {
	body, err := json.Marshal(map[string]string{
		"organizationId": organizationID,
		"connectorType":  "microsoft-graph",
		"consumer":       "finspo-core",
	})
	if err != nil {
		return "", time.Time{}, fmt.Errorf("marshal token request: %w", err)
	}

	req, err := http.NewRequestWithContext(ctx, http.MethodPost,
		p.integrationCoreURL+"/internal/connectors/token",
		bytes.NewReader(body))
	if err != nil {
		return "", time.Time{}, fmt.Errorf("create token request: %w", err)
	}
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("X-Internal-API-Key", p.internalAPIKey)

	resp, err := p.httpClient.Do(req)
	if err != nil {
		return "", time.Time{}, fmt.Errorf("token broker request: %w", err)
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		return "", time.Time{}, fmt.Errorf("token broker returned status %d", resp.StatusCode)
	}

	var result struct {
		Data struct {
			AccessToken string    `json:"accessToken"`
			ExpiresAt   time.Time `json:"expiresAt"`
		} `json:"data"`
	}
	if err := json.NewDecoder(resp.Body).Decode(&result); err != nil {
		return "", time.Time{}, fmt.Errorf("decode token response: %w", err)
	}
	if result.Data.AccessToken == "" {
		return "", time.Time{}, fmt.Errorf("token broker returned empty access token")
	}
	return result.Data.AccessToken, result.Data.ExpiresAt, nil
}
