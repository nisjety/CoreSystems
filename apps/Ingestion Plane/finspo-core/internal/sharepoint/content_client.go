package sharepoint

import (
	"context"
	"fmt"
	"io"
	"net/http"
	"strings"
	"time"
)

// DefaultMaxContentBytes caps how many bytes we download per file. SharePoint
// libraries routinely hold multi-GB assets (videos, disk images); we only want
// text-extractable documents, so a 25 MiB ceiling keeps a single pathological
// file from blowing out memory or the ingest budget.
const DefaultMaxContentBytes int64 = 25 * 1024 * 1024

// ContentClient downloads the primary file stream of a DriveItem from Microsoft
// Graph. Graph answers GET /content with a 302 to a short-lived preauthenticated
// URL; Go's http.Client follows the redirect automatically and (since it points
// at a different host) drops the Authorization header, which is exactly what the
// preauthenticated CDN URL expects.
type ContentClient struct {
	baseURL       string
	httpClient    *http.Client
	tokenProvider AccessTokenProvider
	maxBytes      int64
}

type ContentClientConfig struct {
	BaseURL       string
	HTTPClient    *http.Client
	TokenProvider AccessTokenProvider
	// MaxBytes caps a single download. Zero uses DefaultMaxContentBytes.
	MaxBytes int64
}

func NewContentClient(cfg ContentClientConfig) *ContentClient {
	baseURL := strings.TrimSpace(cfg.BaseURL)
	if baseURL == "" {
		baseURL = defaultGraphBaseURL
	}
	httpClient := cfg.HTTPClient
	if httpClient == nil {
		// Downloads can be larger than metadata calls; give them room.
		httpClient = &http.Client{Timeout: 120 * time.Second}
	}
	maxBytes := cfg.MaxBytes
	if maxBytes <= 0 {
		maxBytes = DefaultMaxContentBytes
	}
	return &ContentClient{
		baseURL:       strings.TrimRight(baseURL, "/"),
		httpClient:    httpClient,
		tokenProvider: cfg.TokenProvider,
		maxBytes:      maxBytes,
	}
}

// MaxBytes exposes the configured per-file download ceiling so callers can skip
// oversized items using the metadata size before spending a download.
func (c *ContentClient) MaxBytes() int64 { return c.maxBytes }

// DownloadContent fetches the raw bytes of one DriveItem, following the Graph
// 302 to the preauthenticated download URL. It refuses payloads larger than the
// configured ceiling (reading one byte past the limit to detect truncation).
func (c *ContentClient) DownloadContent(ctx context.Context, organizationID, driveID, itemID string) ([]byte, error) {
	if c.tokenProvider == nil {
		return nil, ErrNotConfigured
	}
	if strings.TrimSpace(driveID) == "" || strings.TrimSpace(itemID) == "" {
		return nil, fmt.Errorf("driveID and itemID are required")
	}
	token, err := c.tokenProvider.AccessToken(ctx, organizationID)
	if err != nil {
		return nil, fmt.Errorf("resolve access token: %w", err)
	}
	if strings.TrimSpace(token) == "" {
		return nil, ErrNotConfigured
	}

	url := c.baseURL + "/v1.0/drives/" + driveID + "/items/" + itemID + "/content"
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, url, nil)
	if err != nil {
		return nil, fmt.Errorf("create content request: %w", err)
	}
	req.Header.Set("Authorization", "Bearer "+token)

	resp, err := c.httpClient.Do(req)
	if err != nil {
		return nil, fmt.Errorf("content request: %w", err)
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		return nil, readGraphError("content request", resp)
	}

	// Read one byte past the ceiling so we can distinguish "exactly at limit"
	// from "over limit" and reject the latter rather than silently truncating.
	limited := io.LimitReader(resp.Body, c.maxBytes+1)
	data, err := io.ReadAll(limited)
	if err != nil {
		return nil, fmt.Errorf("read content body: %w", err)
	}
	if int64(len(data)) > c.maxBytes {
		return nil, fmt.Errorf("content exceeds max download size of %d bytes", c.maxBytes)
	}
	return data, nil
}
