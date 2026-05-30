package sharepoint

import (
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"path"
	"strings"
	"time"
)

const defaultGraphBaseURL = "https://graph.microsoft.com"

type AccessTokenProvider interface {
	AccessToken(ctx context.Context, organizationID string) (string, error)
}

type GraphBrowserConfig struct {
	BaseURL       string
	HTTPClient    *http.Client
	TokenProvider AccessTokenProvider
}

type GraphBrowser struct {
	baseURL       string
	httpClient    *http.Client
	tokenProvider AccessTokenProvider
}

func NewGraphBrowser(cfg GraphBrowserConfig) *GraphBrowser {
	baseURL := strings.TrimSpace(cfg.BaseURL)
	if baseURL == "" {
		baseURL = defaultGraphBaseURL
	}

	httpClient := cfg.HTTPClient
	if httpClient == nil {
		httpClient = &http.Client{Timeout: 15 * time.Second}
	}

	return &GraphBrowser{
		baseURL:       strings.TrimRight(baseURL, "/"),
		httpClient:    httpClient,
		tokenProvider: cfg.TokenProvider,
	}
}

type sitesPage struct {
	Value []struct {
		ID          string `json:"id"`
		Name        string `json:"name"`
		DisplayName string `json:"displayName"`
		WebURL      string `json:"webUrl"`
		Description string `json:"description"`
	} `json:"value"`
	ODataNextLink string `json:"@odata.nextLink"`
}

type itemsPage struct {
	Value []struct {
		Name                 string     `json:"name"`
		Size                 int64      `json:"size"`
		LastModifiedDateTime *time.Time `json:"lastModifiedDateTime"`
		WebURL               string     `json:"webUrl"`
		Folder               *struct{}  `json:"folder,omitempty"`
	} `json:"value"`
	ODataNextLink string `json:"@odata.nextLink"`
}

func (b *GraphBrowser) ListSites(ctx context.Context, organizationID string) ([]Site, error) {
	token, err := b.accessToken(ctx, organizationID)
	if err != nil {
		return nil, err
	}

	var allSites []Site
	nextURL := b.baseURL + "/v1.0/sites?search=*"
	for {
		request, err := http.NewRequestWithContext(ctx, http.MethodGet, nextURL, nil)
		if err != nil {
			return nil, fmt.Errorf("create list sites request: %w", err)
		}
		request.Header.Set("Authorization", "Bearer "+token)

		response, err := b.httpClient.Do(request)
		if err != nil {
			return nil, fmt.Errorf("list sites request failed: %w", err)
		}
		defer response.Body.Close()

		if response.StatusCode != http.StatusOK {
			return nil, readGraphError("list sites request", response)
		}

		var page sitesPage
		if err := json.NewDecoder(response.Body).Decode(&page); err != nil {
			return nil, fmt.Errorf("decode list sites response: %w", err)
		}

		for _, site := range page.Value {
			allSites = append(allSites, Site{
				ID:          site.ID,
				Name:        site.Name,
				DisplayName: site.DisplayName,
				WebURL:      site.WebURL,
				Description: site.Description,
			})
		}

		if page.ODataNextLink == "" {
			break
		}
		nextURL = page.ODataNextLink
	}

	return allSites, nil
}

func (b *GraphBrowser) ListItems(ctx context.Context, organizationID string, siteID string, itemPath string) ([]Item, error) {
	token, err := b.accessToken(ctx, organizationID)
	if err != nil {
		return nil, err
	}

	normalizedPath := normalizeItemPath(itemPath)
	var allItems []Item
	nextURL := b.baseURL + buildDriveChildrenPath(siteID, itemPath)
	for {
		request, err := http.NewRequestWithContext(ctx, http.MethodGet, nextURL, nil)
		if err != nil {
			return nil, fmt.Errorf("create list items request: %w", err)
		}
		request.Header.Set("Authorization", "Bearer "+token)

		response, err := b.httpClient.Do(request)
		if err != nil {
			return nil, fmt.Errorf("list items request failed: %w", err)
		}
		defer response.Body.Close()

		if response.StatusCode != http.StatusOK {
			return nil, readGraphError("list items request", response)
		}

		var page itemsPage
		if err := json.NewDecoder(response.Body).Decode(&page); err != nil {
			return nil, fmt.Errorf("decode list items response: %w", err)
		}

		for _, item := range page.Value {
			resolvedPath := path.Join(normalizedPath, item.Name)
			if !strings.HasPrefix(resolvedPath, "/") {
				resolvedPath = "/" + resolvedPath
			}

			var modifiedTime time.Time
			if item.LastModifiedDateTime != nil {
				modifiedTime = *item.LastModifiedDateTime
			}

			allItems = append(allItems, Item{
				Name:         item.Name,
				Path:         resolvedPath,
				Size:         item.Size,
				IsFolder:     item.Folder != nil,
				ModifiedTime: modifiedTime,
				WebURL:       item.WebURL,
			})
		}

		if page.ODataNextLink == "" {
			break
		}
		nextURL = page.ODataNextLink
	}

	return allItems, nil
}

func (b *GraphBrowser) accessToken(ctx context.Context, organizationID string) (string, error) {
	if b.tokenProvider == nil {
		return "", ErrNotConfigured
	}

	token, err := b.tokenProvider.AccessToken(ctx, organizationID)
	if err != nil {
		return "", fmt.Errorf("resolve access token: %w", err)
	}
	if strings.TrimSpace(token) == "" {
		return "", ErrNotConfigured
	}

	return token, nil
}

func buildDriveChildrenPath(siteID string, itemPath string) string {
	escapedSiteID := strings.TrimSpace(siteID)
	normalizedPath := normalizeItemPath(itemPath)
	if normalizedPath == "/" {
		return "/v1.0/sites/" + escapedSiteID + "/drive/root/children"
	}

	return "/v1.0/sites/" + escapedSiteID + "/drive/root:" + encodeDrivePath(normalizedPath) + ":/children"
}

func normalizeItemPath(itemPath string) string {
	trimmed := strings.TrimSpace(itemPath)
	if trimmed == "" || trimmed == "/" {
		return "/"
	}

	cleaned := path.Clean("/" + strings.TrimPrefix(trimmed, "/"))
	if cleaned == "." {
		return "/"
	}
	return cleaned
}

func encodeDrivePath(itemPath string) string {
	if itemPath == "/" {
		return ""
	}

	segments := strings.Split(strings.TrimPrefix(itemPath, "/"), "/")
	encodedSegments := make([]string, 0, len(segments))
	for _, segment := range segments {
		if segment == "" {
			continue
		}
		encodedSegments = append(encodedSegments, url.PathEscape(segment))
	}

	return "/" + strings.Join(encodedSegments, "/")
}

func readGraphError(operation string, response *http.Response) error {
	body, err := io.ReadAll(io.LimitReader(response.Body, 4096))
	if err != nil {
		return fmt.Errorf("%s returned status %d", operation, response.StatusCode)
	}

	message := strings.TrimSpace(string(body))
	if message == "" {
		return fmt.Errorf("%s returned status %d", operation, response.StatusCode)
	}

	return fmt.Errorf("%s returned status %d: %s", operation, response.StatusCode, message)
}
