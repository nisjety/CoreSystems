package sharepoint

import (
	"context"
	"encoding/json"
	"fmt"
	"net/http"
	"strings"
	"time"
)

// PermissionsClient pulls ACL entries from /drives/{drive-id}/items/{item-id}/permissions.
type PermissionsClient struct {
	baseURL       string
	httpClient    *http.Client
	tokenProvider AccessTokenProvider
}

type PermissionsClientConfig struct {
	BaseURL       string
	HTTPClient    *http.Client
	TokenProvider AccessTokenProvider
}

func NewPermissionsClient(cfg PermissionsClientConfig) *PermissionsClient {
	baseURL := strings.TrimSpace(cfg.BaseURL)
	if baseURL == "" {
		baseURL = defaultGraphBaseURL
	}
	httpClient := cfg.HTTPClient
	if httpClient == nil {
		httpClient = &http.Client{Timeout: 15 * time.Second}
	}
	return &PermissionsClient{
		baseURL:       strings.TrimRight(baseURL, "/"),
		httpClient:    httpClient,
		tokenProvider: cfg.TokenProvider,
	}
}

// ListItemPermissions returns every permission entry attached to a DriveItem.
// Pagination is handled internally; callers receive the fully flattened slice.
//
// The Graph permissions response can be sensitive (it leaks principal names),
// so finspo persists only a normalized summary downstream — see
// store.Permissions.ReplaceAll. Callers MUST still re-verify access at
// retrieval time; the persisted summary is for governance reporting, not authz.
func (c *PermissionsClient) ListItemPermissions(ctx context.Context, organizationID, driveID, itemID string) ([]PermissionEntry, error) {
	if c.tokenProvider == nil {
		return nil, ErrNotConfigured
	}
	token, err := c.tokenProvider.AccessToken(ctx, organizationID)
	if err != nil {
		return nil, fmt.Errorf("resolve access token: %w", err)
	}
	if strings.TrimSpace(token) == "" {
		return nil, ErrNotConfigured
	}

	url := c.baseURL + "/v1.0/drives/" + driveID + "/items/" + itemID + "/permissions"
	var out []PermissionEntry

	for url != "" {
		req, err := http.NewRequestWithContext(ctx, http.MethodGet, url, nil)
		if err != nil {
			return nil, fmt.Errorf("create permissions request: %w", err)
		}
		req.Header.Set("Authorization", "Bearer "+token)
		req.Header.Set("Accept", "application/json")

		resp, err := c.httpClient.Do(req)
		if err != nil {
			return nil, fmt.Errorf("permissions request: %w", err)
		}

		if resp.StatusCode != http.StatusOK {
			err := readGraphError("permissions request", resp)
			resp.Body.Close()
			return nil, err
		}

		var page struct {
			Value         []PermissionEntry `json:"value"`
			ODataNextLink string            `json:"@odata.nextLink"`
		}
		if decodeErr := json.NewDecoder(resp.Body).Decode(&page); decodeErr != nil {
			resp.Body.Close()
			return nil, fmt.Errorf("decode permissions response: %w", decodeErr)
		}
		resp.Body.Close()

		out = append(out, page.Value...)
		url = page.ODataNextLink
	}

	return out, nil
}
