// Package quarry is the Model Plane v1 client for the Ingestion Plane's
// Quarry-v2 edge (/v1/scrape). It is the canonical replacement for the
// Python adapter that lived in apps/Model Plane v2/agent-core (now dead).
//
// Why a shared package: model-gateway uses it for the public Fetch and
// ExtractStructured RPCs, and any future service that needs full-fat
// web fetch (JS rendering, TLS-fingerprint, charset detection, soft-404,
// JSON-LD, conditional GET) gets the same surface without re-implementing.
//
// What it does *not* do: orchestrate the LLM coercion. That stays in
// model-gateway → inference-core via the existing structured_output_schema
// path on InferRequest. This package is fetch-only.
package quarry

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"strings"
	"time"
)

// ErrUnavailable means the caller didn't configure an edge URL.
// Treat as a soft fault — the service should still respond, just without
// the full-fat fetch path.
var ErrUnavailable = errors.New("quarry: edge URL not configured")

// Error is a typed Quarry error (HTTP 4xx / 5xx with a structured envelope).
// Returned when the edge responded with a non-2xx envelope-shaped body.
type Error struct {
	Code       string
	Message    string
	StatusCode int
}

func (e *Error) Error() string {
	return fmt.Sprintf("quarry: %s (HTTP %d): %s", e.Code, e.StatusCode, e.Message)
}

// RenderHints map onto Quarry's ScrapeRequest.render field. Browser-driver
// only — static / TLS-profile fetches ignore them.
type RenderHints struct {
	WaitForSelector  string
	WaitForTimeoutMS int
}

// ScrapeRequest is a small projection of Quarry's wire shape. Fields we
// don't plumb yet (signals, cache policy, ingest) are intentionally
// omitted to keep the API tight; add them when a caller needs them.
type ScrapeRequest struct {
	URL          string
	OrgID        string // optional, forwarded as X-Quarry-Org (advisory only — edge derives org from JWT)
	Render       *RenderHints
	PreferHTTP3  bool
}

// ScrapeResult is the projected response. The full Quarry envelope is
// kept under Raw for callers that need more (branding, JSON-LD, links).
type ScrapeResult struct {
	URL         string
	FinalURL    string
	Status      int
	ContentType string
	Title       string
	Markdown    string
	Text        string
	Fingerprint string
	Language    string
	Raw         map[string]any
}

// Client talks to a Quarry edge.
//
// A zero Client is invalid — use New. Clients are safe for concurrent use;
// internal http.Client handles connection pooling.
type Client struct {
	baseURL string
	token   string
	http    *http.Client
}

// Config holds construction parameters. Empty BaseURL → New returns a
// client that always errors with ErrUnavailable, which lets services boot
// in dev without a Quarry running.
type Config struct {
	BaseURL string
	Token   string
	Timeout time.Duration
}

// New builds a Client. Trim trailing slashes from BaseURL so callers can
// pass either form. Default timeout is 30s if Config.Timeout is zero —
// Quarry may render JS so we don't want to be too aggressive.
func New(cfg Config) *Client {
	timeout := cfg.Timeout
	if timeout <= 0 {
		timeout = 30 * time.Second
	}
	return &Client{
		baseURL: strings.TrimRight(cfg.BaseURL, "/"),
		token:   cfg.Token,
		http:    &http.Client{Timeout: timeout},
	}
}

// Available reports whether the client was wired with a base URL.
// Services should branch on this to decide between full-fat Quarry fetch
// and a degraded fallback (stub response or plain net/http GET).
func (c *Client) Available() bool {
	return c != nil && c.baseURL != ""
}

// Scrape calls Quarry's /v1/scrape and returns the projected result.
//
// Errors:
//   - ErrUnavailable when the client was constructed with empty BaseURL.
//   - *Error for typed Quarry envelope errors (HTTP 4xx / 5xx with a
//     {"error":{"code","message"}} body).
//   - net.OpError / context.DeadlineExceeded for transport-level failures.
func (c *Client) Scrape(ctx context.Context, req ScrapeRequest) (*ScrapeResult, error) {
	if !c.Available() {
		return nil, ErrUnavailable
	}
	if req.URL == "" {
		return nil, fmt.Errorf("quarry: ScrapeRequest.URL required")
	}

	body := map[string]any{"url": req.URL}
	if req.Render != nil && req.Render.WaitForSelector != "" {
		render := map[string]any{"waitForSelector": req.Render.WaitForSelector}
		if req.Render.WaitForTimeoutMS > 0 {
			render["waitForTimeoutMs"] = req.Render.WaitForTimeoutMS
		}
		body["render"] = render
	}
	if req.PreferHTTP3 {
		body["prefer_http3"] = true
	}

	buf, err := json.Marshal(body)
	if err != nil {
		// Practically unreachable for the shapes above, but explicit.
		return nil, fmt.Errorf("quarry: marshal request: %w", err)
	}

	endpoint := c.baseURL + "/v1/scrape"
	httpReq, err := http.NewRequestWithContext(ctx, http.MethodPost, endpoint, bytes.NewReader(buf))
	if err != nil {
		return nil, fmt.Errorf("quarry: build request: %w", err)
	}
	httpReq.Header.Set("Content-Type", "application/json")
	httpReq.Header.Set("Accept", "application/json")
	if c.token != "" {
		httpReq.Header.Set("Authorization", "Bearer "+c.token)
	}
	if req.OrgID != "" {
		httpReq.Header.Set("X-Quarry-Org", req.OrgID)
	}

	resp, err := c.http.Do(httpReq)
	if err != nil {
		return nil, fmt.Errorf("quarry: request: %w", err)
	}
	defer resp.Body.Close()

	rawBody, err := io.ReadAll(resp.Body)
	if err != nil {
		return nil, fmt.Errorf("quarry: read response: %w", err)
	}

	if resp.StatusCode >= 400 {
		// Quarry's envelope on error is `{ok:false, error:{code,message}}`.
		// Tolerate a non-JSON body for unexpected error sources (proxies,
		// CDNs in front of the edge) — fall back to a synthesized code.
		var envErr struct {
			Error struct {
				Code    string `json:"code"`
				Message string `json:"message"`
			} `json:"error"`
		}
		_ = json.Unmarshal(rawBody, &envErr)
		code := envErr.Error.Code
		if code == "" {
			code = fmt.Sprintf("HTTP_%d", resp.StatusCode)
		}
		message := envErr.Error.Message
		if message == "" {
			message = truncate(string(rawBody), 200)
		}
		return nil, &Error{Code: code, Message: message, StatusCode: resp.StatusCode}
	}

	var env struct {
		Data map[string]any `json:"data"`
	}
	if err := json.Unmarshal(rawBody, &env); err != nil {
		return nil, fmt.Errorf("quarry: decode envelope: %w", err)
	}
	if env.Data == nil {
		// 2xx with no `data` is undefined per the contract; surface
		// loudly rather than silently returning a zero result.
		return nil, fmt.Errorf("quarry: 2xx response missing 'data'")
	}
	return project(req.URL, env.Data), nil
}

func project(requested string, data map[string]any) *ScrapeResult {
	out := &ScrapeResult{
		URL: requested,
		Raw: data,
	}

	// status
	if v, ok := data["status"]; ok {
		out.Status = toInt(v)
	}

	// content_type
	out.ContentType = toString(data["content_type"])

	// fingerprint
	out.Fingerprint = toString(data["fingerprint"])

	// url.final
	if urlObj, ok := data["url"].(map[string]any); ok {
		out.FinalURL = toString(urlObj["final"])
	}
	if out.FinalURL == "" {
		out.FinalURL = requested
	}

	// formats.markdown / formats.text
	if formats, ok := data["formats"].(map[string]any); ok {
		out.Markdown = toString(formats["markdown"])
		out.Text = toString(formats["text"])
		if out.Text == "" {
			// Many pages only have markdown — fall back so callers
			// always get *something* in .Text.
			out.Text = out.Markdown
		}
	}

	// metadata.title / metadata.lang
	if md, ok := data["metadata"].(map[string]any); ok {
		out.Title = toString(md["title"])
		out.Language = toString(md["lang"])
	}

	return out
}

func toString(v any) string {
	if v == nil {
		return ""
	}
	if s, ok := v.(string); ok {
		return s
	}
	return ""
}

func toInt(v any) int {
	switch n := v.(type) {
	case float64:
		return int(n)
	case int:
		return n
	case json.Number:
		i, _ := n.Int64()
		return int(i)
	}
	return 0
}

func truncate(s string, n int) string {
	if len(s) <= n {
		return s
	}
	return s[:n]
}
