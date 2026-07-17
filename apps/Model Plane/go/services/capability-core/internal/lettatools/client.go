// Package lettatools provides an optional, non-authoritative client for
// Letta's POST /v1/tools/search endpoint. The endpoint searches tool
// definitions; it is not a memory or passage search API.
package lettatools

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net"
	"net/http"
	"net/url"
	"strconv"
	"strings"
	"time"
)

const (
	defaultTimeout          = 2 * time.Second
	maximumTimeout          = 10 * time.Second
	defaultMaxResponseBytes = 256 * 1024
	maximumResponseBytes    = 1024 * 1024
	defaultLimit            = 50
	maximumLimit            = 100
	maximumQueryBytes       = 4096
)

// Config is deployment-owned configuration for Letta tool-definition search.
// APIKey must never be logged or included in error text.
type Config struct {
	Enabled               bool
	Endpoint              string
	APIKey                string
	SearchMode            string
	Timeout               time.Duration
	MaxResponseBytes      int64
	MaxLimit              int
	AllowInsecureLoopback bool
}

// String redacts deployment credentials from diagnostic formatting.
func (c Config) String() string {
	return fmt.Sprintf(
		"lettatools.Config{enabled:%t, endpoint:%q, apiKey:[REDACTED], mode:%q}",
		c.Enabled,
		c.Endpoint,
		c.SearchMode,
	)
}

// GoString redacts deployment credentials for %#v formatting.
func (c Config) GoString() string { return c.String() }

// Match is the sole external field used by capability-core. Remote tool IDs,
// schemas, approval flags, source, and metadata are deliberately discarded.
type Match struct {
	Name string
}

// Client calls a fixed, validated Letta endpoint with strict transport and
// response bounds.
type Client struct {
	endpoint         *url.URL
	apiKey           string
	searchMode       string
	maxResponseBytes int64
	maxLimit         int
	http             *http.Client
}

// String redacts the bearer credential.
func (c *Client) String() string {
	if c == nil {
		return "lettatools.Client<nil>"
	}
	return fmt.Sprintf("lettatools.Client{endpoint:%q, apiKey:[REDACTED]}", c.endpoint.String())
}

// GoString redacts the bearer credential for %#v formatting.
func (c *Client) GoString() string { return c.String() }

// ConfigFromLookup parses strict environment-style configuration without
// reading global process state, which keeps configuration tests deterministic.
func ConfigFromLookup(lookup func(string) string) (Config, error) {
	if lookup == nil {
		return Config{}, errors.New("Letta tool search configuration lookup is required")
	}
	enabled, err := parseOptionalBool("LETTA_TOOL_SEARCH_ENABLED", lookup("LETTA_TOOL_SEARCH_ENABLED"), false)
	if err != nil {
		return Config{}, err
	}
	if !enabled {
		return Config{}, nil
	}
	allowLoopback, err := parseOptionalBool(
		"LETTA_TOOL_SEARCH_ALLOW_INSECURE_LOOPBACK",
		lookup("LETTA_TOOL_SEARCH_ALLOW_INSECURE_LOOPBACK"),
		false,
	)
	if err != nil {
		return Config{}, err
	}
	timeout, err := parseDuration("LETTA_TOOL_SEARCH_TIMEOUT", lookup("LETTA_TOOL_SEARCH_TIMEOUT"), defaultTimeout)
	if err != nil {
		return Config{}, err
	}
	limit, err := parseBoundedInt("LETTA_TOOL_SEARCH_LIMIT", lookup("LETTA_TOOL_SEARCH_LIMIT"), defaultLimit, 1, maximumLimit)
	if err != nil {
		return Config{}, err
	}
	maxBytes, err := parseBoundedInt(
		"LETTA_TOOL_SEARCH_MAX_RESPONSE_BYTES",
		lookup("LETTA_TOOL_SEARCH_MAX_RESPONSE_BYTES"),
		defaultMaxResponseBytes,
		1,
		maximumResponseBytes,
	)
	if err != nil {
		return Config{}, err
	}
	config := Config{
		Enabled:               true,
		Endpoint:              strings.TrimSpace(lookup("LETTA_TOOL_SEARCH_URL")),
		APIKey:                strings.TrimSpace(lookup("LETTA_API_KEY")),
		SearchMode:            strings.TrimSpace(lookup("LETTA_TOOL_SEARCH_MODE")),
		Timeout:               timeout,
		MaxResponseBytes:      int64(maxBytes),
		MaxLimit:              limit,
		AllowInsecureLoopback: allowLoopback,
	}
	if config.Endpoint == "" {
		return Config{}, errors.New("LETTA_TOOL_SEARCH_URL is required when Letta tool search is enabled")
	}
	if config.APIKey == "" {
		return Config{}, errors.New("LETTA_API_KEY is required when Letta tool search is enabled")
	}
	if config.SearchMode == "" {
		config.SearchMode = "hybrid"
	}
	if _, err := normalizedConfig(config); err != nil {
		return Config{}, err
	}
	return config, nil
}

// New constructs a validated client. Redirects are always disabled.
func New(config Config) (*Client, error) {
	config, err := normalizedConfig(config)
	if err != nil {
		return nil, err
	}
	endpoint, _ := url.Parse(config.Endpoint)
	return &Client{
		endpoint:         endpoint,
		apiKey:           config.APIKey,
		searchMode:       config.SearchMode,
		maxResponseBytes: config.MaxResponseBytes,
		maxLimit:         config.MaxLimit,
		http: &http.Client{
			Timeout: config.Timeout,
			CheckRedirect: func(_ *http.Request, _ []*http.Request) error {
				return http.ErrUseLastResponse
			},
		},
	}, nil
}

func normalizedConfig(config Config) (Config, error) {
	if !config.Enabled {
		return Config{}, errors.New("Letta tool search client cannot be built while disabled")
	}
	if config.Timeout == 0 {
		config.Timeout = defaultTimeout
	}
	if config.MaxResponseBytes == 0 {
		config.MaxResponseBytes = defaultMaxResponseBytes
	}
	if config.MaxLimit == 0 {
		config.MaxLimit = defaultLimit
	}
	if config.SearchMode == "" {
		config.SearchMode = "hybrid"
	}
	if config.Timeout < 0 || config.Timeout > maximumTimeout {
		return Config{}, errors.New("LETTA_TOOL_SEARCH_TIMEOUT must be positive and at most 10s")
	}
	if config.MaxResponseBytes < 1 || config.MaxResponseBytes > maximumResponseBytes {
		return Config{}, errors.New("LETTA_TOOL_SEARCH_MAX_RESPONSE_BYTES must be between 1 and 1048576")
	}
	if config.MaxLimit < 1 || config.MaxLimit > maximumLimit {
		return Config{}, errors.New("LETTA_TOOL_SEARCH_LIMIT must be between 1 and 100")
	}
	switch config.SearchMode {
	case "vector", "fts", "hybrid":
	default:
		return Config{}, errors.New("LETTA_TOOL_SEARCH_MODE must be vector, fts, or hybrid")
	}
	if config.Endpoint == "" {
		return Config{}, errors.New("LETTA_TOOL_SEARCH_URL is required")
	}
	endpoint, err := url.Parse(config.Endpoint)
	if err != nil || endpoint.Host == "" {
		return Config{}, errors.New("LETTA_TOOL_SEARCH_URL must be an absolute URL")
	}
	if endpoint.User != nil {
		return Config{}, errors.New("LETTA_TOOL_SEARCH_URL must not contain user information")
	}
	if endpoint.RawQuery != "" {
		return Config{}, errors.New("LETTA_TOOL_SEARCH_URL must not contain a query")
	}
	if endpoint.Fragment != "" {
		return Config{}, errors.New("LETTA_TOOL_SEARCH_URL must not contain a fragment")
	}
	if endpoint.Path != "/v1/tools/search" || endpoint.RawPath != "" {
		return Config{}, errors.New("LETTA_TOOL_SEARCH_URL path must be /v1/tools/search")
	}
	switch endpoint.Scheme {
	case "https":
	case "http":
		if !config.AllowInsecureLoopback || !isLoopbackHost(endpoint.Hostname()) {
			return Config{}, errors.New("Letta tool search requires HTTPS; loopback HTTP requires LETTA_TOOL_SEARCH_ALLOW_INSECURE_LOOPBACK=true")
		}
	default:
		return Config{}, errors.New("LETTA_TOOL_SEARCH_URL must use HTTPS")
	}
	if config.APIKey == "" || len(config.APIKey) > 8192 || strings.ContainsAny(config.APIKey, "\r\n") {
		return Config{}, errors.New("LETTA_API_KEY is missing or malformed")
	}
	return config, nil
}

func isLoopbackHost(host string) bool {
	if strings.EqualFold(host, "localhost") {
		return true
	}
	ip := net.ParseIP(host)
	return ip != nil && ip.IsLoopback()
}

func parseOptionalBool(name, raw string, defaultValue bool) (bool, error) {
	switch strings.TrimSpace(raw) {
	case "":
		return defaultValue, nil
	case "true":
		return true, nil
	case "false":
		return false, nil
	default:
		return false, fmt.Errorf("%s must be true or false", name)
	}
}

func parseDuration(name, raw string, defaultValue time.Duration) (time.Duration, error) {
	if strings.TrimSpace(raw) == "" {
		return defaultValue, nil
	}
	value, err := time.ParseDuration(raw)
	if err != nil || value <= 0 || value > maximumTimeout {
		return 0, fmt.Errorf("%s must be positive and at most 10s", name)
	}
	return value, nil
}

func parseBoundedInt(name, raw string, defaultValue, minimum, maximum int) (int, error) {
	if strings.TrimSpace(raw) == "" {
		return defaultValue, nil
	}
	value, err := strconv.Atoi(raw)
	if err != nil || value < minimum || value > maximum {
		return 0, fmt.Errorf("%s must be between %d and %d", name, minimum, maximum)
	}
	return value, nil
}

type searchRequest struct {
	Query      string `json:"query"`
	Limit      int    `json:"limit"`
	SearchMode string `json:"search_mode"`
}

type searchResult struct {
	Tool struct {
		Name string `json:"name"`
	} `json:"tool"`
}

// Search calls Letta's tool-definition search and returns only unique names in
// Letta rank order. Callers must intersect these names with local authority.
func (c *Client) Search(ctx context.Context, query string, requestedLimit int) ([]Match, error) {
	if c == nil {
		return nil, errors.New("Letta tool search client is unavailable")
	}
	query = strings.TrimSpace(query)
	if len(query) > maximumQueryBytes {
		return nil, errors.New("Letta tool search query exceeds the safe bound")
	}
	limit := requestedLimit
	if limit < 1 || limit > c.maxLimit {
		limit = c.maxLimit
	}
	body, err := json.Marshal(searchRequest{Query: query, Limit: limit, SearchMode: c.searchMode})
	if err != nil {
		return nil, errors.New("encode Letta tool search request")
	}
	request, err := http.NewRequestWithContext(ctx, http.MethodPost, c.endpoint.String(), bytes.NewReader(body))
	if err != nil {
		return nil, errors.New("build Letta tool search request")
	}
	request.Header.Set("Accept", "application/json")
	request.Header.Set("Content-Type", "application/json")
	request.Header.Set("Authorization", "Bearer "+c.apiKey)

	response, err := c.http.Do(request)
	if err != nil {
		return nil, errors.New("Letta tool search request failed")
	}
	defer response.Body.Close()
	if response.StatusCode != http.StatusOK {
		_, _ = io.Copy(io.Discard, io.LimitReader(response.Body, c.maxResponseBytes))
		return nil, fmt.Errorf("Letta tool search returned status %d", response.StatusCode)
	}
	encoded, err := io.ReadAll(io.LimitReader(response.Body, c.maxResponseBytes+1))
	if err != nil {
		return nil, errors.New("read Letta tool search response")
	}
	if int64(len(encoded)) > c.maxResponseBytes {
		return nil, errors.New("Letta tool search response exceeds the safe bound")
	}
	var results []searchResult
	if err := json.Unmarshal(encoded, &results); err != nil {
		return nil, errors.New("decode Letta tool search response")
	}
	if len(results) > c.maxLimit {
		results = results[:c.maxLimit]
	}
	matches := make([]Match, 0, len(results))
	seen := make(map[string]struct{}, len(results))
	for _, result := range results {
		name := strings.TrimSpace(result.Tool.Name)
		if name == "" || len(name) > 256 {
			continue
		}
		if _, exists := seen[name]; exists {
			continue
		}
		seen[name] = struct{}{}
		matches = append(matches, Match{Name: name})
	}
	return matches, nil
}
