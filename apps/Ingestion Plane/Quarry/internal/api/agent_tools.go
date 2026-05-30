// Package api: agent-tools endpoints (U2-16, velion ui-ux-velion-gap.md §10).
//
// Exposes two minimal HTTP routes designed to be called by agent-core's
// `web_search` and `web_fetch` tools (which today are stubs in
// `apps/Model Plane v2/agent-core/app/tools/builtins/`):
//
//   POST /v1/agent-tools/fetch
//     Body: { "url": string, "max_chars": int? }
//     Returns: { "url", "title", "content", "fetched_at" }
//     Single-page fetch via Quarry's existing FetchFormats pipeline
//     (Rod-driven Chromium with anti-bot + robots compliance).
//
//   POST /v1/agent-tools/search
//     Body: { "query": string, "limit": int?, "country": string?,
//              "freshness": string?, "site": string? }
//     Returns: { "query", "results": [{"title","url","snippet"}] }
//     Wraps Quarry's existing BraveClient — sources the live Brave Search
//     API key from BRAVE_API_KEY. When the key is missing or Brave is
//     unreachable, returns a 503 with `service_unavailable` so the LLM
//     can either retry or apologise.
//
// Both endpoints sit behind the same gating middleware as the rest of
// /v1 (org-scoped + internal-api-key for cross-plane callers).

package api

import (
	"context"
	"encoding/json"
	"errors"
	"net/http"
	"strings"
	"time"

	"github.com/gofiber/fiber/v2"

	"github.com/triodelab/quarry/internal/scraper"
	"github.com/triodelab/quarry/internal/search"
)

// AgentFetchRequest matches the LLM-tool-call surface agent-core sends.
type AgentFetchRequest struct {
	URL      string `json:"url"`
	MaxChars int    `json:"max_chars,omitempty"`
}

// AgentFetchResponse is what agent-core (and through it, the LLM) receives.
type AgentFetchResponse struct {
	URL       string `json:"url"`
	Title     string `json:"title"`
	Content   string `json:"content"`
	FetchedAt string `json:"fetched_at"`
}

// AgentSearchRequest mirrors agent-core's `web_search` tool input.
type AgentSearchRequest struct {
	Query     string `json:"query"`
	Limit     int    `json:"limit,omitempty"`
	Country   string `json:"country,omitempty"`
	Freshness string `json:"freshness,omitempty"`
	Site      string `json:"site,omitempty"`
}

// AgentSearchResponse is the SERP-shaped payload returned to the LLM.
type AgentSearchResponse struct {
	Query   string          `json:"query"`
	Results []search.Result `json:"results"`
	Source  string          `json:"source"`
}

// registerAgentTools wires the two routes. Call from Handler.Register.
func (h *Handler) registerAgentTools(app *fiber.App) {
	app.Post("/v1/agent-tools/fetch", h.agentToolFetch)
	app.Post("/v1/agent-tools/search", h.agentToolSearch)
}

// agentToolFetch — single-URL fetch using the existing Scraper pipeline.
// We return clean text (markdown or extracted text) bounded by max_chars
// so the LLM can stuff it into its context without re-tokenizing.
func (h *Handler) agentToolFetch(c *fiber.Ctx) error {
	var req AgentFetchRequest
	if err := c.BodyParser(&req); err != nil {
		return c.Status(http.StatusBadRequest).JSON(fiber.Map{
			"error": "invalid request body",
		})
	}
	url := strings.TrimSpace(req.URL)
	if url == "" {
		return c.Status(http.StatusBadRequest).JSON(fiber.Map{
			"error": "url required",
		})
	}
	maxChars := req.MaxChars
	if maxChars <= 0 {
		maxChars = 8_000 // sensible default for a single tool-call payload
	}
	if maxChars > 50_000 {
		maxChars = 50_000
	}

	if h.scraper == nil {
		return c.Status(http.StatusServiceUnavailable).JSON(fiber.Map{
			"error": "scraper not configured",
		})
	}

	ctx, cancel := context.WithTimeout(c.UserContext(), 30*time.Second)
	defer cancel()

	formats, _, err := h.scraper.FetchFormats(ctx, url, &scraper.FormatOptions{
		// Request both markdown (the LLM friendly representation) and
		// extracted text (fallback). HTML is not useful for the tool path.
		Formats: []string{"markdown", "text"},
	})
	if err != nil {
		status := http.StatusBadGateway
		if errors.Is(err, context.DeadlineExceeded) {
			status = http.StatusGatewayTimeout
		}
		return c.Status(status).JSON(fiber.Map{
			"error":   "fetch failed",
			"detail":  err.Error(),
			"url":     url,
		})
	}

	// Prefer markdown when present (LLMs do better with markdown than raw
	// text). Fall back to extracted text. Title comes from extracted
	// metadata when the scraper surfaces it.
	content := stringFromFormats(formats, "markdown")
	if content == "" {
		content = stringFromFormats(formats, "text")
	}
	if len(content) > maxChars {
		content = content[:maxChars]
	}
	title := stringFromFormats(formats, "title")

	return c.Status(http.StatusOK).JSON(AgentFetchResponse{
		URL:       url,
		Title:     title,
		Content:   content,
		FetchedAt: time.Now().UTC().Format(time.RFC3339),
	})
}

// agentToolSearch — web search via Brave (the existing client wired into
// the Handler). Returns a flat list of {title, url, snippet} the LLM can
// reason over directly. When Brave is unavailable we surface a clear
// 503 so agent-core can tell the LLM to retry or apologise.
func (h *Handler) agentToolSearch(c *fiber.Ctx) error {
	var req AgentSearchRequest
	if err := c.BodyParser(&req); err != nil {
		return c.Status(http.StatusBadRequest).JSON(fiber.Map{
			"error": "invalid request body",
		})
	}
	query := strings.TrimSpace(req.Query)
	if query == "" {
		return c.Status(http.StatusBadRequest).JSON(fiber.Map{
			"error": "query required",
		})
	}
	limit := req.Limit
	if limit <= 0 {
		limit = 5
	}
	if limit > 20 {
		limit = 20
	}

	if h.searchClient == nil || !h.searchClient.Enabled() {
		return c.Status(http.StatusServiceUnavailable).JSON(fiber.Map{
			"error":  "search_unavailable",
			"detail": "Brave search client is not configured (set BRAVE_API_KEY)",
			"query":  query,
		})
	}

	ctx, cancel := context.WithTimeout(c.UserContext(), 15*time.Second)
	defer cancel()

	opts := search.SearchOptions{
		Query:   query,
		Limit:   limit,
		Site:    req.Site,
		Country: strings.TrimSpace(req.Country),
	}
	if f := strings.TrimSpace(req.Freshness); f != "" {
		opts.Freshness = f
	}

	results, err := h.searchClient.Search(ctx, search.SearchTypeWeb, opts)
	if err != nil {
		return c.Status(http.StatusBadGateway).JSON(fiber.Map{
			"error":  "search_failed",
			"detail": err.Error(),
			"query":  query,
		})
	}

	return c.Status(http.StatusOK).JSON(AgentSearchResponse{
		Query:   query,
		Results: results,
		Source:  "brave",
	})
}

// stringFromFormats safely extracts a string field from the scraper's
// formats map (which is `map[string]interface{}`).
func stringFromFormats(formats map[string]interface{}, key string) string {
	if formats == nil {
		return ""
	}
	v, ok := formats[key]
	if !ok || v == nil {
		return ""
	}
	switch s := v.(type) {
	case string:
		return s
	case json.RawMessage:
		return string(s)
	default:
		return ""
	}
}
