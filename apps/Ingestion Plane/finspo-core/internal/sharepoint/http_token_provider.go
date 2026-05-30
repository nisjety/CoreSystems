package sharepoint

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"net/http"
)

// HttpAccessTokenProvider fetches access tokens from the integration-core token broker.
type HttpAccessTokenProvider struct {
	integrationCoreURL string
	internalAPIKey     string
	httpClient         *http.Client
}

// NewHttpAccessTokenProvider creates an HttpAccessTokenProvider using the given
// integration-core base URL and internal API key.
func NewHttpAccessTokenProvider(integrationCoreURL, internalAPIKey string) *HttpAccessTokenProvider {
	return &HttpAccessTokenProvider{
		integrationCoreURL: integrationCoreURL,
		internalAPIKey:     internalAPIKey,
		httpClient:         &http.Client{},
	}
}

// AccessToken calls POST /internal/connectors/token and returns the bearer token.
func (p *HttpAccessTokenProvider) AccessToken(ctx context.Context, organizationID string) (string, error) {
	body, err := json.Marshal(map[string]string{
		"organizationId": organizationID,
		"connectorType":  "microsoft-graph",
	})
	if err != nil {
		return "", fmt.Errorf("marshal token request: %w", err)
	}

	req, err := http.NewRequestWithContext(ctx, http.MethodPost,
		p.integrationCoreURL+"/internal/connectors/token",
		bytes.NewReader(body))
	if err != nil {
		return "", fmt.Errorf("create token request: %w", err)
	}
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("X-Internal-API-Key", p.internalAPIKey)

	resp, err := p.httpClient.Do(req)
	if err != nil {
		return "", fmt.Errorf("token broker request: %w", err)
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		return "", fmt.Errorf("token broker returned status %d", resp.StatusCode)
	}

	var result struct {
		Data struct {
			AccessToken string `json:"accessToken"`
		} `json:"data"`
	}
	if err := json.NewDecoder(resp.Body).Decode(&result); err != nil {
		return "", fmt.Errorf("decode token response: %w", err)
	}
	if result.Data.AccessToken == "" {
		return "", fmt.Errorf("token broker returned empty access token")
	}
	return result.Data.AccessToken, nil
}
