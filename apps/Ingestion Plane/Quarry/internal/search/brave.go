package search

import (
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"strings"
	"time"
)

type SearchType string

const (
	SearchTypeWeb    SearchType = "web"
	SearchTypeNews   SearchType = "news"
	SearchTypeImages SearchType = "images"
)

type SearchOptions struct {
	Query         string
	Limit         int
	Site          string
	Country       string
	SearchLang    string
	UILang        string
	Freshness     string
	SafeSearch    string
	ExtraSnippets bool
}

type Result struct {
	Title   string  `json:"title"`
	URL     string  `json:"url"`
	Snippet string  `json:"snippet,omitempty"`
	Source  string  `json:"source"`
	Type    string  `json:"type"`
	Score   float64 `json:"score,omitempty"`
}

type APIError struct {
	StatusCode int
	Message    string
}

func (e *APIError) Error() string {
	if e == nil {
		return ""
	}
	return e.Message
}

type BraveClient struct {
	apiKey     string
	baseURL    string
	httpClient *http.Client
}

func NewBraveClient(apiKey, baseURL string, timeout time.Duration) *BraveClient {
	return NewBraveClientWithHTTPClient(apiKey, baseURL, nil, timeout)
}

// NewBraveClientWithHTTPClient lets callers provide a custom transport while
// preserving the same normalization and auth behavior as production requests.
func NewBraveClientWithHTTPClient(apiKey, baseURL string, httpClient *http.Client, timeout time.Duration) *BraveClient {
	if strings.TrimSpace(baseURL) == "" {
		baseURL = "https://api.search.brave.com"
	}
	if httpClient == nil {
		if timeout <= 0 {
			timeout = 10 * time.Second
		}
		httpClient = &http.Client{Timeout: timeout}
	}
	return &BraveClient{
		apiKey:     strings.TrimSpace(apiKey),
		baseURL:    strings.TrimRight(baseURL, "/"),
		httpClient: httpClient,
	}
}

func (c *BraveClient) Enabled() bool {
	return c != nil && c.apiKey != ""
}

func (c *BraveClient) Name() string { return "brave" }

func (c *BraveClient) Search(ctx context.Context, searchType SearchType, opts SearchOptions) ([]Result, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	if c == nil || !c.Enabled() {
		return nil, fmt.Errorf("brave search is not configured")
	}
	if strings.TrimSpace(opts.Query) == "" {
		return nil, fmt.Errorf("query is required")
	}

	endpoint, err := c.endpointFor(searchType)
	if err != nil {
		return nil, err
	}

	reqURL, err := url.Parse(c.baseURL + endpoint)
	if err != nil {
		return nil, fmt.Errorf("build brave request url: %w", err)
	}

	query := strings.TrimSpace(opts.Query)
	if site := strings.TrimSpace(opts.Site); site != "" {
		query = "site:" + site + " " + query
	}

	params := reqURL.Query()
	params.Set("q", query)
	params.Set("count", fmt.Sprintf("%d", normalizedLimit(opts.Limit)))
	if country := strings.TrimSpace(opts.Country); country != "" {
		params.Set("country", country)
	}
	if searchLang := strings.TrimSpace(opts.SearchLang); searchLang != "" {
		params.Set("search_lang", searchLang)
	}
	if uiLang := strings.TrimSpace(opts.UILang); uiLang != "" {
		params.Set("ui_lang", uiLang)
	}
	if freshness := strings.TrimSpace(opts.Freshness); freshness != "" {
		params.Set("freshness", freshness)
	}
	if safeSearch := strings.TrimSpace(opts.SafeSearch); safeSearch != "" {
		params.Set("safesearch", safeSearch)
	}
	if opts.ExtraSnippets {
		params.Set("extra_snippets", "true")
	}
	reqURL.RawQuery = params.Encode()

	httpReq, err := http.NewRequestWithContext(ctx, http.MethodGet, reqURL.String(), nil)
	if err != nil {
		return nil, fmt.Errorf("create brave request: %w", err)
	}
	httpReq.Header.Set("Accept", "application/json")
	httpReq.Header.Set("X-Subscription-Token", c.apiKey)

	resp, err := c.httpClient.Do(httpReq)
	if err != nil {
		return nil, fmt.Errorf("execute brave request: %w", err)
	}
	defer resp.Body.Close()

	body, err := io.ReadAll(io.LimitReader(resp.Body, 2<<20))
	if err != nil {
		return nil, fmt.Errorf("read brave response: %w", err)
	}
	if resp.StatusCode < http.StatusOK || resp.StatusCode >= http.StatusMultipleChoices {
		return nil, &APIError{
			StatusCode: resp.StatusCode,
			Message:    fmt.Sprintf("brave search returned status %d: %s", resp.StatusCode, strings.TrimSpace(string(body))),
		}
	}

	return decodeResults(searchType, body)
}

func (c *BraveClient) endpointFor(searchType SearchType) (string, error) {
	switch searchType {
	case SearchTypeWeb:
		return "/res/v1/web/search", nil
	case SearchTypeNews:
		return "/res/v1/news/search", nil
	case SearchTypeImages:
		return "/res/v1/images/search", nil
	default:
		return "", fmt.Errorf("unsupported search type: %s", searchType)
	}
}

func decodeResults(searchType SearchType, payload []byte) ([]Result, error) {
	var raw map[string]interface{}
	if err := json.Unmarshal(payload, &raw); err != nil {
		return nil, fmt.Errorf("decode brave payload: %w", err)
	}

	records := firstResultArray(raw,
		[]string{string(searchType), "results"},
		[]string{"results"},
	)

	results := make([]Result, 0, len(records))
	for _, record := range records {
		urlValue := pickString(record, "url", "page_url", "thumbnail", "source_url")
		title := pickString(record, "title", "name")
		snippet := pickString(record, "description", "snippet", "page_fetched")
		if urlValue == "" {
			continue
		}
		if title == "" {
			title = urlValue
		}
		results = append(results, Result{
			Title:   title,
			URL:     urlValue,
			Snippet: snippet,
			Source:  "brave",
			Type:    string(searchType),
		})
	}

	return results, nil
}

func firstResultArray(root map[string]interface{}, paths ...[]string) []map[string]interface{} {
	for _, path := range paths {
		value := walk(root, path...)
		if items := asMapSlice(value); len(items) > 0 {
			return items
		}
	}
	return nil
}

func walk(value interface{}, path ...string) interface{} {
	current := value
	for _, segment := range path {
		asMap, ok := current.(map[string]interface{})
		if !ok {
			return nil
		}
		current = asMap[segment]
	}
	return current
}

func asMapSlice(value interface{}) []map[string]interface{} {
	items, ok := value.([]interface{})
	if !ok {
		return nil
	}
	out := make([]map[string]interface{}, 0, len(items))
	for _, item := range items {
		entry, ok := item.(map[string]interface{})
		if ok {
			out = append(out, entry)
		}
	}
	return out
}

func pickString(record map[string]interface{}, keys ...string) string {
	for _, key := range keys {
		raw, ok := record[key]
		if !ok {
			continue
		}
		switch value := raw.(type) {
		case string:
			if strings.TrimSpace(value) != "" {
				return strings.TrimSpace(value)
			}
		case map[string]interface{}:
			if nested := pickString(value, "url", "link", "src"); nested != "" {
				return nested
			}
		}
	}
	return ""
}

func normalizedLimit(limit int) int {
	switch {
	case limit <= 0:
		return 10
	case limit > 20:
		return 20
	default:
		return limit
	}
}
