package scraper

import (
	"context"
	"fmt"
	"path"
	"strings"
	"time"

	"github.com/PuerkitoBio/goquery"

	"github.com/triodelab/quarry/internal/actions"
	"github.com/triodelab/quarry/internal/ai"
	"github.com/triodelab/quarry/internal/driver"
	"github.com/triodelab/quarry/internal/transform"
)

type FormatOptions struct {
	Formats         []string
	IncludeTags     []string
	ExcludeTags     []string
	OnlyMainContent bool
	WaitFor         int
	MaxAgeMs        int64
	Headers         map[string]string
	Actions         []actions.ActionStep
	ParserMode      string

	// Attributes format: CSS-selector based extraction
	Attributes []AttributeSelector

	// Browser emulation
	Mobile   bool
	Viewport *driver.ViewportConfig
	Location *driver.GeoLocation
	BlockAds bool

	// Proxy injection (set by middleware, piped to driver)
	ProxyURL string

	// RenderJS controls headless browser (Rod) usage.
	// nil = auto-detect (Phase 1: fetch with Colly first, escalate if SPA detected).
	// true = always use Rod. false = never use Rod.
	RenderJS *bool

	// AutoScroll scrolls the page to trigger lazy-loaded content.
	// Only effective when Rod driver is selected (via RenderJS or auto-escalation).
	AutoScroll bool

	// AgentGoal, when non-empty, enables AI-driven interaction via Model Plane v2.
	// If the page still has insufficient content after rendering, ai-core's
	// AgentNavigate drives the browser (click, type, scroll) until the goal is
	// met or agentMaxSteps is exhausted.
	// Example: "extract the main product details including price and description"
	AgentGoal string

	// AgentSchema is an optional JSON Schema string forwarded to AgentNavigate
	// so the AI knows what structured fields to target.
	AgentSchema string

	// OrgID is forwarded to ai-core for billing / rate-limiting context.
	OrgID string
}

func (s *Scraper) FetchFormats(ctx context.Context, targetURL string, opts *FormatOptions) (map[string]interface{}, []actions.ActionResult, error) {
	if opts == nil {
		opts = &FormatOptions{}
	}

	// ── Format-level cache check (Phase 5 performance optimization) ─────────
	// Only use cache for read-only requests (no actions, no screenshot).
	canCache := s.cacheManager != nil && len(opts.Actions) == 0 && !containsFormat(opts.Formats, "screenshot")
	cacheMaxAge := s.cfg.DefaultMaxAgeMs
	if opts.MaxAgeMs > 0 {
		cacheMaxAge = opts.MaxAgeMs
	}
	if canCache {
		cached, hit, _ := s.cacheManager.GetCachedFormats(ctx, targetURL, opts.Formats, cacheMaxAge)
		if hit {
			return cached, nil, nil
		}
	}

	waterfall := driver.DefaultWaterfall(s.cfg.UserAgent, s.cfg.EnableStealth, s.browserPool, s.cfg.AICoreBaseURL)

	// Resolve JS rendering requirement:
	// - opts.RenderJS == nil  → auto-detect (default: start without JS, escalate if needed)
	// - opts.RenderJS == true → force Rod (headless Chromium)
	// - opts.RenderJS == false → force Colly (HTTP-only)
	autoDetect := opts.RenderJS == nil
	needJS := false
	if opts.RenderJS != nil {
		needJS = *opts.RenderJS
	}

	driverInput := driver.SelectionInput{
		NeedJavaScript: needJS,
		NeedScreenshot: containsFormat(opts.Formats, "screenshot"),
		HasActions:     len(opts.Actions) > 0,
		WaitForMs:      opts.WaitFor,
		Formats:        opts.Formats,
		Mobile:         opts.Mobile,
		HasGeo:         opts.Location != nil,
		BlockAds:       opts.BlockAds,
		TargetURL:      targetURL,
	}
	drv, err := waterfall.Select(driverInput)
	if err != nil {
		return nil, nil, fmt.Errorf("engine selection failed: %w", err)
	}
	defer func() { _ = drv.Close() }()

	fetchResult, err := drv.Fetch(ctx, targetURL, &driver.FetchOptions{
		Headers:    opts.Headers,
		WaitFor:    opts.WaitFor,
		ProxyURL:   opts.ProxyURL,
		Mobile:     opts.Mobile,
		Viewport:   opts.Viewport,
		Location:   opts.Location,
		BlockAds:   opts.BlockAds,
		AutoScroll: opts.AutoScroll,
		// Block images/fonts/media when we only need HTML — saves 300-800 ms.
		// Skip when requesting a screenshot so the render is visually complete.
		BlockMedia: !containsFormat(opts.Formats, "screenshot"),
	})
	if err != nil {
		return nil, nil, err
	}

	// ── Driver escalation on anti-bot responses (403/429) ───────────────────
	// If the initial non-JS driver got a 403 (Forbidden) or 429 (Too Many
	// Requests), escalate to Rod with stealth mode which can bypass many
	// bot-detection systems (Cloudflare, Akamai, etc.).
	if !driverInput.NeedJavaScript && (fetchResult.Status == 403 || fetchResult.Status == 429) {
		_ = drv.Close()
		jsInput := driverInput
		jsInput.NeedJavaScript = true
		jsDrv, jsErr := waterfall.Select(jsInput)
		if jsErr == nil {
			waitFor := opts.WaitFor
			if waitFor <= 0 {
				waitFor = 3000
			}
			jsResult, jsFetchErr := jsDrv.Fetch(ctx, targetURL, &driver.FetchOptions{
				Headers:    opts.Headers,
				WaitFor:    waitFor,
				ProxyURL:   opts.ProxyURL,
				Mobile:     opts.Mobile,
				Viewport:   opts.Viewport,
				Location:   opts.Location,
				BlockAds:   opts.BlockAds,
				AutoScroll: opts.AutoScroll,
			})
			if jsFetchErr == nil && jsResult.Status != 403 && jsResult.Status != 429 {
				fetchResult = jsResult
				drv = jsDrv
			} else {
				_ = jsDrv.Close()
			}
		}
	}

	html, err := drv.HTML(ctx)
	if err != nil || strings.TrimSpace(html) == "" {
		html = fetchResult.HTML
	}

	// ── JS escalation: content-quality check ─────────────────────────────
	// Instead of framework fingerprinting, we use a render-and-validate
	// strategy (same as Firecrawl): escalate to Rod whenever the non-JS
	// result has too little usable text, regardless of which framework
	// built the page. This is framework-agnostic and future-proof.
	if autoDetect && !fetchResult.Rendered && isContentInsufficient(html) {
		_ = drv.Close()

		var jsDrv driver.PageDriver
		if s.browserPool != nil {
			jsDrv = driver.NewRodDriverFromPool(s.browserPool)
		} else {
			jsInput := driverInput
			jsInput.NeedJavaScript = true
			var jsErr error
			jsDrv, jsErr = waterfall.Select(jsInput)
			if jsErr != nil {
				jsDrv = nil
			}
		}

		if jsDrv != nil {
			jsCtx, jsCancel := context.WithTimeout(ctx, 30*time.Second)
			jsResult, jsFetchErr := jsDrv.Fetch(jsCtx, targetURL, &driver.FetchOptions{
				Headers:    opts.Headers,
				WaitFor:    opts.WaitFor,
				ProxyURL:   opts.ProxyURL,
				Mobile:     opts.Mobile,
				Viewport:   opts.Viewport,
				Location:   opts.Location,
				BlockAds:   opts.BlockAds,
				AutoScroll: opts.AutoScroll,
				BlockMedia: !containsFormat(opts.Formats, "screenshot"),
			})
			jsCancel()
			if jsFetchErr == nil {
				fetchResult = jsResult
				drv = jsDrv
				html, err = drv.HTML(ctx)
				if err != nil || strings.TrimSpace(html) == "" {
					html = fetchResult.HTML
				}
			} else {
				_ = jsDrv.Close()
			}
		}
	}

	// ── Agent-assisted interaction (Model Plane v2) ───────────────────────
	// When the rendered page still has insufficient content and a goal/prompt
	// is provided, hand control to ai-core's AgentNavigate to interact with
	// the page (dismiss cookie banners, click "load more", navigate tabs, etc.)
	// before extracting. Skipped when explicit actions are supplied by the
	// caller (they take precedence) or when no AI client is available.
	if len(opts.Actions) == 0 && s.aiClient != nil && opts.AgentGoal != "" && isContentInsufficient(html) {
		html = agentAssistedFetch(ctx, drv, s.aiClient, targetURL,
			opts.AgentGoal, opts.AgentSchema, opts.OrgID, html)
	}

	actionResults := make([]actions.ActionResult, 0)
	if len(opts.Actions) > 0 {
		actionResults, err = actions.Execute(ctx, drv, opts.Actions)
		if err != nil {
			return nil, actionResults, err
		}
	}

	filteredHTML, err := applyTagFilters(html, opts.IncludeTags, opts.ExcludeTags)
	if err != nil {
		filteredHTML = html
	}

	// ── Content transformer pipeline — clean, resolve links, fix lazy images ──
	contentDoc := &transform.ContentDoc{
		URL:         fetchResult.URL,
		HTML:        filteredHTML,
		ContentType: fetchResult.ContentType,
	}
	pipeline := transform.DefaultContentPipeline()
	if pipeErr := pipeline.Run(ctx, contentDoc); pipeErr == nil {
		filteredHTML = contentDoc.HTML
	}

	formats := normalizeFormats(opts.Formats)
	outputs := make(map[string]interface{}, len(formats))
	mediaExt := strings.ToLower(path.Ext(fetchResult.URL))
	isMediaURL := mediaExt == ".pdf" || mediaExt == ".docx" || mediaExt == ".xlsx"

	for _, format := range formats {
		switch format {
		case "json":
			if isMediaURL {
				mediaResult, mediaErr := transform.ParseMediaURL(ctx, fetchResult.URL, opts.Headers, opts.ParserMode)
				if mediaErr != nil {
					return nil, actionResults, mediaErr
				}
				outputs[format] = mediaResult
				continue
			}
			outputs[format] = map[string]interface{}{
				"url":         fetchResult.URL,
				"status":      fetchResult.Status,
				"contentType": fetchResult.ContentType,
				"links":       fetchResult.Links,
				"rendered":    fetchResult.Rendered,
			}
		case "html":
			outputs[format] = filteredHTML
		case "rawhtml":
			outputs[format] = fetchResult.RawHTML
		case "links":
			outputs[format] = fetchResult.Links
		case "markdown":
			if isMediaURL {
				mediaResult, mediaErr := transform.ParseMediaURL(ctx, fetchResult.URL, opts.Headers, opts.ParserMode)
				if mediaErr != nil {
					return nil, actionResults, mediaErr
				}
				outputs[format] = mediaResult.Text
				continue
			}
			markdown, convErr := transform.HTMLToMarkdown(fetchResult.URL, filteredHTML, opts.OnlyMainContent)
			if convErr != nil {
				return nil, actionResults, convErr
			}
			outputs[format] = markdown
		case "chunks":
			// Semantic chunking: convert to markdown, then split into LLM-friendly chunks.
			md, convErr := transform.HTMLToMarkdown(fetchResult.URL, filteredHTML, true)
			if convErr != nil {
				return nil, actionResults, convErr
			}
			outputs[format] = transform.ChunkMarkdown(md, transform.ChunkOptions{
				MaxTokens: 512,
				Overlap:   50,
			})
		case "media":
			mediaResult, mediaErr := transform.ParseMediaURL(ctx, fetchResult.URL, opts.Headers, opts.ParserMode)
			if mediaErr != nil {
				return nil, actionResults, mediaErr
			}
			outputs[format] = mediaResult
		case "seo":
			outputs[format] = analyzeSEO(fetchResult.URL, filteredHTML)
		case "wcag":
			outputs[format] = analyzeWCAG(filteredHTML)
		case "pagestatus":
			outputs[format] = buildPageStatus(fetchResult.URL, fetchResult.Status, fetchResult.ContentType)
		case "screenshot":
			shot, shotErr := drv.Screenshot(ctx, true)
			if shotErr != nil {
				return nil, actionResults, shotErr
			}
			outputs[format] = shot
		case "pdf":
			pdfBytes, pdfErr := drv.GeneratePDF(ctx)
			if pdfErr != nil {
				return nil, actionResults, pdfErr
			}
			outputs[format] = pdfBytes
		case "summary":
			// AI-generated page summary via AI-Core SummarizeContent RPC.
			if s.aiClient != nil {
				resp, summaryErr := s.aiClient.SummarizeContent(ctx, &ai.SummarizeRequest{
					HTML: filteredHTML,
					URL:  fetchResult.URL,
				})
				if summaryErr != nil {
					// Fallback: use the first 500 chars of markdown.
					md, convErr := transform.HTMLToMarkdown(fetchResult.URL, filteredHTML, true)
					if convErr == nil && len(md) > 500 {
						md = md[:500] + "..."
					}
					outputs[format] = md
				} else {
					outputs[format] = resp.Summary
				}
			} else {
				// No AI client: fallback to truncated markdown.
				md, convErr := transform.HTMLToMarkdown(fetchResult.URL, filteredHTML, true)
				if convErr == nil && len(md) > 500 {
					md = md[:500] + "..."
				}
				outputs[format] = md
			}
		case "branding":
			outputs[format] = extractBranding(fetchResult.URL, filteredHTML)
		case "attributes":
			outputs[format] = extractAttributes(ctx, filteredHTML, opts.Attributes)
		case "youtube":
			outputs[format] = extractYouTubeMetadata(fetchResult.URL, filteredHTML)
		case "pagination":
			outputs[format] = DetectPaginationLinks(filteredHTML)
		case "knowledge_article":
			// Structured extraction for knowledge base ingestion (Zendesk / Intercom / Notion).
			// Returns a KnowledgeArticle with title, summary, body (markdown), tags,
			// author, published date, content type, word count, and reading time.
			outputs[format] = extractKnowledgeArticle(ctx, fetchResult.URL, filteredHTML)
		default:
			outputs[format] = nil
		}
	}

	// ── Write to format-level cache ─────────────────────────────────────────
	if canCache && len(outputs) > 0 {
		_ = s.cacheManager.CacheFormats(ctx, targetURL, opts.Formats, outputs, cacheMaxAge)
	}

	return outputs, actionResults, nil
}

func normalizeFormats(formats []string) []string {
	if len(formats) == 0 {
		return []string{"json"}
	}
	seen := make(map[string]struct{}, len(formats))
	result := make([]string, 0, len(formats))
	for _, f := range formats {
		normalized := strings.ToLower(strings.TrimSpace(f))
		if normalized == "rawhtml" {
			normalized = "rawhtml"
		}
		if normalized == "" {
			continue
		}
		if _, exists := seen[normalized]; exists {
			continue
		}
		seen[normalized] = struct{}{}
		result = append(result, normalized)
	}
	if len(result) == 0 {
		return []string{"json"}
	}
	return result
}

func containsFormat(formats []string, expected string) bool {
	for _, format := range formats {
		if strings.EqualFold(strings.TrimSpace(format), expected) {
			return true
		}
	}
	return false
}

// isContentInsufficient returns true when an HTML response has too little
// visible text to be considered a fully-rendered page. Drives the
// render-and-validate escalation strategy: escalate to Rod whenever a
// non-JS response contains fewer than 200 visible characters, regardless of
// which framework (if any) built the page.
func isContentInsufficient(html string) bool {
	if strings.TrimSpace(html) == "" {
		return true
	}
	// Strip all tags to measure visible text.
	var b strings.Builder
	inTag := false
	for _, r := range html {
		switch {
		case r == '<':
			inTag = true
		case r == '>':
			inTag = false
		case !inTag:
			b.WriteRune(r)
		}
	}
	return len(strings.TrimSpace(b.String())) < 200
}

func applyTagFilters(html string, includeTags, excludeTags []string) (string, error) {
	doc, err := goquery.NewDocumentFromReader(strings.NewReader(html))
	if err != nil {
		return "", err
	}

	for _, tag := range excludeTags {
		t := strings.TrimSpace(tag)
		if t == "" {
			continue
		}
		doc.Find(t).Remove()
	}

	if len(includeTags) == 0 {
		bodyHTML, bodyErr := doc.Find("body").Html()
		if bodyErr != nil {
			return html, nil
		}
		return bodyHTML, nil
	}

	fragments := make([]string, 0, len(includeTags))
	for _, tag := range includeTags {
		t := strings.TrimSpace(tag)
		if t == "" {
			continue
		}
		doc.Find(t).Each(func(i int, selection *goquery.Selection) {
			part, htmlErr := goquery.OuterHtml(selection)
			if htmlErr == nil && strings.TrimSpace(part) != "" {
				fragments = append(fragments, part)
			}
		})
	}
	if len(fragments) == 0 {
		return "", nil
	}
	return strings.Join(fragments, "\n"), nil
}
