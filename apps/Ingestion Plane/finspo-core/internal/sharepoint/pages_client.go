package sharepoint

import (
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"strings"
	"time"

	"github.com/triodelab/finspo/internal/extract"
)

// PagesClient reads SharePoint site pages ("posts") from the Microsoft Graph
// sitePages API. The v1.0 surface addresses pages through an OData cast
// (`/sites/{id}/pages/microsoft.graph.sitePage`); tenants/clouds where that
// has not rolled out still serve the uncast beta path, so every entry-point
// request falls back to beta on 400/404/501 before giving up.
type PagesClient struct {
	baseURL       string
	httpClient    *http.Client
	tokenProvider AccessTokenProvider
}

type PagesClientConfig struct {
	BaseURL       string
	HTTPClient    *http.Client
	TokenProvider AccessTokenProvider
}

func NewPagesClient(cfg PagesClientConfig) *PagesClient {
	baseURL := strings.TrimSpace(cfg.BaseURL)
	if baseURL == "" {
		baseURL = defaultGraphBaseURL
	}
	httpClient := cfg.HTTPClient
	if httpClient == nil {
		httpClient = &http.Client{Timeout: 30 * time.Second}
	}
	return &PagesClient{
		baseURL:       strings.TrimRight(baseURL, "/"),
		httpClient:    httpClient,
		tokenProvider: cfg.TokenProvider,
	}
}

type sitePagesPage struct {
	Value []struct {
		ID                   string     `json:"id"`
		Name                 string     `json:"name"`
		Title                string     `json:"title"`
		Description          string     `json:"description"`
		PageLayout           string     `json:"pageLayout"`
		WebURL               string     `json:"webUrl"`
		LastModifiedDateTime *time.Time `json:"lastModifiedDateTime"`
	} `json:"value"`
	ODataNextLink string `json:"@odata.nextLink"`
}

// ListSitePages returns every site page on a SharePoint site.
func (c *PagesClient) ListSitePages(ctx context.Context, organizationID string, siteID string) ([]SitePage, error) {
	token, err := c.accessToken(ctx, organizationID)
	if err != nil {
		return nil, err
	}

	site := strings.TrimSpace(siteID)
	primary := c.baseURL + "/v1.0/sites/" + site + "/pages/microsoft.graph.sitePage"
	fallback := c.baseURL + "/beta/sites/" + site + "/pages"

	var allPages []SitePage
	nextURL := primary
	first := true
	for {
		body, err := c.getJSON(ctx, token, nextURL)
		if err != nil {
			if first && isPagesAPIUnrolled(err) {
				nextURL = fallback
				first = false
				continue
			}
			return nil, err
		}
		first = false

		var page sitePagesPage
		if err := json.Unmarshal(body, &page); err != nil {
			return nil, fmt.Errorf("decode site pages response: %w", err)
		}
		for _, p := range page.Value {
			allPages = append(allPages, SitePage{
				ID:                   p.ID,
				Name:                 p.Name,
				Title:                p.Title,
				Description:          p.Description,
				PageLayout:           p.PageLayout,
				WebURL:               p.WebURL,
				LastModifiedDateTime: p.LastModifiedDateTime,
			})
		}

		if page.ODataNextLink == "" {
			break
		}
		nextURL = page.ODataNextLink
	}

	return allPages, nil
}

// canvasWebPart is one web part inside a page's canvasLayout. Only text web
// parts carry innerHtml; every other web part kind decodes to an empty string
// and is skipped.
type canvasWebPart struct {
	InnerHTML string `json:"innerHtml"`
}

// canvasColumn tolerates both key casings Graph has shipped for the web part
// collection ("webparts" on v1.0, "webParts" on beta).
type canvasColumn struct {
	Webparts    []canvasWebPart `json:"webparts"`
	WebPartsAlt []canvasWebPart `json:"webParts"`
}

func (c canvasColumn) parts() []canvasWebPart {
	if len(c.Webparts) > 0 {
		return c.Webparts
	}
	return c.WebPartsAlt
}

type canvasLayoutBody struct {
	CanvasLayout struct {
		HorizontalSections []struct {
			Columns []canvasColumn `json:"columns"`
		} `json:"horizontalSections"`
		VerticalSection *canvasColumn `json:"verticalSection"`
	} `json:"canvasLayout"`
}

// FetchPageText fetches one page expanded with its canvasLayout and returns
// the plain-text concatenation of every text web part, in canvas order.
// Pages whose canvas holds no text web parts return "".
func (c *PagesClient) FetchPageText(ctx context.Context, organizationID string, siteID string, pageID string) (string, error) {
	token, err := c.accessToken(ctx, organizationID)
	if err != nil {
		return "", err
	}

	site := strings.TrimSpace(siteID)
	page := url.PathEscape(strings.TrimSpace(pageID))
	primary := c.baseURL + "/v1.0/sites/" + site + "/pages/" + page + "/microsoft.graph.sitePage?$expand=canvasLayout"
	fallback := c.baseURL + "/beta/sites/" + site + "/pages/" + page + "?$expand=canvasLayout"

	body, err := c.getJSON(ctx, token, primary)
	if err != nil {
		if !isPagesAPIUnrolled(err) {
			return "", err
		}
		if body, err = c.getJSON(ctx, token, fallback); err != nil {
			return "", err
		}
	}

	var decoded canvasLayoutBody
	if err := json.Unmarshal(body, &decoded); err != nil {
		return "", fmt.Errorf("decode page canvas response: %w", err)
	}

	var blocks []string
	appendParts := func(parts []canvasWebPart) {
		for _, part := range parts {
			text := strings.TrimSpace(extract.HTMLToText(part.InnerHTML))
			if text != "" {
				blocks = append(blocks, text)
			}
		}
	}
	for _, section := range decoded.CanvasLayout.HorizontalSections {
		for _, column := range section.Columns {
			appendParts(column.parts())
		}
	}
	if decoded.CanvasLayout.VerticalSection != nil {
		appendParts(decoded.CanvasLayout.VerticalSection.parts())
	}

	return strings.Join(blocks, "\n\n"), nil
}

// getJSON performs one authorized GET and returns the raw body on 200, or a
// graphStatusError otherwise so callers can decide whether to fall back.
func (c *PagesClient) getJSON(ctx context.Context, token string, requestURL string) ([]byte, error) {
	request, err := http.NewRequestWithContext(ctx, http.MethodGet, requestURL, nil)
	if err != nil {
		return nil, fmt.Errorf("create site pages request: %w", err)
	}
	request.Header.Set("Authorization", "Bearer "+token)
	request.Header.Set("Accept", "application/json")

	response, err := c.httpClient.Do(request)
	if err != nil {
		return nil, fmt.Errorf("site pages request failed: %w", err)
	}
	defer response.Body.Close()

	if response.StatusCode != http.StatusOK {
		return nil, &graphStatusError{
			status: response.StatusCode,
			cause:  readGraphError("site pages request", response),
		}
	}

	body, err := io.ReadAll(response.Body)
	if err != nil {
		return nil, fmt.Errorf("read site pages response: %w", err)
	}
	return body, nil
}

func (c *PagesClient) accessToken(ctx context.Context, organizationID string) (string, error) {
	if c.tokenProvider == nil {
		return "", ErrNotConfigured
	}
	token, err := c.tokenProvider.AccessToken(ctx, organizationID)
	if err != nil {
		return "", fmt.Errorf("resolve access token: %w", err)
	}
	if strings.TrimSpace(token) == "" {
		return "", ErrNotConfigured
	}
	return token, nil
}

// graphStatusError carries the HTTP status so the v1.0→beta fallback can
// distinguish "endpoint shape unknown to this tenant" from real failures.
type graphStatusError struct {
	status int
	cause  error
}

func (e *graphStatusError) Error() string { return e.cause.Error() }
func (e *graphStatusError) Unwrap() error { return e.cause }

func isPagesAPIUnrolled(err error) bool {
	statusErr, ok := err.(*graphStatusError)
	if !ok {
		return false
	}
	switch statusErr.status {
	case http.StatusBadRequest, http.StatusNotFound, http.StatusNotImplemented:
		return true
	}
	return false
}
