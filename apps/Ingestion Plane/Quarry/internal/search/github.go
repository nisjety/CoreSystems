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

type GitHubClient struct {
	baseURL    string
	token      string
	httpClient *http.Client
}

func NewGitHubClient(token, baseURL string, timeout time.Duration) *GitHubClient {
	return NewGitHubClientWithHTTPClient(token, baseURL, nil, timeout)
}

func NewGitHubClientWithHTTPClient(token, baseURL string, httpClient *http.Client, timeout time.Duration) *GitHubClient {
	if strings.TrimSpace(baseURL) == "" {
		baseURL = "https://api.github.com"
	}
	if httpClient == nil {
		if timeout <= 0 {
			timeout = 10 * time.Second
		}
		httpClient = &http.Client{Timeout: timeout}
	}
	return &GitHubClient{
		baseURL:    strings.TrimRight(baseURL, "/"),
		token:      strings.TrimSpace(token),
		httpClient: httpClient,
	}
}

func (c *GitHubClient) SearchRepositories(ctx context.Context, opts SearchOptions) ([]Result, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	if c == nil || c.httpClient == nil {
		return nil, fmt.Errorf("github search is not configured")
	}
	query := strings.TrimSpace(opts.Query)
	if query == "" {
		return nil, fmt.Errorf("query is required")
	}

	reqURL, err := url.Parse(c.baseURL + "/search/repositories")
	if err != nil {
		return nil, fmt.Errorf("build github request url: %w", err)
	}
	params := reqURL.Query()
	params.Set("q", withGitHubScope(query, opts.Site))
	params.Set("per_page", fmt.Sprintf("%d", normalizedLimit(opts.Limit)))
	reqURL.RawQuery = params.Encode()

	httpReq, err := http.NewRequestWithContext(ctx, http.MethodGet, reqURL.String(), nil)
	if err != nil {
		return nil, fmt.Errorf("create github request: %w", err)
	}
	httpReq.Header.Set("Accept", "application/vnd.github+json")
	if c.token != "" {
		httpReq.Header.Set("Authorization", "Bearer "+c.token)
	}

	resp, err := c.httpClient.Do(httpReq)
	if err != nil {
		return nil, fmt.Errorf("execute github request: %w", err)
	}
	defer resp.Body.Close()

	body, err := io.ReadAll(io.LimitReader(resp.Body, 2<<20))
	if err != nil {
		return nil, fmt.Errorf("read github response: %w", err)
	}
	if resp.StatusCode < http.StatusOK || resp.StatusCode >= http.StatusMultipleChoices {
		return nil, &APIError{
			StatusCode: resp.StatusCode,
			Message:    fmt.Sprintf("github search returned status %d: %s", resp.StatusCode, strings.TrimSpace(string(body))),
		}
	}

	var payload struct {
		Items []struct {
			FullName    string `json:"full_name"`
			HTMLURL     string `json:"html_url"`
			Description string `json:"description"`
		} `json:"items"`
	}
	if err := json.Unmarshal(body, &payload); err != nil {
		return nil, fmt.Errorf("decode github payload: %w", err)
	}

	results := make([]Result, 0, len(payload.Items))
	for _, item := range payload.Items {
		if strings.TrimSpace(item.HTMLURL) == "" {
			continue
		}
		title := strings.TrimSpace(item.FullName)
		if title == "" {
			title = item.HTMLURL
		}
		results = append(results, Result{
			Title:   title,
			URL:     strings.TrimSpace(item.HTMLURL),
			Snippet: strings.TrimSpace(item.Description),
			Source:  "github",
			Type:    "github",
		})
	}

	return results, nil
}

func withGitHubScope(query, scope string) string {
	scope = normalizeGitHubScope(scope)
	if scope == "" {
		return query
	}
	if strings.Contains(scope, "/") {
		return query + " repo:" + scope
	}
	return query + " org:" + scope
}

func normalizeGitHubScope(scope string) string {
	trimmed := strings.TrimSpace(scope)
	trimmed = strings.TrimPrefix(trimmed, "https://github.com/")
	trimmed = strings.TrimPrefix(trimmed, "http://github.com/")
	trimmed = strings.TrimPrefix(trimmed, "github.com/")
	return strings.Trim(trimmed, "/")
}

// Enabled returns true when the GitHub client has been configured.
func (c *GitHubClient) Enabled() bool {
	return c != nil && c.httpClient != nil
}

// Name satisfies SearchClient.
func (c *GitHubClient) Name() string { return "github" }

// Search satisfies SearchClient. For GitHub, all search types are mapped to
// repository search.
func (c *GitHubClient) Search(ctx context.Context, _ SearchType, opts SearchOptions) ([]Result, error) {
	return c.SearchRepositories(ctx, opts)
}
