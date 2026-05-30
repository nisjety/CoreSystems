package sharepoint

import (
	"context"
	"encoding/json"
	"fmt"
	"net/http"
	"strings"
	"time"
)

// DeltaClient pulls DriveItem changes from the Microsoft Graph delta endpoint.
// Callers pass either a fresh drive id (for an initial full crawl) or a
// resume URL — typically the @odata.deltaLink returned by a prior sync.
type DeltaClient struct {
	baseURL       string
	httpClient    *http.Client
	tokenProvider AccessTokenProvider
}

type DeltaClientConfig struct {
	BaseURL       string
	HTTPClient    *http.Client
	TokenProvider AccessTokenProvider
}

func NewDeltaClient(cfg DeltaClientConfig) *DeltaClient {
	baseURL := strings.TrimSpace(cfg.BaseURL)
	if baseURL == "" {
		baseURL = defaultGraphBaseURL
	}
	httpClient := cfg.HTTPClient
	if httpClient == nil {
		httpClient = &http.Client{Timeout: 30 * time.Second}
	}
	return &DeltaClient{
		baseURL:       strings.TrimRight(baseURL, "/"),
		httpClient:    httpClient,
		tokenProvider: cfg.TokenProvider,
	}
}

// InitialDeltaURL builds the starting URL for a brand-new sync against a drive.
func (c *DeltaClient) InitialDeltaURL(driveID string) string {
	return c.baseURL + "/v1.0/drives/" + driveID + "/root/delta"
}

// Fetch performs ONE GET against a delta URL (either initial or @odata.nextLink
// or @odata.deltaLink) and decodes the response. Pagination is the caller's
// responsibility: follow NextLink until empty, then save DeltaLink.
func (c *DeltaClient) Fetch(ctx context.Context, organizationID, deltaURL string) (DeltaPage, error) {
	if c.tokenProvider == nil {
		return DeltaPage{}, ErrNotConfigured
	}
	token, err := c.tokenProvider.AccessToken(ctx, organizationID)
	if err != nil {
		return DeltaPage{}, fmt.Errorf("resolve access token: %w", err)
	}
	if strings.TrimSpace(token) == "" {
		return DeltaPage{}, ErrNotConfigured
	}

	req, err := http.NewRequestWithContext(ctx, http.MethodGet, deltaURL, nil)
	if err != nil {
		return DeltaPage{}, fmt.Errorf("create delta request: %w", err)
	}
	req.Header.Set("Authorization", "Bearer "+token)
	req.Header.Set("Accept", "application/json")

	resp, err := c.httpClient.Do(req)
	if err != nil {
		return DeltaPage{}, fmt.Errorf("delta request: %w", err)
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		return DeltaPage{}, readGraphError("delta request", resp)
	}

	var raw struct {
		Value         []json.RawMessage `json:"value"`
		ODataNextLink string            `json:"@odata.nextLink"`
		ODataDeltaLink string           `json:"@odata.deltaLink"`
	}
	if err := json.NewDecoder(resp.Body).Decode(&raw); err != nil {
		return DeltaPage{}, fmt.Errorf("decode delta response: %w", err)
	}

	items := make([]DriveItem, 0, len(raw.Value))
	for i, payload := range raw.Value {
		var item DriveItem
		if err := json.Unmarshal(payload, &item); err != nil {
			return DeltaPage{}, fmt.Errorf("decode delta item %d: %w", i, err)
		}
		items = append(items, item)
	}

	return DeltaPage{
		Items:     items,
		NextLink:  raw.ODataNextLink,
		DeltaLink: raw.ODataDeltaLink,
	}, nil
}
