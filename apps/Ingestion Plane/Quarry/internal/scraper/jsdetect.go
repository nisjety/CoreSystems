package scraper

import (
	"regexp"
	"strings"
)

// SPASignal describes a heuristic match indicating a page likely requires
// JavaScript rendering to produce meaningful content.
type SPASignal struct {
	Name       string
	Confidence float64 // 0.0 – 1.0
}

// SPADetectionResult holds both the signals and the detected framework name.
type SPADetectionResult struct {
	Signals   []SPASignal
	Framework string // e.g. "react", "nextjs", "vue", "nuxt", "angular", "svelte", "sveltekit", "gatsby", "remix", "astro", "qwik", ""
	NeedsJS   bool   // whether JS rendering is needed
}

// spaThreshold is the cumulative confidence score above which we consider a
// page an SPA that needs headless rendering.
const spaThreshold = 0.6

// minContentLen is the minimum body text length (after stripping tags) below
// which a page is considered "content-thin" and a candidate for JS escalation.
// Keep this conservative: many legitimate simple pages (e.g. landing pages,
// API endpoints, or example.com) have sparse text and are NOT SPAs.
const minContentLen = 100

// DetectSPASignals inspects raw HTML returned by a non-JS driver (Colly/HTTP)
// and returns a list of heuristic signals suggesting the page is a JavaScript
// rendered SPA. The caller can sum the confidence values and compare against
// spaThreshold to decide whether to re-fetch with a headless browser.
func DetectSPASignals(html string) []SPASignal {
	if html == "" {
		return nil
	}

	var signals []SPASignal
	lower := strings.ToLower(html)

	// ── 1. Empty root containers typical of React / Vue / Angular SPAs ──────
	if reEmptyRoot.MatchString(html) {
		signals = append(signals, SPASignal{Name: "empty-root-container", Confidence: 0.7})
	}

	// ── 2. <noscript> tag with fallback message ─────────────────────────────
	if strings.Contains(lower, "<noscript") {
		if reNoscriptEnable.MatchString(lower) {
			signals = append(signals, SPASignal{Name: "noscript-enable-js", Confidence: 0.6})
		} else {
			signals = append(signals, SPASignal{Name: "noscript-present", Confidence: 0.3})
		}
	}

	// ── 3. Framework-specific markers ───────────────────────────────────────
	// React / Next.js
	if strings.Contains(lower, "__next_data__") || strings.Contains(lower, "__next") || strings.Contains(html, "data-reactroot") {
		signals = append(signals, SPASignal{Name: "react-nextjs", Confidence: 0.8})
	}
	// Nuxt / Vue
	if strings.Contains(lower, "__nuxt") || strings.Contains(html, "data-v-") || strings.Contains(lower, "__vue__") {
		signals = append(signals, SPASignal{Name: "vue-nuxt", Confidence: 0.8})
	}
	// Angular
	if strings.Contains(html, "ng-version") || strings.Contains(html, "ng-app") || strings.Contains(lower, "angular") && strings.Contains(lower, "app-root") {
		signals = append(signals, SPASignal{Name: "angular", Confidence: 0.8})
	}
	// Svelte / SvelteKit
	if strings.Contains(lower, "svelte") && strings.Contains(lower, "__sveltekit") {
		signals = append(signals, SPASignal{Name: "sveltekit", Confidence: 0.8})
	}
	// Gatsby
	if strings.Contains(lower, "___gatsby") {
		signals = append(signals, SPASignal{Name: "gatsby", Confidence: 0.8})
	}
	// Remix
	if strings.Contains(lower, "__remix") || strings.Contains(lower, "__remixcontext") {
		signals = append(signals, SPASignal{Name: "remix", Confidence: 0.5})
	}
	// Astro
	if strings.Contains(lower, "astro-island") || strings.Contains(lower, "data-astro") {
		signals = append(signals, SPASignal{Name: "astro", Confidence: 0.7})
	}
	// Qwik
	if strings.Contains(lower, "q:container") || strings.Contains(lower, "qwik") && strings.Contains(lower, "q:") {
		signals = append(signals, SPASignal{Name: "qwik", Confidence: 0.8})
	}

	// ── 4. Heavy JS bundle loading (many script tags, few content tags) ─────
	scriptCount := strings.Count(lower, "<script")
	pCount := strings.Count(lower, "<p")
	if scriptCount > 5 && pCount < 2 {
		signals = append(signals, SPASignal{Name: "heavy-js-few-paragraphs", Confidence: 0.5})
	}

	// ── 5. Content-thin body: very little visible text ──────────────────────
	// Low confidence alone — a lightweight static page is NOT necessarily
	// a SPA. This signal only tips the balance when combined with others.
	bodyText := extractVisibleText(html)
	if len(bodyText) < minContentLen {
		signals = append(signals, SPASignal{Name: "content-thin", Confidence: 0.3})
	}

	return signals
}

// NeedsJSRendering returns true if the cumulative SPA signal confidence
// exceeds spaThreshold.
//
// Exception — SSR bypass: if the page has SPA framework signals but NOT an
// empty-root container (classic CSR pattern), and the colly-fetched HTML
// already contains rich visible text (> 2000 chars), we trust the static
// fetch and skip Rod. This correctly handles Next.js/React SSR pages like
// MDN, Next.js docs, etc., which look like SPAs but actually ship full
// content in the initial HTML response.
func NeedsJSRendering(html string) bool {
	signals := DetectSPASignals(html)
	var total float64
	hasEmptyRoot := false
	for _, s := range signals {
		total += s.Confidence
		if s.Name == "empty-root-container" {
			hasEmptyRoot = true
		}
	}
	if total < spaThreshold {
		return false
	}
	// SPA signals crossed the threshold. But if there's no empty-root
	// container AND the page has rich visible text, the framework is running
	// in SSR mode — colly already captured the content.
	if !hasEmptyRoot {
		if len(extractVisibleText(html)) > 2000 {
			return false // SSR SPA with good colly content — skip Rod
		}
	}
	return true
}

// DetectSPA returns a full detection result including the framework name
// and whether JS rendering is needed. This is used by the adaptive wait
// engine to select framework-specific wait strategies.
func DetectSPA(html string) SPADetectionResult {
	signals := DetectSPASignals(html)

	// Determine framework from signals.
	framework := ""
	for _, s := range signals {
		switch s.Name {
		case "react-nextjs":
			// Distinguish Next.js from plain React
			lower := strings.ToLower(html)
			if strings.Contains(lower, "__next_data__") || strings.Contains(lower, "__next") {
				framework = "nextjs"
			} else {
				framework = "react"
			}
		case "vue-nuxt":
			lower := strings.ToLower(html)
			if strings.Contains(lower, "__nuxt") {
				framework = "nuxt"
			} else {
				framework = "vue"
			}
		case "angular":
			framework = "angular"
		case "sveltekit":
			if strings.Contains(strings.ToLower(html), "__sveltekit") {
				framework = "sveltekit"
			} else {
				framework = "svelte"
			}
		case "gatsby":
			framework = "gatsby"
		case "remix":
			framework = "remix"
		case "astro":
			framework = "astro"
		case "qwik":
			framework = "qwik"
		}
		if framework != "" {
			break // use the first (highest confidence) match
		}
	}

	needsJS := NeedsJSRendering(html)
	return SPADetectionResult{
		Signals:   signals,
		Framework: framework,
		NeedsJS:   needsJS,
	}
}

// ---------- Helpers ----------

// reEmptyRoot matches common SPA root containers that are empty or near-empty.
// Examples: <div id="root"></div>, <div id="app"></div>, <div id="__next"></div>
var reEmptyRoot = regexp.MustCompile(`(?i)<div\s+id=["'](root|app|__next|__nuxt|___gatsby|main)["'][^>]*>\s*</div>`)

// reNoscriptEnable matches noscript blocks that ask the user to enable JS.
var reNoscriptEnable = regexp.MustCompile(`(?i)<noscript[^>]*>.*?(enable|activate|requires?|needs?)\s+(javascript|js).*?</noscript>`)

// extractVisibleText does a rough extraction of visible text from HTML by
// stripping all tags and collapsing whitespace. Not meant to be precise —
// just enough for a content-length heuristic.
func extractVisibleText(html string) string {
	// Remove script and style blocks entirely.
	cleaned := reScriptBlock.ReplaceAllString(html, "")
	cleaned = reStyleBlock.ReplaceAllString(cleaned, "")
	// Remove all remaining tags.
	cleaned = reHTMLTag.ReplaceAllString(cleaned, " ")
	// Collapse whitespace.
	cleaned = reWhitespace.ReplaceAllString(cleaned, " ")
	return strings.TrimSpace(cleaned)
}

var (
	reScriptBlock = regexp.MustCompile(`(?is)<script[^>]*>.*?</script>`)
	reStyleBlock  = regexp.MustCompile(`(?is)<style[^>]*>.*?</style>`)
	reHTMLTag     = regexp.MustCompile(`<[^>]+>`)
	reWhitespace  = regexp.MustCompile(`\s+`)
)
