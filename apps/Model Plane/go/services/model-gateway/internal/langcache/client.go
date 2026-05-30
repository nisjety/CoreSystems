// Package langcache implements a Redis LangCache semantic-cache client that
// satisfies proxy.SemanticCache. LangCache is a managed REST service that
// generates embeddings server-side, so this client only ships prompt/response
// text plus scoping attributes — there is no local vector store.
package langcache

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"strings"
	"time"
)

const (
	defaultTimeout   = 5 * time.Second
	defaultThreshold = 0.9
)

// Config configures the LangCache client. BaseURL, CacheID, and APIKey are
// required; New returns ok=false when any is empty so callers can fall back to
// running without a cache.
type Config struct {
	BaseURL   string        // e.g. https://api.langcache.redis.io
	CacheID   string        // LangCache service cache id (path parameter)
	APIKey    string        // LangCache service API key (Bearer token)
	Threshold float64       // semantic similarity threshold; <=0 uses default
	Timeout   time.Duration // per-request timeout; <=0 uses default
}

// Client talks to the LangCache REST API.
type Client struct {
	http      *http.Client
	baseURL   string
	cacheID   string
	apiKey    string
	threshold float64
}

// New builds a Client. ok is false when required config is missing.
func New(cfg Config) (client *Client, ok bool) {
	if cfg.BaseURL == "" || cfg.CacheID == "" || cfg.APIKey == "" {
		return nil, false
	}
	threshold := cfg.Threshold
	if threshold <= 0 {
		threshold = defaultThreshold
	}
	timeout := cfg.Timeout
	if timeout <= 0 {
		timeout = defaultTimeout
	}
	return &Client{
		http:      &http.Client{Timeout: timeout},
		baseURL:   strings.TrimRight(cfg.BaseURL, "/"),
		cacheID:   cfg.CacheID,
		apiKey:    cfg.APIKey,
		threshold: threshold,
	}, true
}

type searchRequest struct {
	Prompt              string            `json:"prompt"`
	SimilarityThreshold float64           `json:"similarityThreshold,omitempty"`
	Attributes          map[string]string `json:"attributes,omitempty"`
}

type storeRequest struct {
	Prompt     string            `json:"prompt"`
	Response   string            `json:"response"`
	Attributes map[string]string `json:"attributes,omitempty"`
}

type entry struct {
	Response   string  `json:"response"`
	Similarity float64 `json:"similarity"`
}

// Lookup searches the cache for a semantically similar prompt, scoped to the
// org and model so responses never leak across tenants. hit is false on a miss.
func (c *Client) Lookup(ctx context.Context, prompt, orgID, model string) (response string, hit bool, err error) {
	raw, err := c.post(ctx, "/entries/search", searchRequest{
		Prompt:              prompt,
		SimilarityThreshold: c.threshold,
		Attributes:          attrs(orgID, model),
	})
	if err != nil {
		return "", false, err
	}
	entries, err := parseEntries(raw)
	if err != nil {
		return "", false, err
	}
	if len(entries) == 0 || entries[0].Response == "" {
		return "", false, nil
	}
	return entries[0].Response, true, nil
}

// Store writes a prompt/response pair to the cache, scoped by org and model.
func (c *Client) Store(ctx context.Context, prompt, orgID, model, response string) error {
	_, err := c.post(ctx, "/entries", storeRequest{
		Prompt:     prompt,
		Response:   response,
		Attributes: attrs(orgID, model),
	})
	return err
}

func attrs(orgID, model string) map[string]string {
	a := make(map[string]string, 2)
	if orgID != "" {
		a["org_id"] = orgID
	}
	if model != "" {
		a["model"] = model
	}
	return a
}

func (c *Client) post(ctx context.Context, path string, body any) ([]byte, error) {
	buf, err := json.Marshal(body)
	if err != nil {
		return nil, fmt.Errorf("langcache marshal: %w", err)
	}
	url := fmt.Sprintf("%s/v1/caches/%s%s", c.baseURL, c.cacheID, path)
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, url, bytes.NewReader(buf))
	if err != nil {
		return nil, fmt.Errorf("langcache new request: %w", err)
	}
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("Accept", "application/json")
	req.Header.Set("Authorization", "Bearer "+c.apiKey)

	resp, err := c.http.Do(req)
	if err != nil {
		return nil, fmt.Errorf("langcache POST %s: %w", path, err)
	}
	defer func() { _ = resp.Body.Close() }()

	data, err := io.ReadAll(resp.Body)
	if err != nil {
		return nil, fmt.Errorf("langcache read body: %w", err)
	}
	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		return nil, fmt.Errorf("langcache POST %s: status %d: %s", path, resp.StatusCode, strings.TrimSpace(string(data)))
	}
	return data, nil
}

// parseEntries accepts either a bare JSON array of entries or a {"data": [...]}
// envelope, so the client tolerates either documented search-response shape.
func parseEntries(raw []byte) ([]entry, error) {
	trimmed := bytes.TrimSpace(raw)
	if len(trimmed) == 0 {
		return nil, nil
	}
	switch trimmed[0] {
	case '[':
		var arr []entry
		if err := json.Unmarshal(trimmed, &arr); err != nil {
			return nil, fmt.Errorf("langcache decode array: %w", err)
		}
		return arr, nil
	case '{':
		var env struct {
			Data []entry `json:"data"`
		}
		if err := json.Unmarshal(trimmed, &env); err != nil {
			return nil, fmt.Errorf("langcache decode object: %w", err)
		}
		return env.Data, nil
	default:
		return nil, fmt.Errorf("langcache unexpected response: %q", string(trimmed))
	}
}
