package api

import (
	"context"
	"fmt"
	"net/http"
	"net/url"
	"strings"
	"time"

	"github.com/gofiber/fiber/v2"
	"github.com/rs/zerolog/log"

	"github.com/triodelab/quarry/internal/scraper"
)

const (
	// llmsTxtMaxPages is the maximum number of sitemap/link URLs the handler
	// summarises. Increased from 50 to 100 to match Firecrawl's depth.
	llmsTxtMaxPages = 100

	// llmsTxtSummaryLen is the maximum character length kept per page summary
	// when building the standard full LLMs.txt variant.
	llmsTxtSummaryLen = 380

	// llmsTxtCtxMaxContentLen caps per-page content in /ctx mode to avoid
	// ballooning the output beyond LLM context windows.
	llmsTxtCtxMaxContentLen = 8000
)

// llmsTxtRequest is the request body for both LLMs.txt endpoints.
type llmsTxtRequest struct {
	URL string `json:"url" query:"url"`
}

// v1LLMsTxt handles GET /v1/llmstxt — returns a concise LLMs.txt document for
// the given website (title + top-level page list with one-line descriptions).
func (h *Handler) v1LLMsTxt(c *fiber.Ctx) error {
	req, err := parseLLMsTxtRequest(c)
	if err != nil {
		return writeError(c, http.StatusBadRequest, err.Error(), nil)
	}
	doc, genErr := h.generateLLMsTxt(c.UserContext(), req.URL, false)
	if genErr != nil {
		return writeError(c, http.StatusBadGateway, "failed to generate llms.txt", genErr.Error())
	}
	c.Set("Content-Type", "text/plain; charset=utf-8")
	return c.SendString(doc)
}

// v1LLMsTxtFull handles GET /v1/llmstxt/full — same as v1LLMsTxt but includes
// extracted page content (up to llmsTxtSummaryLen chars per page).
func (h *Handler) v1LLMsTxtFull(c *fiber.Ctx) error {
	req, err := parseLLMsTxtRequest(c)
	if err != nil {
		return writeError(c, http.StatusBadRequest, err.Error(), nil)
	}
	doc, genErr := h.generateLLMsTxt(c.UserContext(), req.URL, true)
	if genErr != nil {
		return writeError(c, http.StatusBadGateway, "failed to generate llms.txt/full", genErr.Error())
	}
	c.Set("Content-Type", "text/plain; charset=utf-8")
	return c.SendString(doc)
}

// v1LLMsTxtCtx handles GET /v1/llmstxt/ctx — generates an expanded context
// document following the llmstxt.org llms-ctx.txt convention. Similar to /full
// but with expanded content (up to llmsTxtCtxMaxContentLen per page) and an
// "Optional" section for lower-priority pages.
func (h *Handler) v1LLMsTxtCtx(c *fiber.Ctx) error {
	req, err := parseLLMsTxtRequest(c)
	if err != nil {
		return writeError(c, http.StatusBadRequest, err.Error(), nil)
	}
	doc, genErr := h.generateLLMsTxtCtx(c.UserContext(), req.URL, false)
	if genErr != nil {
		return writeError(c, http.StatusBadGateway, "failed to generate llms-ctx.txt", genErr.Error())
	}
	c.Set("Content-Type", "text/plain; charset=utf-8")
	return c.SendString(doc)
}

// v1LLMsTxtCtxFull handles GET /v1/llmstxt/ctx/full — generates a full-content
// context document (llms-ctx-full.txt) with no per-page content truncation.
func (h *Handler) v1LLMsTxtCtxFull(c *fiber.Ctx) error {
	req, err := parseLLMsTxtRequest(c)
	if err != nil {
		return writeError(c, http.StatusBadRequest, err.Error(), nil)
	}
	doc, genErr := h.generateLLMsTxtCtx(c.UserContext(), req.URL, true)
	if genErr != nil {
		return writeError(c, http.StatusBadGateway, "failed to generate llms-ctx-full.txt", genErr.Error())
	}
	c.Set("Content-Type", "text/plain; charset=utf-8")
	return c.SendString(doc)
}

// parseLLMsTxtRequest parses the URL param from query string or JSON body.
func parseLLMsTxtRequest(c *fiber.Ctx) (*llmsTxtRequest, error) {
	var req llmsTxtRequest
	// Accept URL via query string (?url=...) or JSON body.
	if raw := strings.TrimSpace(c.Query("url")); raw != "" {
		req.URL = raw
	} else if err := c.BodyParser(&req); err == nil && strings.TrimSpace(req.URL) != "" {
		// Body parsed OK.
	}
	req.URL = strings.TrimSpace(req.URL)
	if req.URL == "" {
		return nil, fmt.Errorf("url query parameter is required")
	}
	if err := validateAbsoluteHTTPURL(req.URL); err != nil {
		return nil, err
	}
	return &req, nil
}

// generateLLMsTxt crawls the target URL, collects page links + metadata, and
// renders an LLMs.txt document conforming to the llmstxt.org specification.
//
// When full is true, it also scrapes each discovered URL for its main content
// and appends a summary block.
func (h *Handler) generateLLMsTxt(ctx context.Context, targetURL string, full bool) (string, error) {
	if h.scraper == nil {
		return "", fmt.Errorf("scraper is not initialized")
	}

	parsed, err := url.ParseRequestURI(targetURL)
	if err != nil {
		return "", fmt.Errorf("invalid url: %w", err)
	}

	// 1. Fetch the root page to get the site title and description.
	rootCtx, rootCancel := context.WithTimeout(ctx, 20*time.Second)
	defer rootCancel()
	rootOutputs, _, rootErr := h.scraper.FetchFormats(rootCtx, targetURL, &scraper.FormatOptions{
		Formats: []string{"markdown", "metadata"},
	})
	rootTitle := parsed.Hostname()
	rootDesc := ""
	if rootErr == nil {
		if meta, ok := rootOutputs["metadata"].(map[string]interface{}); ok {
			if t, ok := meta["title"].(string); ok && strings.TrimSpace(t) != "" {
				rootTitle = strings.TrimSpace(t)
			}
			if d, ok := meta["description"].(string); ok {
				rootDesc = strings.TrimSpace(d)
			}
		}
	}

	// 2. Collect page URLs from sitemap + crawled links.
	pageURLs := fetchSitemapURLs(ctx, parsed)
	if len(pageURLs) == 0 {
		linkCtx, linkCancel := context.WithTimeout(ctx, 20*time.Second)
		defer linkCancel()
		if linkOutputs, _, le := h.scraper.FetchFormats(linkCtx, targetURL, &scraper.FormatOptions{Formats: []string{"links"}}); le == nil {
			if links, ok := linkOutputs["links"].([]string); ok {
				pageURLs = links
			}
		}
	}

	// Deduplicate and filter to same domain, cap at llmsTxtMaxPages.
	seen := map[string]struct{}{targetURL: {}}
	filtered := make([]string, 0, llmsTxtMaxPages)
	for _, u := range pageURLs {
		u = strings.TrimSpace(u)
		if _, ok := seen[u]; ok {
			continue
		}
		pu, perr := url.Parse(u)
		if perr != nil || !sameDomain(parsed.Hostname(), pu.Hostname(), false) {
			continue
		}
		seen[u] = struct{}{}
		filtered = append(filtered, u)
		if len(filtered) >= llmsTxtMaxPages {
			break
		}
	}

	// 3. Optionally scrape each page for summary content.
	type pageEntry struct {
		URL     string
		Title   string
		Desc    string
		Content string
	}

	entries := make([]pageEntry, 0, len(filtered))
	for _, u := range filtered {
		entry := pageEntry{URL: u, Title: u}
		if full {
			pCtx, pCancel := context.WithTimeout(ctx, 15*time.Second)
			outputs, _, pErr := h.scraper.FetchFormats(pCtx, u, &scraper.FormatOptions{
				Formats:         []string{"markdown", "metadata"},
				OnlyMainContent: true,
			})
			pCancel()
			if pErr == nil {
				if meta, ok := outputs["metadata"].(map[string]interface{}); ok {
					if t, ok := meta["title"].(string); ok && strings.TrimSpace(t) != "" {
						entry.Title = strings.TrimSpace(t)
					}
					if d, ok := meta["description"].(string); ok {
						entry.Desc = strings.TrimSpace(d)
					}
				}
				if md, ok := outputs["markdown"].(string); ok {
					content := strings.TrimSpace(md)
					if len(content) > llmsTxtSummaryLen {
						content = content[:llmsTxtSummaryLen] + "…"
					}
					entry.Content = content
				}
			}
		}
		entries = append(entries, entry)
	}

	// 4. Render the LLMs.txt document.
	var b strings.Builder

	// Header block.
	fmt.Fprintf(&b, "# %s\n\n", rootTitle)
	if rootDesc != "" {
		fmt.Fprintf(&b, "> %s\n\n", rootDesc)
	}

	// Pages section.
	if len(entries) > 0 {
		b.WriteString("## Pages\n\n")
		for _, e := range entries {
			if full && e.Content != "" {
				fmt.Fprintf(&b, "### [%s](%s)\n\n", e.Title, e.URL)
				if e.Desc != "" {
					fmt.Fprintf(&b, "> %s\n\n", e.Desc)
				}
				fmt.Fprintf(&b, "%s\n\n", e.Content)
			} else {
				if e.Desc != "" {
					fmt.Fprintf(&b, "- [%s](%s): %s\n", e.Title, e.URL, e.Desc)
				} else {
					fmt.Fprintf(&b, "- [%s](%s)\n", e.Title, e.URL)
				}
			}
		}
		b.WriteString("\n")
	}

	if !full {
		fmt.Fprintf(&b, "---\nGenerated by Quarry at %s\n", time.Now().UTC().Format(time.RFC3339))
	}

	log.Info().
		Str("url", targetURL).
		Int("pages", len(entries)).
		Bool("full", full).
		Msg("llmstxt generated")

	return strings.TrimRight(b.String(), "\n") + "\n", nil
}

// generateLLMsTxtCtx generates an expanded context document following the
// llmstxt.org llms-ctx.txt / llms-ctx-full.txt convention.
//
//   - When fullContent is false (ctx): each page's content is truncated at
//     llmsTxtCtxMaxContentLen chars.
//   - When fullContent is true (ctx/full): no truncation — full markdown
//     content per page is included.
//
// The document uses the llmstxt.org structure: H1 title → blockquote →
// "## Pages" section with expanded content → "## Optional" section for
// lower-priority pages.
func (h *Handler) generateLLMsTxtCtx(ctx context.Context, targetURL string, fullContent bool) (string, error) {
	if h.scraper == nil {
		return "", fmt.Errorf("scraper is not initialized")
	}

	parsed, err := url.ParseRequestURI(targetURL)
	if err != nil {
		return "", fmt.Errorf("invalid url: %w", err)
	}

	// 1. Fetch root page for site title and description.
	rootCtx, rootCancel := context.WithTimeout(ctx, 20*time.Second)
	defer rootCancel()
	rootOutputs, _, rootErr := h.scraper.FetchFormats(rootCtx, targetURL, &scraper.FormatOptions{
		Formats: []string{"markdown", "metadata"},
	})
	rootTitle := parsed.Hostname()
	rootDesc := ""
	if rootErr == nil {
		if meta, ok := rootOutputs["metadata"].(map[string]interface{}); ok {
			if t, ok := meta["title"].(string); ok && strings.TrimSpace(t) != "" {
				rootTitle = strings.TrimSpace(t)
			}
			if d, ok := meta["description"].(string); ok {
				rootDesc = strings.TrimSpace(d)
			}
		}
	}

	// 2. Collect page URLs.
	pageURLs := fetchSitemapURLs(ctx, parsed)
	if len(pageURLs) == 0 {
		linkCtx, linkCancel := context.WithTimeout(ctx, 20*time.Second)
		defer linkCancel()
		if linkOutputs, _, le := h.scraper.FetchFormats(linkCtx, targetURL, &scraper.FormatOptions{Formats: []string{"links"}}); le == nil {
			if links, ok := linkOutputs["links"].([]string); ok {
				pageURLs = links
			}
		}
	}

	// Deduplicate, same domain, cap at llmsTxtMaxPages.
	seen := map[string]struct{}{targetURL: {}}
	filtered := make([]string, 0, llmsTxtMaxPages)
	for _, u := range pageURLs {
		u = strings.TrimSpace(u)
		if _, ok := seen[u]; ok {
			continue
		}
		pu, perr := url.Parse(u)
		if perr != nil || !sameDomain(parsed.Hostname(), pu.Hostname(), false) {
			continue
		}
		seen[u] = struct{}{}
		filtered = append(filtered, u)
		if len(filtered) >= llmsTxtMaxPages {
			break
		}
	}

	// 3. Scrape each page for content.
	type ctxPageEntry struct {
		URL     string
		Title   string
		Desc    string
		Content string
	}

	entries := make([]ctxPageEntry, 0, len(filtered))
	for _, u := range filtered {
		entry := ctxPageEntry{URL: u, Title: u}
		pCtx, pCancel := context.WithTimeout(ctx, 15*time.Second)
		outputs, _, pErr := h.scraper.FetchFormats(pCtx, u, &scraper.FormatOptions{
			Formats:         []string{"markdown", "metadata"},
			OnlyMainContent: true,
		})
		pCancel()
		if pErr == nil {
			if meta, ok := outputs["metadata"].(map[string]interface{}); ok {
				if t, ok := meta["title"].(string); ok && strings.TrimSpace(t) != "" {
					entry.Title = strings.TrimSpace(t)
				}
				if d, ok := meta["description"].(string); ok {
					entry.Desc = strings.TrimSpace(d)
				}
			}
			if md, ok := outputs["markdown"].(string); ok {
				content := strings.TrimSpace(md)
				// Only truncate in ctx mode (not ctx/full).
				if !fullContent && len(content) > llmsTxtCtxMaxContentLen {
					content = content[:llmsTxtCtxMaxContentLen] + "…"
				}
				entry.Content = content
			}
		}
		entries = append(entries, entry)
	}

	// 4. Render the llms-ctx.txt document.
	// Split entries into primary (first 70%) and optional (last 30%).
	primaryCount := len(entries)
	if primaryCount > 10 {
		primaryCount = (len(entries) * 70) / 100
		if primaryCount < 5 {
			primaryCount = 5
		}
	}
	primaryEntries := entries
	var optionalEntries []ctxPageEntry
	if len(entries) > primaryCount {
		primaryEntries = entries[:primaryCount]
		optionalEntries = entries[primaryCount:]
	}

	var b strings.Builder

	// Header.
	fmt.Fprintf(&b, "# %s\n\n", rootTitle)
	if rootDesc != "" {
		fmt.Fprintf(&b, "> %s\n\n", rootDesc)
	}

	// Primary pages section.
	if len(primaryEntries) > 0 {
		b.WriteString("## Pages\n\n")
		for _, e := range primaryEntries {
			fmt.Fprintf(&b, "### [%s](%s)\n\n", e.Title, e.URL)
			if e.Desc != "" {
				fmt.Fprintf(&b, "> %s\n\n", e.Desc)
			}
			if e.Content != "" {
				fmt.Fprintf(&b, "%s\n\n", e.Content)
			}
		}
	}

	// Optional section (lower-priority pages).
	if len(optionalEntries) > 0 {
		b.WriteString("## Optional\n\n")
		for _, e := range optionalEntries {
			fmt.Fprintf(&b, "### [%s](%s)\n\n", e.Title, e.URL)
			if e.Desc != "" {
				fmt.Fprintf(&b, "> %s\n\n", e.Desc)
			}
			if e.Content != "" {
				fmt.Fprintf(&b, "%s\n\n", e.Content)
			}
		}
	}

	mode := "ctx"
	if fullContent {
		mode = "ctx-full"
	}
	log.Info().
		Str("url", targetURL).
		Int("pages", len(entries)).
		Str("mode", mode).
		Msg("llmstxt ctx generated")

	return strings.TrimRight(b.String(), "\n") + "\n", nil
}
